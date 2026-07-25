use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension};

use crate::error::{SearchError, SearchResult};
use crate::model::SearchFileKind;

use super::{
    path_from_storage_bytes, path_to_storage, recursive_storage_range, DirectorySignature,
    DirectorySnapshot, EntryObservationState, EntryStageProgress, FileSignature,
    IndexedEntryStageState, KnownDirectChild, KnownEntryState, KnownFileEntry, SearchDatabase,
};

/// 单批上限同时约束 SQL 参数、writer 交接和返回状态，容量与索引规模无关。
pub(crate) const MAX_CLASSIFICATION_BATCH_ENTRIES: usize = 128;
pub(crate) const MAX_CLASSIFICATION_BATCH_BYTES: usize = 262_144;
pub(crate) const MAX_KNOWN_ENTRY_PAGE_ENTRIES: usize = 128;
const CLASSIFICATION_FIXED_BYTES_PER_ENTRY: usize = 96;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedFile {
    pub path: PathBuf,
    pub signature: FileSignature,
}

impl ObservedFile {
    pub(crate) fn estimated_bytes(&self) -> usize {
        self.path
            .as_os_str()
            .as_encoded_bytes()
            .len()
            .saturating_add(CLASSIFICATION_FIXED_BYTES_PER_ENTRY)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileClassification {
    pub known_entry: Option<KnownEntryState>,
}

impl SearchDatabase {
    /// 只读取持久签名；未变化分类不能顺带写入“本轮已见”状态。
    pub(crate) fn classify_observed_files(
        &self,
        observed_files: &[ObservedFile],
    ) -> SearchResult<Vec<FileClassification>> {
        validate_classification_batch(observed_files)?;
        let mut statement = self.connection.prepare_cached(
            "SELECT
                f.device,
                f.inode,
                f.mtime_ns,
                f.ctime_ns,
                f.size,
                s.metadata_stage_state,
                s.content_stage_state,
                f.observation_state
             FROM files AS f
             LEFT JOIN file_stage_state AS s ON s.path = f.path
             WHERE f.path = ?1 AND f.tombstoned = 0",
        )?;

        observed_files
            .iter()
            .map(|observed_file| {
                let storage_path = path_to_storage(&observed_file.path);
                Ok(FileClassification {
                    known_entry: read_known_entry(&mut statement, &storage_path)?,
                })
            })
            .collect()
    }

    pub(crate) fn known_files_page(
        &self,
        scope: &Path,
        after_path: Option<&Path>,
        limit: usize,
    ) -> SearchResult<Vec<KnownFileEntry>> {
        validate_page_limit(limit)?;
        let scope_range = recursive_storage_range(scope);
        let after_path = after_path.map(path_to_storage).unwrap_or_default();
        let mut statement = self.connection.prepare_cached(
            "SELECT
                f.path,
                f.device,
                f.inode,
                f.mtime_ns,
                f.ctime_ns,
                f.size,
                s.metadata_stage_state,
                s.content_stage_state,
                f.observation_state
             FROM files AS f
             LEFT JOIN file_stage_state AS s ON s.path = f.path
             WHERE f.tombstoned = 0
               AND f.path > ?4
               AND NOT EXISTS (
                    SELECT 1
                    FROM directory_snapshots AS parent_directory
                    WHERE parent_directory.path = f.parent_path
                      AND parent_directory.observation_state = 'inaccessible'
               )
               AND (
                    f.path = ?1
                    OR (f.path >= ?2 AND f.path < ?3)
               )
             ORDER BY f.path
             LIMIT ?5",
        )?;
        let rows = statement.query_map(
            params![
                scope_range.exact_path,
                scope_range.descendant_lower,
                scope_range.descendant_upper,
                after_path,
                limit as i64,
            ],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )?;

        rows.map(|row| {
            let (
                path,
                device,
                inode,
                mtime_ns,
                ctime_ns,
                size,
                metadata_stage_state,
                content_stage_state,
                observation_state,
            ) = row?;
            Ok(KnownFileEntry {
                path: path_from_storage_bytes(path),
                state: known_entry_state(
                    device,
                    inode,
                    mtime_ns,
                    ctime_ns,
                    size,
                    metadata_stage_state.as_deref(),
                    content_stage_state.as_deref(),
                    &observation_state,
                )?,
            })
        })
        .collect()
    }

    pub(crate) fn directory_snapshots_page(
        &self,
        scope: &Path,
        after_path: Option<&Path>,
        limit: usize,
    ) -> SearchResult<Vec<DirectorySnapshot>> {
        validate_page_limit(limit)?;
        let scope_range = recursive_storage_range(scope);
        let after_path = after_path.map(path_to_storage).unwrap_or_default();
        let mut statement = self.connection.prepare_cached(
            "SELECT path, parent_path, root_path, device, inode, mtime_ns, ctime_ns,
                    observation_state
             FROM directory_snapshots
             WHERE path > ?4
               AND (
                    path = ?1
                    OR (path >= ?2 AND path < ?3)
               )
             ORDER BY path
             LIMIT ?5",
        )?;
        let rows = statement.query_map(
            params![
                scope_range.exact_path,
                scope_range.descendant_lower,
                scope_range.descendant_upper,
                after_path,
                limit as i64,
            ],
            read_directory_snapshot_row,
        )?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub(crate) fn directory_snapshot_paths_page(
        &self,
        after_path: Option<&Path>,
        limit: usize,
    ) -> SearchResult<Vec<PathBuf>> {
        validate_page_limit(limit)?;
        let after_path = after_path.map(path_to_storage).unwrap_or_default();
        let mut statement = self.connection.prepare_cached(
            "SELECT path
             FROM directory_snapshots
             WHERE path > ?1 AND observation_state = 'observable'
             ORDER BY path
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![after_path, limit as i64], |row| {
            row.get::<_, Vec<u8>>(0).map(path_from_storage_bytes)
        })?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub(crate) fn directory_snapshot(
        &self,
        path: &Path,
    ) -> SearchResult<Option<DirectorySnapshot>> {
        self.connection
            .query_row(
                "SELECT path, parent_path, root_path, device, inode, mtime_ns, ctime_ns,
                        observation_state
                 FROM directory_snapshots
                 WHERE path = ?1",
                params![path_to_storage(path)],
                read_directory_snapshot_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn upsert_directory_snapshot(
        &self,
        snapshot: &DirectorySnapshot,
    ) -> SearchResult<()> {
        self.connection.execute(
            "INSERT INTO directory_snapshots (
                path, parent_path, root_path, device, inode, mtime_ns, ctime_ns,
                observation_state
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(path) DO UPDATE SET
                parent_path = excluded.parent_path,
                root_path = excluded.root_path,
                device = excluded.device,
                inode = excluded.inode,
                mtime_ns = excluded.mtime_ns,
                ctime_ns = excluded.ctime_ns,
                observation_state = excluded.observation_state",
            params![
                path_to_storage(&snapshot.path),
                path_to_storage(&snapshot.parent_path),
                path_to_storage(&snapshot.root_path),
                snapshot.signature.device as i64,
                snapshot.signature.inode as i64,
                snapshot.signature.mtime_ns,
                snapshot.signature.ctime_ns,
                snapshot.observation_state.as_storage_value(),
            ],
        )?;
        Ok(())
    }

    pub(crate) fn direct_children_page(
        &self,
        parent: &Path,
        after_path: Option<&Path>,
        limit: usize,
    ) -> SearchResult<Vec<KnownDirectChild>> {
        validate_page_limit(limit)?;
        let parent = path_to_storage(parent);
        let after_path = after_path.map(path_to_storage).unwrap_or_default();
        let mut statement = self.connection.prepare_cached(
            "SELECT path, entry_kind
             FROM (
                SELECT path, 'file' AS entry_kind
                FROM files
                WHERE parent_path = ?1 AND tombstoned = 0
                UNION ALL
                SELECT path, 'directory' AS entry_kind
                FROM directory_snapshots
                WHERE parent_path = ?1
             )
             WHERE path > ?2
             ORDER BY path
             LIMIT ?3",
        )?;
        let rows = statement.query_map(params![parent, after_path, limit as i64], |row| {
            let path = path_from_storage_bytes(row.get::<_, Vec<u8>>(0)?);
            let kind = SearchFileKind::from_storage_value(&row.get::<_, String>(1)?);
            Ok(KnownDirectChild { path, kind })
        })?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub(crate) fn mark_scope_inaccessible(&self, scope: &Path) -> SearchResult<bool> {
        let transaction = self.connection.unchecked_transaction()?;
        let scope_range = recursive_storage_range(scope);
        let files_changed = transaction.execute(
            "UPDATE files
             SET observation_state = 'inaccessible'
             WHERE tombstoned = 0
               AND observation_state <> 'inaccessible'
               AND (
                    path = ?1
                    OR (path >= ?2 AND path < ?3)
               )",
            params![
                &scope_range.exact_path,
                &scope_range.descendant_lower,
                &scope_range.descendant_upper,
            ],
        )?;
        let directories_changed = transaction.execute(
            "UPDATE directory_snapshots
             SET observation_state = 'inaccessible'
             WHERE observation_state <> 'inaccessible'
               AND (
                    path = ?1
                    OR (path >= ?2 AND path < ?3)
               )",
            params![
                &scope_range.exact_path,
                &scope_range.descendant_lower,
                &scope_range.descendant_upper,
            ],
        )?;
        transaction.commit()?;
        Ok(files_changed > 0 || directories_changed > 0)
    }

    pub(crate) fn delete_scope(&self, scope: &Path) -> SearchResult<()> {
        let transaction = self.connection.unchecked_transaction()?;
        let scope_range = recursive_storage_range(scope);
        transaction.execute(
            "DELETE FROM file_search_fts
             WHERE rowid IN (
                SELECT rowid FROM files
                WHERE path = ?1 OR (path >= ?2 AND path < ?3)
             )",
            params![
                &scope_range.exact_path,
                &scope_range.descendant_lower,
                &scope_range.descendant_upper,
            ],
        )?;
        transaction.execute(
            "DELETE FROM file_stage_state
             WHERE path IN (
                SELECT path FROM files
                WHERE path = ?1 OR (path >= ?2 AND path < ?3)
             )",
            params![
                &scope_range.exact_path,
                &scope_range.descendant_lower,
                &scope_range.descendant_upper,
            ],
        )?;
        transaction.execute(
            "DELETE FROM files
             WHERE path = ?1 OR (path >= ?2 AND path < ?3)",
            params![
                &scope_range.exact_path,
                &scope_range.descendant_lower,
                &scope_range.descendant_upper,
            ],
        )?;
        transaction.execute(
            "DELETE FROM directory_snapshots
             WHERE path = ?1 OR (path >= ?2 AND path < ?3)",
            params![
                &scope_range.exact_path,
                &scope_range.descendant_lower,
                &scope_range.descendant_upper,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn directory_snapshot_count(&self) -> SearchResult<u64> {
        self.connection
            .query_row("SELECT COUNT(*) FROM directory_snapshots", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| count.max(0) as u64)
            .map_err(Into::into)
    }
}

pub(crate) fn validate_classification_batch(observed_files: &[ObservedFile]) -> SearchResult<()> {
    let estimated_bytes = observed_files.iter().fold(0_usize, |total, observed_file| {
        total.saturating_add(observed_file.estimated_bytes())
    });
    if observed_files.len() > MAX_CLASSIFICATION_BATCH_ENTRIES
        || estimated_bytes > MAX_CLASSIFICATION_BATCH_BYTES
    {
        return Err(SearchError::InvalidQuery(format!(
            "scan classification batch exceeds capacity: {} entries, {} bytes",
            observed_files.len(),
            estimated_bytes
        )));
    }
    Ok(())
}

fn validate_page_limit(limit: usize) -> SearchResult<()> {
    if limit == 0 || limit > MAX_KNOWN_ENTRY_PAGE_ENTRIES {
        return Err(SearchError::InvalidQuery(format!(
            "known entry page size {limit} is outside 1..={MAX_KNOWN_ENTRY_PAGE_ENTRIES}"
        )));
    }
    Ok(())
}

fn read_known_entry(
    statement: &mut rusqlite::Statement<'_>,
    storage_path: &[u8],
) -> SearchResult<Option<KnownEntryState>> {
    let stored_row = statement
        .query_row(params![storage_path], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
            ))
        })
        .optional()?;

    stored_row
        .map(
            |(
                device,
                inode,
                mtime_ns,
                ctime_ns,
                size,
                metadata_stage_state,
                content_stage_state,
                observation_state,
            )| {
                known_entry_state(
                    device,
                    inode,
                    mtime_ns,
                    ctime_ns,
                    size,
                    metadata_stage_state.as_deref(),
                    content_stage_state.as_deref(),
                    &observation_state,
                )
            },
        )
        .transpose()
}

fn known_entry_state(
    device: Option<i64>,
    inode: Option<i64>,
    mtime_ns: Option<i64>,
    ctime_ns: Option<i64>,
    size: i64,
    metadata_stage_state: Option<&str>,
    content_stage_state: Option<&str>,
    observation_state: &str,
) -> SearchResult<KnownEntryState> {
    let stage_state = match (metadata_stage_state, content_stage_state) {
        (Some(metadata), Some(content)) => IndexedEntryStageState {
            metadata: EntryStageProgress::from_storage_value(metadata)?,
            content: EntryStageProgress::from_storage_value(content)?,
        },
        _ => IndexedEntryStageState::pending(),
    };
    Ok(KnownEntryState {
        signature: FileSignature {
            device: device.map(|value| value as u64),
            inode: inode.map(|value| value as u64),
            mtime_ns,
            ctime_ns,
            size: size.max(0) as u64,
        },
        stage_state,
        observation_state: EntryObservationState::from_storage_value(observation_state)?,
    })
}

fn read_directory_snapshot_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DirectorySnapshot> {
    let observation_state = row.get::<_, String>(7)?;
    let observation_state = match observation_state.as_str() {
        "observable" => EntryObservationState::Observable,
        "inaccessible" => EntryObservationState::Inaccessible,
        _ => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unsupported directory observation state: {observation_state}"),
                )),
            ))
        }
    };
    Ok(DirectorySnapshot {
        path: path_from_storage_bytes(row.get::<_, Vec<u8>>(0)?),
        parent_path: path_from_storage_bytes(row.get::<_, Vec<u8>>(1)?),
        root_path: path_from_storage_bytes(row.get::<_, Vec<u8>>(2)?),
        signature: DirectorySignature {
            device: row.get::<_, i64>(3)? as u64,
            inode: row.get::<_, i64>(4)? as u64,
            mtime_ns: row.get(5)?,
            ctime_ns: row.get(6)?,
        },
        observation_state,
    })
}
