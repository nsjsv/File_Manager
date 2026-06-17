use std::collections::HashMap;
use std::fs as std_fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};

use super::catalog::SearchCatalogRecord;
use super::path_encoding::{path_from_bytes, path_storage_key, path_to_bytes};
use super::search_index_error;
use super::types::{
    file_kind_from_key, file_kind_key, FileSearchIndexFailure, FileSearchIndexStatus,
    IGNORE_POLICY_VERSION, INDEX_FORMAT_VERSION,
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
const MANIFEST_EXCLUDE_RULES_HASH: &str = "exclude_rules_hash";
const MANIFEST_BUILT_AT_MS: &str = "built_at_ms";
const MANIFEST_UPDATED_AT_MS: &str = "updated_at_ms";
const MANIFEST_INDEX_SIZE_BYTES: &str = "index_size_bytes";
const MANIFEST_FAILED_COUNT: &str = "failed_count";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchCatalogIdentity {
    pub(crate) generation: String,
    pub(crate) record_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchIndexManifest {
    pub(crate) format_version: u32,
    pub(crate) root_key: String,
    pub(crate) root_text: String,
    pub(crate) include_hidden: bool,
    pub(crate) ignore_policy_version: u32,
    pub(crate) record_count: usize,
    pub(crate) generation: String,
    pub(crate) exclude_rules_hash: String,
    pub(crate) built_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) index_size_bytes: u64,
    pub(crate) failed_count: usize,
}

impl SearchIndexManifest {
    pub(crate) fn new(
        root: &Path,
        include_hidden: bool,
        exclude_patterns: &[String],
        record_count: usize,
        failed_count: usize,
        built_at_ms: Option<i64>,
    ) -> Self {
        let updated_at_ms = current_time_ms();
        let built_at_ms = built_at_ms.unwrap_or(updated_at_ms);
        let root_key = path_storage_key(root);
        let exclude_rules_hash = exclude_rules_hash(exclude_patterns);
        let generation = format!(
            "{updated_at_ms}:{record_count}:{failed_count}:{root_key}:{include_hidden}:{exclude_rules_hash}"
        );

        Self {
            format_version: INDEX_FORMAT_VERSION,
            root_key,
            root_text: root.to_string_lossy().into_owned(),
            include_hidden,
            ignore_policy_version: IGNORE_POLICY_VERSION,
            record_count,
            generation,
            exclude_rules_hash,
            built_at_ms,
            updated_at_ms,
            index_size_bytes: 0,
            failed_count,
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
        exclude_patterns: &[String],
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
        if self.exclude_rules_hash != exclude_rules_hash(exclude_patterns) {
            return Err(search_index_error(
                index_dir,
                "search index exclude rules are outdated",
            ));
        }
        Ok(())
    }

    pub(crate) fn to_status(
        &self,
        root: PathBuf,
        index_dir: PathBuf,
        failures: Vec<FileSearchIndexFailure>,
        index_size_bytes: u64,
    ) -> FileSearchIndexStatus {
        FileSearchIndexStatus {
            root,
            index_dir,
            exists: true,
            include_hidden: self.include_hidden,
            record_count: self.record_count,
            index_size_bytes,
            built_at_ms: Some(self.built_at_ms),
            updated_at_ms: Some(self.updated_at_ms),
            failed_count: failures.len().max(self.failed_count),
            exclude_rules_hash: Some(self.exclude_rules_hash.clone()),
            failures,
        }
    }

    fn entries(&self) -> Vec<(&'static str, String)> {
        vec![
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
            (MANIFEST_EXCLUDE_RULES_HASH, self.exclude_rules_hash.clone()),
            (MANIFEST_BUILT_AT_MS, self.built_at_ms.to_string()),
            (MANIFEST_UPDATED_AT_MS, self.updated_at_ms.to_string()),
            (MANIFEST_INDEX_SIZE_BYTES, self.index_size_bytes.to_string()),
            (MANIFEST_FAILED_COUNT, self.failed_count.to_string()),
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
    if !index_dir.exists() {
        return std_fs::rename(pending_index_dir, index_dir)
            .map_err(|error| search_index_error(index_dir, error));
    }

    let pending_catalog_path = catalog_path(pending_index_dir);
    let target_catalog_path = catalog_path(index_dir);
    std_fs::rename(&pending_catalog_path, &target_catalog_path)
        .map_err(|error| search_index_error(index_dir, error))?;
    std_fs::remove_dir_all(pending_index_dir).map_err(|error| search_index_error(index_dir, error))
}

pub(crate) fn read_manifest(index_dir: &Path) -> Result<SearchIndexManifest, FileError> {
    let connection = open_catalog_connection(index_dir)?;
    read_manifest_from_connection(index_dir, &connection)
}

pub(crate) fn write_catalog(
    index_dir: &Path,
    manifest: &mut SearchIndexManifest,
    records: &[SearchCatalogRecord],
    failures: &[FileSearchIndexFailure],
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
                kind TEXT NOT NULL,
                mtime_ms INTEGER,
                size_bytes INTEGER,
                changed_at_generation TEXT
             );
             CREATE TABLE failures (
                path_key TEXT PRIMARY KEY,
                path BLOB NOT NULL,
                message TEXT NOT NULL,
                first_failed_at_ms INTEGER NOT NULL,
                last_failed_at_ms INTEGER NOT NULL,
                retry_count INTEGER NOT NULL
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
            .prepare(
                "INSERT INTO entries (
                    path_key, path, kind, mtime_ms, size_bytes, changed_at_generation
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|error| search_index_error(&catalog_path, error))?;
        for record in records {
            insert_entry
                .execute(params![
                    record.storage_key.as_str(),
                    path_to_bytes(&record.path),
                    file_kind_key(record.kind),
                    record.mtime_ms,
                    record.size_bytes.map(saturating_u64_to_i64),
                    manifest.generation.as_str(),
                ])
                .map_err(|error| search_index_error(&catalog_path, error))?;
        }
    }
    {
        let mut insert_failure = tx
            .prepare(
                "INSERT INTO failures (
                    path_key, path, message, first_failed_at_ms, last_failed_at_ms, retry_count
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|error| search_index_error(&catalog_path, error))?;
        for failure in failures {
            insert_failure
                .execute(params![
                    path_storage_key(&failure.path),
                    path_to_bytes(&failure.path),
                    failure.message.as_str(),
                    failure.first_failed_at_ms,
                    failure.last_failed_at_ms,
                    i64::from(failure.retry_count),
                ])
                .map_err(|error| search_index_error(&catalog_path, error))?;
        }
    }
    tx.commit()
        .map_err(|error| search_index_error(&catalog_path, error))?;

    persist_catalog_size(index_dir, manifest)
}

pub(crate) fn load_catalog(
    index_dir: &Path,
    root: &Path,
    include_hidden: bool,
    exclude_patterns: &[String],
) -> Result<(SearchIndexManifest, Vec<SearchCatalogRecord>), FileError> {
    let connection = open_catalog_connection(index_dir)?;
    let manifest = read_manifest_from_connection(index_dir, &connection)?;
    manifest.validate_for(index_dir, root, include_hidden, exclude_patterns)?;
    let mut statement = connection
        .prepare("SELECT path, kind, mtime_ms, size_bytes FROM entries ORDER BY id")
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
        let mtime_ms = row
            .get::<_, Option<i64>>(2)
            .map_err(|error| search_index_error(index_dir, error))?;
        let size_bytes = row
            .get::<_, Option<i64>>(3)
            .map_err(|error| search_index_error(index_dir, error))?
            .and_then(|value| u64::try_from(value).ok());
        let kind = file_kind_from_key(&kind_key).unwrap_or(FileKind::Other);
        records.push(SearchCatalogRecord::from_path_with_index_metadata(
            root,
            path_from_bytes(path_bytes),
            kind,
            mtime_ms,
            size_bytes,
        ));
    }

    Ok((manifest, records))
}

pub(crate) fn read_index_status(
    index_dir: &Path,
    root: &Path,
    include_hidden: bool,
    exclude_patterns: &[String],
) -> Result<FileSearchIndexStatus, FileError> {
    let connection = match open_catalog_connection(index_dir) {
        Ok(connection) => connection,
        Err(_) => {
            return Ok(FileSearchIndexStatus::missing(
                root.to_path_buf(),
                index_dir.to_path_buf(),
                include_hidden,
            ));
        }
    };
    let manifest = read_manifest_from_connection(index_dir, &connection)?;
    manifest.validate_for(index_dir, root, include_hidden, exclude_patterns)?;
    let failures = read_failures_from_connection(index_dir, &connection)?;
    let index_size_bytes = catalog_dir_size(index_dir).unwrap_or(manifest.index_size_bytes);
    Ok(manifest.to_status(
        root.to_path_buf(),
        index_dir.to_path_buf(),
        failures,
        index_size_bytes,
    ))
}

pub(crate) fn read_failures(index_dir: &Path) -> Result<Vec<FileSearchIndexFailure>, FileError> {
    let connection = open_catalog_connection(index_dir)?;
    read_failures_from_connection(index_dir, &connection)
}

pub(crate) fn clear_failures(index_dir: &Path) -> Result<(), FileError> {
    let connection = open_catalog_connection(index_dir)?;
    connection
        .execute("DELETE FROM failures", [])
        .map_err(|error| search_index_error(index_dir, error))?;
    connection
        .execute(
            "UPDATE manifest SET value = '0' WHERE key = ?1",
            params![MANIFEST_FAILED_COUNT],
        )
        .map_err(|error| search_index_error(index_dir, error))?;
    Ok(())
}

pub(crate) fn remove_catalog_dir(index_dir: &Path) -> Result<(), FileError> {
    if !index_dir.exists() {
        return Ok(());
    }
    std_fs::remove_dir_all(index_dir).map_err(|error| search_index_error(index_dir, error))
}

pub(crate) fn catalog_dir_size(index_dir: &Path) -> Result<u64, FileError> {
    catalog_dir_size_inner(index_dir).map_err(|error| search_index_error(index_dir, error))
}

fn persist_catalog_size(
    index_dir: &Path,
    manifest: &mut SearchIndexManifest,
) -> Result<(), FileError> {
    let index_size_bytes = catalog_dir_size(index_dir)?;
    manifest.index_size_bytes = index_size_bytes;

    let catalog_path = catalog_path(index_dir);
    let connection = Connection::open(&catalog_path)
        .map_err(|error| search_index_error(&catalog_path, error))?;
    connection
        .execute(
            "UPDATE manifest SET value = ?1 WHERE key = ?2",
            params![
                manifest.index_size_bytes.to_string(),
                MANIFEST_INDEX_SIZE_BYTES
            ],
        )
        .map_err(|error| search_index_error(&catalog_path, error))?;
    Ok(())
}

pub(crate) fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

pub(crate) fn exclude_rules_hash(patterns: &[String]) -> String {
    let mut normalized = patterns
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
        .collect::<Vec<_>>();
    normalized.sort_unstable();

    let mut hash = 0xcbf29ce484222325u64;
    for pattern in normalized {
        for byte in pattern.as_bytes().iter().chain(std::iter::once(&0)) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
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
        exclude_rules_hash: required_manifest_value(
            index_dir,
            &values,
            MANIFEST_EXCLUDE_RULES_HASH,
        )?,
        built_at_ms: parse_manifest_i64(index_dir, &values, MANIFEST_BUILT_AT_MS)?,
        updated_at_ms: parse_manifest_i64(index_dir, &values, MANIFEST_UPDATED_AT_MS)?,
        index_size_bytes: parse_manifest_u64(index_dir, &values, MANIFEST_INDEX_SIZE_BYTES)?,
        failed_count: parse_manifest_usize(index_dir, &values, MANIFEST_FAILED_COUNT)?,
    })
}

fn read_failures_from_connection(
    index_dir: &Path,
    connection: &Connection,
) -> Result<Vec<FileSearchIndexFailure>, FileError> {
    let mut statement = connection
        .prepare(
            "SELECT path, message, first_failed_at_ms, last_failed_at_ms, retry_count
             FROM failures
             ORDER BY last_failed_at_ms DESC, path_key",
        )
        .map_err(|error| search_index_error(index_dir, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| search_index_error(index_dir, error))?;
    let mut failures = Vec::new();

    while let Some(row) = rows
        .next()
        .map_err(|error| search_index_error(index_dir, error))?
    {
        let path_bytes = row
            .get::<_, Vec<u8>>(0)
            .map_err(|error| search_index_error(index_dir, error))?;
        let message = row
            .get::<_, String>(1)
            .map_err(|error| search_index_error(index_dir, error))?;
        let first_failed_at_ms = row
            .get::<_, i64>(2)
            .map_err(|error| search_index_error(index_dir, error))?;
        let last_failed_at_ms = row
            .get::<_, i64>(3)
            .map_err(|error| search_index_error(index_dir, error))?;
        let retry_count = row
            .get::<_, i64>(4)
            .map_err(|error| search_index_error(index_dir, error))?;
        failures.push(FileSearchIndexFailure {
            path: path_from_bytes(path_bytes),
            message,
            first_failed_at_ms,
            last_failed_at_ms,
            retry_count: u32::try_from(retry_count).unwrap_or(u32::MAX),
        });
    }

    Ok(failures)
}

fn catalog_path(index_dir: &Path) -> PathBuf {
    index_dir.join(CATALOG_FILE_NAME)
}

fn catalog_dir_size_inner(path: &Path) -> std::io::Result<u64> {
    let metadata = std_fs::symlink_metadata(path)?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }

    let mut size = 0u64;
    for entry in std_fs::read_dir(path)? {
        let entry = entry?;
        size = size.saturating_add(catalog_dir_size_inner(&entry.path())?);
    }
    Ok(size)
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

fn parse_manifest_i64(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<i64, FileError> {
    required_manifest_value(index_dir, values, key)?
        .parse()
        .map_err(|error| search_index_error(index_dir, error))
}

fn parse_manifest_u64(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<u64, FileError> {
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

fn saturating_u64_to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclude_rules_hash_is_order_independent() {
        let left = vec!["target/".to_owned(), "node_modules/".to_owned()];
        let right = vec!["node_modules/".to_owned(), "target/".to_owned()];

        assert_eq!(exclude_rules_hash(&left), exclude_rules_hash(&right));
    }
}
