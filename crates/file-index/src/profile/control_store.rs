use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use file_core::FileKind;
use rusqlite::{params, Connection, OptionalExtension, Row};

use super::{
    ContentIndexPolicy, IndexProfile, IndexRootSnapshot, IndexTaskPhase, IndexTaskStatus,
    MediaMetadataPolicy, MediaMetadataScope,
};
use crate::search::path_encoding::{path_from_bytes, path_storage_key, path_to_bytes};
use crate::search::{DirectoryErrorPolicy, FileSearchIndexFailure, SearchIndexFileRecord};
use crate::IndexError;

const SCHEMA_VERSION: u32 = 6;
const CONTROL_EXTRACTOR_VERSION: u32 = crate::search::EXTRACTOR_VERSION;
const CONTROL_DB_BUSY_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct ProfileStore {
    pub(super) db_path: PathBuf,
    write_gate: Arc<Mutex<()>>,
}

impl ProfileStore {
    pub fn open(db_path: impl Into<PathBuf>) -> Result<Self, IndexError> {
        let store = Self {
            db_path: db_path.into(),
            write_gate: Arc::new(Mutex::new(())),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn save_profile(&self, profile: &IndexProfile) -> Result<(), IndexError> {
        self.with_write_lock(|| {
            let mut connection = self.connection()?;
            let transaction = connection
                .transaction()
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            transaction
                .execute(
                    "INSERT INTO profiles (
                        id, include_hidden, content_enabled, content_max_file_bytes,
                        media_scope, directory_error_policy
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(id) DO UPDATE SET
                        include_hidden = excluded.include_hidden,
                        content_enabled = excluded.content_enabled,
                        content_max_file_bytes = excluded.content_max_file_bytes,
                        media_scope = excluded.media_scope,
                        directory_error_policy = excluded.directory_error_policy",
                    params![
                        profile.id,
                        profile.include_hidden,
                        profile.content.enabled,
                        saturating_u64_to_i64(profile.content.max_file_bytes),
                        profile.media.scope.config_value(),
                        profile.directory_error_policy.config_value(),
                    ],
                )
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            transaction
                .execute(
                    "DELETE FROM profile_roots WHERE profile_id = ?1",
                    params![profile.id],
                )
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            transaction
                .execute(
                    "DELETE FROM profile_exclude_patterns WHERE profile_id = ?1",
                    params![profile.id],
                )
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            {
                let mut insert_root = transaction
                    .prepare(
                        "INSERT INTO profile_roots (profile_id, ordinal, path_text, path)
                         VALUES (?1, ?2, ?3, ?4)",
                    )
                    .map_err(|error| IndexError::store(&self.db_path, error))?;
                for (ordinal, root) in profile.roots.iter().enumerate() {
                    insert_root
                        .execute(params![
                            profile.id,
                            saturating_usize_to_i64(ordinal),
                            root.to_string_lossy(),
                            path_to_bytes(root),
                        ])
                        .map_err(|error| IndexError::store(&self.db_path, error))?;
                }
            }
            {
                let mut insert_pattern = transaction
                    .prepare(
                        "INSERT INTO profile_exclude_patterns (profile_id, ordinal, pattern)
                         VALUES (?1, ?2, ?3)",
                    )
                    .map_err(|error| IndexError::store(&self.db_path, error))?;
                for (ordinal, pattern) in profile.exclude_patterns.iter().enumerate() {
                    insert_pattern
                        .execute(params![
                            profile.id,
                            saturating_usize_to_i64(ordinal),
                            pattern.as_str()
                        ])
                        .map_err(|error| IndexError::store(&self.db_path, error))?;
                }
            }
            transaction
                .commit()
                .map_err(|error| IndexError::store(&self.db_path, error))
        })
    }

    pub fn load_profiles(&self) -> Result<Vec<IndexProfile>, IndexError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, include_hidden, content_enabled, content_max_file_bytes,
                    media_scope, directory_error_policy
                 FROM profiles
                 ORDER BY id",
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut rows = statement
            .query([])
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut profiles = Vec::new();

        while let Some(row) = rows
            .next()
            .map_err(|error| IndexError::store(&self.db_path, error))?
        {
            let id = row
                .get::<_, String>(0)
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            let include_hidden = row
                .get::<_, bool>(1)
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            let content_enabled = row
                .get::<_, bool>(2)
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            let max_file_bytes = row
                .get::<_, i64>(3)
                .map_err(|error| IndexError::store(&self.db_path, error))
                .and_then(|value| {
                    u64::try_from(value).map_err(|error| IndexError::store(&self.db_path, error))
                })?;
            let media_scope = row
                .get::<_, String>(4)
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            let media_scope =
                MediaMetadataScope::from_config_value(&media_scope).ok_or_else(|| {
                    IndexError::store(
                        &self.db_path,
                        format!("unknown media metadata scope {media_scope}"),
                    )
                })?;
            let directory_error_policy = row
                .get::<_, String>(5)
                .map_err(|error| IndexError::store(&self.db_path, error))
                .and_then(|value| {
                    DirectoryErrorPolicy::from_config_value(&value).ok_or_else(|| {
                        IndexError::store(
                            &self.db_path,
                            format!("unknown directory error policy {value}"),
                        )
                    })
                })?;
            profiles.push(IndexProfile {
                roots: self.load_roots(&id)?,
                exclude_patterns: self.load_exclude_patterns(&id)?,
                id,
                include_hidden,
                directory_error_policy,
                content: ContentIndexPolicy {
                    enabled: content_enabled,
                    max_file_bytes,
                },
                media: MediaMetadataPolicy { scope: media_scope },
            });
        }

        Ok(profiles)
    }

    pub fn delete_profile(&self, id: &str) -> Result<(), IndexError> {
        self.with_write_lock(|| {
            self.connection()?
                .execute("DELETE FROM profiles WHERE id = ?1", params![id])
                .map(|_| ())
                .map_err(|error| IndexError::store(&self.db_path, error))
        })
    }

    pub fn save_task_status(
        &self,
        profile_id: &str,
        root: Option<&Path>,
        phase: IndexTaskPhase,
        message: Option<&str>,
    ) -> Result<(), IndexError> {
        let root_text = root.map(|root| root.to_string_lossy().into_owned());
        let root_path = root.map(path_to_bytes);
        let task_key = task_key(profile_id, root);
        self.with_write_lock(|| {
            self.connection()?
                .execute(
                    "INSERT INTO index_tasks (
                        profile_id, root_text, root_path, task_key, phase, message,
                        updated_at_ms, extractor_version
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(task_key) DO UPDATE SET
                        profile_id = excluded.profile_id,
                        root_text = excluded.root_text,
                        root_path = excluded.root_path,
                        phase = excluded.phase,
                        message = excluded.message,
                        updated_at_ms = excluded.updated_at_ms,
                        extractor_version = excluded.extractor_version",
                    params![
                        profile_id,
                        root_text,
                        root_path,
                        task_key,
                        phase.as_str(),
                        message,
                        current_time_ms(),
                        i64::from(CONTROL_EXTRACTOR_VERSION),
                    ],
                )
                .map(|_| ())
                .map_err(|error| IndexError::store(&self.db_path, error))
        })
    }

    pub fn load_task_statuses(&self) -> Result<Vec<IndexTaskStatus>, IndexError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT profile_id, root_text, root_path, phase, message, updated_at_ms, extractor_version
                 FROM index_tasks
                 ORDER BY profile_id, root_text",
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut rows = statement
            .query([])
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut statuses = Vec::new();

        while let Some(row) = rows
            .next()
            .map_err(|error| IndexError::store(&self.db_path, error))?
        {
            let phase_text = row
                .get::<_, String>(3)
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            let phase = IndexTaskPhase::from_str(&phase_text).ok_or_else(|| {
                IndexError::store(
                    &self.db_path,
                    format!("unknown index task phase {phase_text}"),
                )
            })?;
            let extractor_version = row
                .get::<_, i64>(6)
                .map_err(|error| IndexError::store(&self.db_path, error))
                .and_then(|value| {
                    u32::try_from(value).map_err(|error| IndexError::store(&self.db_path, error))
                })?;
            statuses.push(IndexTaskStatus {
                profile_id: row
                    .get::<_, String>(0)
                    .map_err(|error| IndexError::store(&self.db_path, error))?,
                root: optional_path_from_row(row, 2, 1, &self.db_path)?,
                phase,
                message: row
                    .get::<_, Option<String>>(4)
                    .map_err(|error| IndexError::store(&self.db_path, error))?,
                updated_at_ms: row
                    .get::<_, i64>(5)
                    .map_err(|error| IndexError::store(&self.db_path, error))?,
                extractor_version,
            });
        }

        Ok(statuses)
    }

