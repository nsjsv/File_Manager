use std::path::Path;

use rusqlite::{params, Connection};

use super::control_store::{file_kind_key, saturating_u64_to_i64, ProfileStore};
use crate::search::path_encoding::path_to_bytes;
use crate::search::{FileSearchIndexFailure, SearchIndexFileRecord};
use crate::IndexError;

pub(crate) struct RootSnapshotWriteSession {
    db_path: std::path::PathBuf,
    connection: Connection,
    profile_id: String,
    root_text: String,
    root_path: Vec<u8>,
}

impl ProfileStore {
    pub(crate) fn begin_root_snapshot(
        &self,
        profile_id: &str,
        root: &Path,
    ) -> Result<RootSnapshotWriteSession, IndexError> {
        let connection = Connection::open(&self.db_path)
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        let root_text = root.to_string_lossy().into_owned();
        let root_path = path_to_bytes(root);
        connection
            .execute_batch("BEGIN IMMEDIATE;")
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        connection
            .execute(
                "DELETE FROM indexed_files
                 WHERE profile_id = ?1 AND (root_path = ?2 OR root_text = ?3)",
                params![profile_id, root_path.as_slice(), root_text.as_str()],
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?;
        connection
            .execute(
                "DELETE FROM index_failures
                 WHERE profile_id = ?1 AND (root_path = ?2 OR root_text = ?3)",
                params![profile_id, root_path.as_slice(), root_text.as_str()],
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?;

        Ok(RootSnapshotWriteSession {
            db_path: self.db_path.clone(),
            connection,
            profile_id: profile_id.to_owned(),
            root_text,
            root_path,
        })
    }
}

impl RootSnapshotWriteSession {
    pub(crate) fn add_record(&mut self, record: &SearchIndexFileRecord) -> Result<(), IndexError> {
        self.connection
            .prepare_cached(
                "INSERT INTO indexed_files (
                    profile_id, root_text, root_path, path_text, path,
                    relative_path_text, relative_path,
                    kind, mtime_ms, size_bytes, extractor_version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?
            .execute(params![
                self.profile_id.as_str(),
                self.root_text.as_str(),
                self.root_path.as_slice(),
                record.path.to_string_lossy(),
                path_to_bytes(&record.path),
                record.relative_path.to_string_lossy(),
                path_to_bytes(&record.relative_path),
                file_kind_key(record.kind),
                record.mtime_ms,
                record.size_bytes.map(saturating_u64_to_i64),
                i64::from(crate::search::EXTRACTOR_VERSION),
            ])
            .map(|_| ())
            .map_err(|error| IndexError::store(&self.db_path, error))
    }

    pub(crate) fn add_failure(
        &mut self,
        failure: &FileSearchIndexFailure,
    ) -> Result<(), IndexError> {
        self.connection
            .prepare_cached(
                "INSERT INTO index_failures (
                    profile_id, root_text, root_path, path_text, path, message,
                    first_failed_at_ms, last_failed_at_ms, retry_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .map_err(|error| IndexError::store(&self.db_path, error))?
            .execute(params![
                self.profile_id.as_str(),
                self.root_text.as_str(),
                self.root_path.as_slice(),
                failure.path.to_string_lossy(),
                path_to_bytes(&failure.path),
                failure.message.as_str(),
                failure.first_failed_at_ms,
                failure.last_failed_at_ms,
                i64::from(failure.retry_count),
            ])
            .map(|_| ())
            .map_err(|error| IndexError::store(&self.db_path, error))
    }

    pub(crate) fn finish(self) -> Result<(), IndexError> {
        self.connection
            .execute_batch("COMMIT;")
            .map_err(|error| IndexError::store(&self.db_path, error))
    }
}
