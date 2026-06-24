use std::fs as std_fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use super::catalog::SearchCatalogRecord;
use super::manifest::{
    read_manifest_from_connection, SearchIndexManifest, MANIFEST_FAILED_COUNT,
    MANIFEST_INDEX_SIZE_BYTES,
};
use super::path_encoding::{path_from_bytes, path_storage_key, path_to_bytes};
use super::search_index_error;
use super::types::{
    file_kind_from_key, file_kind_key, DirectoryErrorPolicy, FileSearchIndexFailure,
    FileSearchIndexStatus,
};
use crate::profile::MediaMetadataScope;
use crate::IndexError;
use file_core::FileKind;

const CATALOG_FILE_NAME: &str = "catalog.sqlite";

pub(crate) fn prepare_catalog_dir(root: &Path, pending_index_dir: &Path) -> Result<(), IndexError> {
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
) -> Result<(), IndexError> {
    if !index_dir.exists() {
        return std_fs::rename(pending_index_dir, index_dir)
            .map_err(|error| search_index_error(index_dir, error));
    }

    let previous_index_dir = index_dir.with_extension("previous");
    if previous_index_dir.exists() {
        std_fs::remove_dir_all(&previous_index_dir)
            .map_err(|error| search_index_error(&previous_index_dir, error))?;
    }
    std_fs::rename(index_dir, &previous_index_dir)
        .map_err(|error| search_index_error(index_dir, error))?;
    match std_fs::rename(pending_index_dir, index_dir) {
        Ok(()) => std_fs::remove_dir_all(&previous_index_dir)
            .map_err(|error| search_index_error(&previous_index_dir, error)),
        Err(error) => {
            let _ = std_fs::rename(&previous_index_dir, index_dir);
            Err(search_index_error(index_dir, error))
        }
    }
}

pub(crate) fn read_manifest(index_dir: &Path) -> Result<SearchIndexManifest, IndexError> {
    let connection = open_catalog_connection(index_dir)?;
    read_manifest_from_connection(index_dir, &connection)
}

pub(crate) fn write_catalog(
    index_dir: &Path,
    manifest: &mut SearchIndexManifest,
    records: &[SearchCatalogRecord],
    failures: &[FileSearchIndexFailure],
) -> Result<(), IndexError> {
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
    directory_error_policy: DirectoryErrorPolicy,
    content_index_enabled: bool,
    content_max_file_bytes: u64,
    media_metadata_scope: MediaMetadataScope,
) -> Result<(SearchIndexManifest, Vec<SearchCatalogRecord>), IndexError> {
    let connection = open_catalog_connection(index_dir)?;
    let manifest = read_manifest_from_connection(index_dir, &connection)?;
    manifest.validate_for(
        index_dir,
        root,
        include_hidden,
        exclude_patterns,
        directory_error_policy,
        content_index_enabled,
        content_max_file_bytes,
        media_metadata_scope,
    )?;
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
    directory_error_policy: DirectoryErrorPolicy,
    content_index_enabled: bool,
    content_max_file_bytes: u64,
    media_metadata_scope: MediaMetadataScope,
) -> Result<FileSearchIndexStatus, IndexError> {
    let connection = match open_catalog_connection(index_dir) {
        Ok(connection) => connection,
        Err(_) => {
            return Ok(FileSearchIndexStatus::missing(
                root.to_path_buf(),
                index_dir.to_path_buf(),
                include_hidden,
                content_index_enabled,
                content_max_file_bytes,
                media_metadata_scope,
            ));
        }
    };
    let manifest = read_manifest_from_connection(index_dir, &connection)?;
    if let Some(reason) = manifest.stale_reason_for(
        root,
        include_hidden,
        exclude_patterns,
        directory_error_policy,
        content_index_enabled,
        content_max_file_bytes,
        media_metadata_scope,
    ) {
        return Ok(FileSearchIndexStatus::stale(
            root.to_path_buf(),
            index_dir.to_path_buf(),
            include_hidden,
            content_index_enabled,
            content_max_file_bytes,
            media_metadata_scope,
            reason,
        ));
    }
    let failures = read_failures_from_connection(index_dir, &connection)?;
    let index_size_bytes = catalog_dir_size(index_dir).unwrap_or(manifest.index_size_bytes);
    Ok(manifest.to_status(
        root.to_path_buf(),
        index_dir.to_path_buf(),
        failures,
        index_size_bytes,
    ))
}

pub(crate) fn read_failures(index_dir: &Path) -> Result<Vec<FileSearchIndexFailure>, IndexError> {
    let connection = open_catalog_connection(index_dir)?;
    read_failures_from_connection(index_dir, &connection)
}

pub(crate) fn clear_failures(index_dir: &Path) -> Result<(), IndexError> {
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

pub(crate) fn remove_catalog_dir(index_dir: &Path) -> Result<(), IndexError> {
    if !index_dir.exists() {
        return Ok(());
    }
    std_fs::remove_dir_all(index_dir).map_err(|error| search_index_error(index_dir, error))
}

pub(crate) fn catalog_dir_size(index_dir: &Path) -> Result<u64, IndexError> {
    catalog_dir_size_inner(index_dir).map_err(|error| search_index_error(index_dir, error))
}

fn persist_catalog_size(
    index_dir: &Path,
    manifest: &mut SearchIndexManifest,
) -> Result<(), IndexError> {
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

fn open_catalog_connection(index_dir: &Path) -> Result<Connection, IndexError> {
    let catalog_path = catalog_path(index_dir);
    if !catalog_path.is_file() {
        return Err(search_index_error(index_dir, "search catalog is not ready"));
    }
    Connection::open(&catalog_path).map_err(|error| search_index_error(&catalog_path, error))
}

fn read_failures_from_connection(
    index_dir: &Path,
    connection: &Connection,
) -> Result<Vec<FileSearchIndexFailure>, IndexError> {
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

fn saturating_u64_to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}