    pub fn save_root_snapshot(
        &self,
        profile_id: &str,
        root: &Path,
        records: &[SearchIndexFileRecord],
        failures: &[FileSearchIndexFailure],
    ) -> Result<(), IndexError> {
        self.with_write_lock(|| {
            let mut connection = self.connection()?;
            let transaction = connection
                .transaction()
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            let root_text = root.to_string_lossy().into_owned();
            let root_path = path_to_bytes(root);
            transaction
                .execute(
                    "DELETE FROM indexed_files
                     WHERE profile_id = ?1 AND (root_path = ?2 OR root_text = ?3)",
                    params![profile_id, root_path, root_text],
                )
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            transaction
                .execute(
                    "DELETE FROM index_failures
                     WHERE profile_id = ?1 AND (root_path = ?2 OR root_text = ?3)",
                    params![profile_id, root_path, root_text],
                )
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            {
                let mut insert_record = transaction
                    .prepare(
                        "INSERT INTO indexed_files (
                            profile_id, root_text, root_path, path_text, path,
                            relative_path_text, relative_path,
                            kind, mtime_ms, size_bytes, extractor_version
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    )
                    .map_err(|error| IndexError::store(&self.db_path, error))?;
                for record in records {
                    insert_record
                        .execute(params![
                            profile_id,
                            root_text,
                            root_path,
                            record.path.to_string_lossy(),
                            path_to_bytes(&record.path),
                            record.relative_path.to_string_lossy(),
                            path_to_bytes(&record.relative_path),
                            file_kind_key(record.kind),
                            record.mtime_ms,
                            record.size_bytes.map(saturating_u64_to_i64),
                            i64::from(CONTROL_EXTRACTOR_VERSION),
                        ])
                        .map_err(|error| IndexError::store(&self.db_path, error))?;
                }
            }
            {
                let mut insert_failure = transaction
                    .prepare(
                        "INSERT INTO index_failures (
                            profile_id, root_text, root_path, path_text, path, message,
                            first_failed_at_ms, last_failed_at_ms, retry_count
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    )
                    .map_err(|error| IndexError::store(&self.db_path, error))?;
                for failure in failures {
                    insert_failure
                        .execute(params![
                            profile_id,
                            root_text,
                            root_path,
                            failure.path.to_string_lossy(),
                            path_to_bytes(&failure.path),
                            failure.message.as_str(),
                            failure.first_failed_at_ms,
                            failure.last_failed_at_ms,
                            i64::from(failure.retry_count),
                        ])
                        .map_err(|error| IndexError::store(&self.db_path, error))?;
                }
            }
            transaction
                .commit()
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            Ok(())
        })
    }

