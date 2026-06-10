use std::collections::HashMap;
use std::fs as std_fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};

use super::catalog::SearchCatalogRecord;
use super::path_encoding::{path_from_bytes, path_storage_key, path_to_bytes};
use super::search_index_error;
use super::types::{
    file_kind_from_key, file_kind_key, IGNORE_POLICY_VERSION, INDEX_FORMAT_VERSION,
};
use crate::{FileError, FileKind};

const CATALOG_FILE_NAME: &str = "catalog.sqlite";
const MANIFEST_FORMAT_VERSION: &str = "format_version";
const MANIFEST_ROOT_KEY: &str = "root_key";
const MANIFEST_ROOT_TEXT: &str = "root_text";
const MANIFEST_INCLUDE_HIDDEN: &str = "include_hidden";
const MANIFEST_IGNORE_POLICY_VERSION: &str = "ignore_policy_version";
const MANIFEST_RECORD_COUNT: &str = "record_count";
const MANIFEST_GENERATION: &str = "generation";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchCatalogIdentity {
    pub(crate) generation: String,
    pub(crate) record_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchIndexManifest {
    format_version: u32,
    root_key: String,
    root_text: String,
    include_hidden: bool,
    ignore_policy_version: u32,
    record_count: usize,
    generation: String,
}

impl SearchIndexManifest {
    pub(crate) fn new(root: &Path, include_hidden: bool, record_count: usize) -> Self {
        let built_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let root_key = path_storage_key(root);
        let generation = format!("{built_at}:{record_count}:{root_key}:{include_hidden}");

        Self {
            format_version: INDEX_FORMAT_VERSION,
            root_key,
            root_text: root.to_string_lossy().into_owned(),
            include_hidden,
            ignore_policy_version: IGNORE_POLICY_VERSION,
            record_count,
            generation,
        }
    }

    pub(crate) fn identity(&self) -> SearchCatalogIdentity {
        SearchCatalogIdentity {
            generation: self.generation.clone(),
            record_count: self.record_count,
        }
    }

    pub(crate) fn validate_for(
        &self,
        index_dir: &Path,
        root: &Path,
        include_hidden: bool,
    ) -> Result<(), FileError> {
        if self.format_version != INDEX_FORMAT_VERSION {
            return Err(search_index_error(
                index_dir,
                "search index format is outdated",
            ));
        }
        if self.ignore_policy_version != IGNORE_POLICY_VERSION {
            return Err(search_index_error(
                index_dir,
                "search index ignore policy is outdated",
            ));
        }
        if self.root_key != path_storage_key(root) {
            return Err(search_index_error(
                index_dir,
                "search index root does not match",
            ));
        }
        if self.include_hidden != include_hidden {
            return Err(search_index_error(
                index_dir,
                "search index options do not match current hidden-file setting",
            ));
        }
        Ok(())
    }

    fn entries(&self) -> [(&'static str, String); 7] {
        [
            (MANIFEST_FORMAT_VERSION, self.format_version.to_string()),
            (MANIFEST_ROOT_KEY, self.root_key.clone()),
            (MANIFEST_ROOT_TEXT, self.root_text.clone()),
            (MANIFEST_INCLUDE_HIDDEN, self.include_hidden.to_string()),
            (
                MANIFEST_IGNORE_POLICY_VERSION,
                self.ignore_policy_version.to_string(),
            ),
            (MANIFEST_RECORD_COUNT, self.record_count.to_string()),
            (MANIFEST_GENERATION, self.generation.clone()),
        ]
    }
}

pub(crate) fn prepare_catalog_dir(root: &Path, pending_index_dir: &Path) -> Result<(), FileError> {
    if let Some(parent) = pending_index_dir.parent() {
        std_fs::create_dir_all(parent).map_err(|error| search_index_error(root, error))?;
    }
    if pending_index_dir.exists() {
        std_fs::remove_dir_all(pending_index_dir)
            .map_err(|error| search_index_error(root, error))?;
    }
    std_fs::create_dir_all(pending_index_dir).map_err(|error| search_index_error(root, error))
}

pub(crate) fn replace_catalog_dir(
    index_dir: &Path,
    pending_index_dir: &Path,
) -> Result<(), FileError> {
    if index_dir.exists() {
        std_fs::remove_dir_all(index_dir).map_err(|error| search_index_error(index_dir, error))?;
    }
    std_fs::rename(pending_index_dir, index_dir)
        .map_err(|error| search_index_error(index_dir, error))
}

pub(crate) fn read_manifest(index_dir: &Path) -> Result<SearchIndexManifest, FileError> {
    let connection = open_catalog_connection(index_dir)?;
    read_manifest_from_connection(index_dir, &connection)
}

pub(crate) fn write_catalog(
    index_dir: &Path,
    manifest: &SearchIndexManifest,
    records: &[SearchCatalogRecord],
) -> Result<(), FileError> {
    let catalog_path = catalog_path(index_dir);
    let mut connection = Connection::open(&catalog_path)
        .map_err(|error| search_index_error(&catalog_path, error))?;
    connection
        .execute_batch(
            "PRAGMA journal_mode = OFF;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE manifest (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE entries (
                id INTEGER PRIMARY KEY,
                path_key TEXT NOT NULL UNIQUE,
                path BLOB NOT NULL,
                kind TEXT NOT NULL
             );",
        )
        .map_err(|error| search_index_error(&catalog_path, error))?;

    let tx = connection
        .transaction()
        .map_err(|error| search_index_error(&catalog_path, error))?;
    {
        let mut insert_manifest = tx
            .prepare("INSERT INTO manifest (key, value) VALUES (?1, ?2)")
            .map_err(|error| search_index_error(&catalog_path, error))?;
        for (key, value) in manifest.entries() {
            insert_manifest
                .execute(params![key, value])
                .map_err(|error| search_index_error(&catalog_path, error))?;
        }
    }
    {
        let mut insert_entry = tx
            .prepare("INSERT INTO entries (path_key, path, kind) VALUES (?1, ?2, ?3)")
            .map_err(|error| search_index_error(&catalog_path, error))?;
        for record in records {
            insert_entry
                .execute(params![
                    record.storage_key.as_str(),
                    path_to_bytes(&record.path),
                    file_kind_key(record.kind)
                ])
                .map_err(|error| search_index_error(&catalog_path, error))?;
        }
    }
    tx.commit()
        .map_err(|error| search_index_error(&catalog_path, error))
}

pub(crate) fn load_catalog(
    index_dir: &Path,
    root: &Path,
    include_hidden: bool,
) -> Result<(SearchIndexManifest, Vec<SearchCatalogRecord>), FileError> {
    let connection = open_catalog_connection(index_dir)?;
    let manifest = read_manifest_from_connection(index_dir, &connection)?;
    manifest.validate_for(index_dir, root, include_hidden)?;
    let mut statement = connection
        .prepare("SELECT path, kind FROM entries ORDER BY id")
        .map_err(|error| search_index_error(index_dir, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| search_index_error(index_dir, error))?;
    let mut records = Vec::with_capacity(manifest.record_count);

    while let Some(row) = rows
        .next()
        .map_err(|error| search_index_error(index_dir, error))?
    {
        let path_bytes = row
            .get::<_, Vec<u8>>(0)
            .map_err(|error| search_index_error(index_dir, error))?;
        let kind_key = row
            .get::<_, String>(1)
            .map_err(|error| search_index_error(index_dir, error))?;
        let kind = file_kind_from_key(&kind_key).unwrap_or(FileKind::Other);
        records.push(SearchCatalogRecord::from_path(
            root,
            path_from_bytes(path_bytes),
            kind,
        ));
    }

    Ok((manifest, records))
}

fn open_catalog_connection(index_dir: &Path) -> Result<Connection, FileError> {
    let catalog_path = catalog_path(index_dir);
    if !catalog_path.is_file() {
        return Err(search_index_error(index_dir, "search catalog is not ready"));
    }
    Connection::open(&catalog_path).map_err(|error| search_index_error(&catalog_path, error))
}

fn read_manifest_from_connection(
    index_dir: &Path,
    connection: &Connection,
) -> Result<SearchIndexManifest, FileError> {
    let mut statement = connection
        .prepare("SELECT key, value FROM manifest")
        .map_err(|error| search_index_error(index_dir, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| search_index_error(index_dir, error))?;
    let mut values = HashMap::new();

    while let Some(row) = rows
        .next()
        .map_err(|error| search_index_error(index_dir, error))?
    {
        let key = row
            .get::<_, String>(0)
            .map_err(|error| search_index_error(index_dir, error))?;
        let value = row
            .get::<_, String>(1)
            .map_err(|error| search_index_error(index_dir, error))?;
        values.insert(key, value);
    }

    Ok(SearchIndexManifest {
        format_version: parse_manifest_u32(index_dir, &values, MANIFEST_FORMAT_VERSION)?,
        root_key: required_manifest_value(index_dir, &values, MANIFEST_ROOT_KEY)?,
        root_text: required_manifest_value(index_dir, &values, MANIFEST_ROOT_TEXT)?,
        include_hidden: parse_manifest_bool(index_dir, &values, MANIFEST_INCLUDE_HIDDEN)?,
        ignore_policy_version: parse_manifest_u32(
            index_dir,
            &values,
            MANIFEST_IGNORE_POLICY_VERSION,
        )?,
        record_count: parse_manifest_usize(index_dir, &values, MANIFEST_RECORD_COUNT)?,
        generation: required_manifest_value(index_dir, &values, MANIFEST_GENERATION)?,
    })
}

fn catalog_path(index_dir: &Path) -> PathBuf {
    index_dir.join(CATALOG_FILE_NAME)
}

fn required_manifest_value(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<String, FileError> {
    values
        .get(key)
        .cloned()
        .ok_or_else(|| search_index_error(index_dir, format!("search manifest is missing {key}")))
}

fn parse_manifest_u32(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<u32, FileError> {
    required_manifest_value(index_dir, values, key)?
        .parse()
        .map_err(|error| search_index_error(index_dir, error))
}

fn parse_manifest_usize(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<usize, FileError> {
    required_manifest_value(index_dir, values, key)?
        .parse()
        .map_err(|error| search_index_error(index_dir, error))
}

fn parse_manifest_bool(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<bool, FileError> {
    required_manifest_value(index_dir, values, key)?
        .parse()
        .map_err(|error| search_index_error(index_dir, error))
}