    pub fn load_root_snapshot(
        &self,
        profile_id: &str,
        root: &Path,
    ) -> Result<IndexRootSnapshot, IndexError> {
        let root_text = root.to_string_lossy().into_owned();
        let root_path = path_to_bytes(root);
        let connection = self.connection()?;
        let mut records_statement = connection
            .prepare(
                "SELECT path_text, path, relative_path_text, relative_path, kind, mtime_ms, size_bytes
                 FROM indexed_files
                 WHERE profile_id = ?1 AND (root_path = ?2 OR root_text = ?3)
                 ORDER BY path_text",
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut rows = records_statement
            .query(params![profile_id, root_path, root_text])
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut records = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| IndexError::store(&self.db_path, error))?
        {
            let kind_text = row
                .get::<_, String>(4)
                .map_err(|error| IndexError::store(&self.db_path, error))?;
            let size_bytes = row
                .get::<_, Option<i64>>(6)
                .map_err(|error| IndexError::store(&self.db_path, error))?
                .and_then(|value| u64::try_from(value).ok());
            records.push(SearchIndexFileRecord {
                path: required_path_from_row(row, 1, 0, &self.db_path)?,
                relative_path: required_path_from_row(row, 3, 2, &self.db_path)?,
                kind: file_kind_from_key(&kind_text),
                mtime_ms: row
                    .get::<_, Option<i64>>(5)
                    .map_err(|error| IndexError::store(&self.db_path, error))?,
                size_bytes,
            });
        }

        let mut failures_statement = connection
            .prepare(
                "SELECT path_text, path, message, first_failed_at_ms, last_failed_at_ms, retry_count
                 FROM index_failures
                 WHERE profile_id = ?1 AND (root_path = ?2 OR root_text = ?3)
                 ORDER BY path_text",
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut rows = failures_statement
            .query(params![profile_id, root_path, root_text])
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut failures = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| IndexError::store(&self.db_path, error))?
        {
            let retry_count = row
                .get::<_, i64>(5)
                .map_err(|error| IndexError::store(&self.db_path, error))
                .and_then(|value| {
                    u32::try_from(value).map_err(|error| IndexError::store(&self.db_path, error))
                })?;
            failures.push(FileSearchIndexFailure {
                path: required_path_from_row(row, 1, 0, &self.db_path)?,
                message: row
                    .get::<_, String>(2)
                    .map_err(|error| IndexError::store(&self.db_path, error))?,
                first_failed_at_ms: row
                    .get::<_, i64>(3)
                    .map_err(|error| IndexError::store(&self.db_path, error))?,
                last_failed_at_ms: row
                    .get::<_, i64>(4)
                    .map_err(|error| IndexError::store(&self.db_path, error))?,
                retry_count,
            });
        }

        Ok(IndexRootSnapshot {
            profile_id: profile_id.to_owned(),
            root: root.to_path_buf(),
            records,
            failures,
        })
    }

    fn initialize(&self) -> Result<(), IndexError> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| IndexError::store(parent, error))?;
        }
        self.remove_unsupported_database()?;
        self.connection()?
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS schema_meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS profiles (
                    id TEXT PRIMARY KEY,
                    include_hidden INTEGER NOT NULL,
                    content_enabled INTEGER NOT NULL,
                    content_max_file_bytes INTEGER NOT NULL,
                    media_scope TEXT NOT NULL DEFAULT 'off',
                    directory_error_policy TEXT NOT NULL DEFAULT 'skip_unreadable'
                 );
                 CREATE TABLE IF NOT EXISTS profile_roots (
                    profile_id TEXT NOT NULL,
                    ordinal INTEGER NOT NULL,
                    path_text TEXT NOT NULL,
                    path BLOB,
                    PRIMARY KEY (profile_id, ordinal),
                    FOREIGN KEY(profile_id) REFERENCES profiles(id) ON DELETE CASCADE
                 );
                 CREATE TABLE IF NOT EXISTS profile_exclude_patterns (
                    profile_id TEXT NOT NULL,
                    ordinal INTEGER NOT NULL,
                    pattern TEXT NOT NULL,
                    PRIMARY KEY (profile_id, ordinal),
                    FOREIGN KEY(profile_id) REFERENCES profiles(id) ON DELETE CASCADE
                 );
                 CREATE TABLE IF NOT EXISTS index_tasks (
                    task_key TEXT PRIMARY KEY,
                    profile_id TEXT NOT NULL,
                    root_text TEXT,
                    root_path BLOB,
                    phase TEXT NOT NULL,
                    message TEXT,
                    updated_at_ms INTEGER NOT NULL,
                    extractor_version INTEGER NOT NULL,
                    FOREIGN KEY(profile_id) REFERENCES profiles(id) ON DELETE CASCADE
                 );
                 CREATE TABLE IF NOT EXISTS indexed_files (
                    profile_id TEXT NOT NULL,
                    root_text TEXT NOT NULL,
                    root_path BLOB,
                    path_text TEXT NOT NULL,
                    path BLOB,
                    relative_path_text TEXT NOT NULL,
                    relative_path BLOB,
                    kind TEXT NOT NULL,
                    mtime_ms INTEGER,
                    size_bytes INTEGER,
                    extractor_version INTEGER NOT NULL,
                    PRIMARY KEY (profile_id, root_text, path_text),
                    FOREIGN KEY(profile_id) REFERENCES profiles(id) ON DELETE CASCADE
                 );
                 CREATE TABLE IF NOT EXISTS index_failures (
                    profile_id TEXT NOT NULL,
                    root_text TEXT NOT NULL,
                    root_path BLOB,
                    path_text TEXT NOT NULL,
                    path BLOB,
                    message TEXT NOT NULL,
                    first_failed_at_ms INTEGER NOT NULL,
                    last_failed_at_ms INTEGER NOT NULL,
                    retry_count INTEGER NOT NULL,
                    PRIMARY KEY (profile_id, root_text, path_text),
                    FOREIGN KEY(profile_id) REFERENCES profiles(id) ON DELETE CASCADE
                 );",
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        self.write_schema_version()?;
        self.validate_schema_version()
    }

    fn remove_unsupported_database(&self) -> Result<(), IndexError> {
        if !self.db_path.is_file() {
            return Ok(());
        }
        let connection = Connection::open(&self.db_path)
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let version = connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten();
        drop(connection);

        if version.as_deref() == Some(&SCHEMA_VERSION.to_string()) {
            return Ok(());
        }
        std::fs::remove_file(&self.db_path).map_err(|error| IndexError::store(&self.db_path, error))
    }

    fn write_schema_version(&self) -> Result<(), IndexError> {
        let connection = self.connection()?;
        let version = connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok();
        match version.as_deref() {
            None => {
                connection
                    .execute(
                        "INSERT INTO schema_meta (key, value) VALUES ('schema_version', ?1)",
                        params![SCHEMA_VERSION.to_string()],
                    )
                    .map_err(|error| IndexError::store(&self.db_path, error))?;
            }
            Some(version) if version == SCHEMA_VERSION.to_string() => {}
            Some(version) => {
                return Err(IndexError::store(
                    &self.db_path,
                    format!("unsupported index control schema version: {version}"),
                ));
            }
        }
        connection
            .execute(
                "INSERT INTO schema_meta (key, value)
                    VALUES ('extractor_version', ?1)
                    ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![CONTROL_EXTRACTOR_VERSION.to_string()],
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        Ok(())
    }

    fn validate_schema_version(&self) -> Result<(), IndexError> {
        let version = self
            .connection()?
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        if version == SCHEMA_VERSION.to_string() {
            Ok(())
        } else {
            Err(IndexError::store(
                &self.db_path,
                format!("unsupported index control schema version: {version}"),
            ))
        }
    }

    fn load_roots(&self, id: &str) -> Result<Vec<PathBuf>, IndexError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT path_text, path FROM profile_roots WHERE profile_id = ?1 ORDER BY ordinal",
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut rows = statement
            .query(params![id])
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut roots = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| IndexError::store(&self.db_path, error))?
        {
            roots.push(required_path_from_row(row, 1, 0, &self.db_path)?);
        }
        Ok(roots)
    }

    fn load_exclude_patterns(&self, id: &str) -> Result<Vec<String>, IndexError> {
        self.load_ordered_texts("profile_exclude_patterns", "pattern", id)
    }

    fn load_ordered_texts(
        &self,
        table: &str,
        column: &str,
        id: &str,
    ) -> Result<Vec<String>, IndexError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(&format!(
                "SELECT {column} FROM {table} WHERE profile_id = ?1 ORDER BY ordinal"
            ))
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut rows = statement
            .query(params![id])
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let mut values = Vec::new();

        while let Some(row) = rows
            .next()
            .map_err(|error| IndexError::store(&self.db_path, error))?
        {
            values.push(
                row.get::<_, String>(0)
                    .map_err(|error| IndexError::store(&self.db_path, error))?,
            );
        }
        Ok(values)
    }

    fn with_write_lock<T>(
        &self,
        write: impl FnOnce() -> Result<T, IndexError>,
    ) -> Result<T, IndexError> {
        let _guard = self
            .write_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        write()
    }

    fn connection(&self) -> Result<Connection, IndexError> {
        let connection = Connection::open(&self.db_path)
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        connection
            .busy_timeout(CONTROL_DB_BUSY_TIMEOUT)
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        Ok(connection)
    }
}

pub(super) fn saturating_u64_to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn saturating_usize_to_i64(value: usize) -> i64 {
    value.min(i64::MAX as usize) as i64
}

fn task_key(profile_id: &str, root: Option<&Path>) -> String {
    match root {
        Some(root) => format!("{profile_id}:{}", path_storage_key(root)),
        None => format!("{profile_id}:profile"),
    }
}

fn required_path_from_row(
    row: &Row<'_>,
    bytes_column: usize,
    text_column: usize,
    db_path: &Path,
) -> Result<PathBuf, IndexError> {
    optional_path_from_row(row, bytes_column, text_column, db_path)?.ok_or_else(|| {
        IndexError::store(
            db_path,
            format!("missing path column {bytes_column} or {text_column}"),
        )
    })
}

fn optional_path_from_row(
    row: &Row<'_>,
    bytes_column: usize,
    text_column: usize,
    db_path: &Path,
) -> Result<Option<PathBuf>, IndexError> {
    let path_bytes = row
        .get::<_, Option<Vec<u8>>>(bytes_column)
        .map_err(|error| IndexError::store(db_path, error))?;
    if let Some(path_bytes) = path_bytes {
        return Ok(Some(path_from_bytes(path_bytes)));
    }
    row.get::<_, Option<String>>(text_column)
        .map(|path| path.map(PathBuf::from))
        .map_err(|error| IndexError::store(db_path, error))
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

pub(super) fn file_kind_key(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Directory => "directory",
        FileKind::File => "file",
        FileKind::Symlink => "symlink",
        FileKind::Other => "other",
    }
}

fn file_kind_from_key(value: &str) -> FileKind {
    match value {
        "directory" => FileKind::Directory,
        "file" => FileKind::File,
        "symlink" => FileKind::Symlink,
        "other" => FileKind::Other,
        _ => FileKind::Other,
    }
}
