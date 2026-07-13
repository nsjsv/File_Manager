use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::error::{SearchError, SearchResult};
use crate::extractor::{DurableContentStageState, ExtractionStatus};
use crate::model::SearchFileKind;

#[path = "database/known_entry.rs"]
mod known_entry;
#[path = "database/scan.rs"]
mod scan;
#[path = "database/search.rs"]
mod search;

#[cfg(test)]
use search::{MAX_SEARCH_SNIPPET_BYTES, MAX_SNIPPET_HITS_PER_BATCH};

pub(crate) use known_entry::{
    DirectorySignature, DirectorySnapshot, EntryObservationState, KnownDirectChild, KnownFileEntry,
};
pub use known_entry::{FileSignature, KnownEntryState};
pub(crate) use scan::{
    validate_classification_batch, FileClassification, ObservedFile,
    MAX_CLASSIFICATION_BATCH_BYTES, MAX_CLASSIFICATION_BATCH_ENTRIES, MAX_KNOWN_ENTRY_PAGE_ENTRIES,
};

/// Bumped whenever the on-disk schema changes in a way that requires a migration.
const SCHEMA_VERSION: i64 = 7;

const WRITER_PAGE_CACHE_KIB: i64 = 2_048;
const READER_PAGE_CACHE_KIB: i64 = 512;
const WAL_AUTOCHECKPOINT_PAGES: i64 = 128;
const WAL_JOURNAL_LIMIT_BYTES: i64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStageProgress {
    Pending,
    Complete,
    Skipped,
}

impl EntryStageProgress {
    fn as_storage_value(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Complete => "complete",
            Self::Skipped => "skipped",
        }
    }

    fn from_storage_value(value: &str) -> SearchResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "complete" => Ok(Self::Complete),
            "skipped" => Ok(Self::Skipped),
            _ => Err(SearchError::InvalidQuery(format!(
                "unsupported entry stage progress: {value}"
            ))),
        }
    }

    fn is_pending(self) -> bool {
        matches!(self, Self::Pending)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedEntryStageState {
    pub metadata: EntryStageProgress,
    pub content: EntryStageProgress,
}

impl IndexedEntryStageState {
    fn allows_signature_skip(&self) -> bool {
        !self.metadata.is_pending() && !self.content.is_pending()
    }

    pub fn pending() -> Self {
        Self {
            metadata: EntryStageProgress::Pending,
            content: EntryStageProgress::Pending,
        }
    }

    fn from_legacy_content_status(extraction_status: &ExtractionStatus) -> Self {
        Self {
            metadata: EntryStageProgress::Complete,
            content: content_stage_progress_for_status(extraction_status),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    pub path: PathBuf,
    pub parent_path: PathBuf,
    pub display_name: String,
    pub kind: SearchFileKind,
    pub size: u64,
    pub modified_ms: Option<i64>,
    pub accessed_ms: Option<i64>,
    pub created_ms: Option<i64>,
    pub mime_type: Option<String>,
    pub stage_state: IndexedEntryStageState,
    pub content: Option<String>,
    pub extraction_status: ExtractionStatus,
    pub device: Option<u64>,
    /// Filesystem inode and nanosecond mtime used for incremental crawls.
    /// Mirrors how tracker-miner-fs keys change detection off inode + mtime.
    pub inode: Option<u64>,
    pub mtime_ns: Option<i64>,
    pub ctime_ns: Option<i64>,
}

#[derive(Debug)]
pub(crate) enum ScanFileMutation {
    Observable(IndexedFile),
    Inaccessible { scope: PathBuf, file: IndexedFile },
}

struct QueryVisibleMetadataRow<'file> {
    path: &'file Path,
    parent_path: &'file Path,
    display_name: &'file str,
    kind: &'file SearchFileKind,
    size: u64,
    modified_ms: Option<i64>,
    accessed_ms: Option<i64>,
    created_ms: Option<i64>,
    mime_type: Option<&'file str>,
    extraction_status: &'file ExtractionStatus,
    signature: FileSignature,
}

struct FulltextContentRow<'file> {
    path: &'file Path,
    display_name: &'file str,
    content: Option<&'file str>,
}

impl IndexedFile {
    fn query_visible_metadata_row(&self) -> QueryVisibleMetadataRow<'_> {
        QueryVisibleMetadataRow {
            path: &self.path,
            parent_path: &self.parent_path,
            display_name: &self.display_name,
            kind: &self.kind,
            size: self.size,
            modified_ms: self.modified_ms,
            accessed_ms: self.accessed_ms,
            created_ms: self.created_ms,
            mime_type: self.mime_type.as_deref(),
            extraction_status: &self.extraction_status,
            signature: FileSignature {
                device: self.device,
                inode: self.inode,
                mtime_ns: self.mtime_ns,
                ctime_ns: self.ctime_ns,
                size: self.size,
            },
        }
    }

    fn fulltext_content_row(&self) -> FulltextContentRow<'_> {
        FulltextContentRow {
            path: &self.path,
            display_name: &self.display_name,
            content: self.content.as_deref(),
        }
    }
}

pub struct SearchDatabase {
    connection: Connection,
    backing_path: Option<PathBuf>,
}

impl SearchDatabase {
    /// Opens the single writable connection. Only the dedicated writer thread
    /// should hold one of these so that all writes are serialized through it,
    /// the way tracker-miner-fs funnels every store operation through one queue.
    pub fn open(path: &Path) -> SearchResult<Self> {
        let connection = Connection::open(path)?;
        configure_writer_connection(&connection)?;
        let database = Self {
            connection,
            backing_path: Some(path.to_path_buf()),
        };
        database.initialize()?;
        Ok(database)
    }

    /// 打开受固定页缓存约束的只读连接；连接数量由协议和 daemon reader owner 限制。
    pub fn open_read_only(path: &Path) -> SearchResult<Self> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure_read_only_connection(&connection)?;
        Ok(Self {
            connection,
            backing_path: Some(path.to_path_buf()),
        })
    }

    pub fn in_memory() -> SearchResult<Self> {
        let connection = Connection::open_in_memory()?;
        configure_writer_connection(&connection)?;
        let database = Self {
            connection,
            backing_path: None,
        };
        database.initialize()?;
        Ok(database)
    }

    pub fn upsert_file(&self, file: &IndexedFile) -> SearchResult<()> {
        let transaction = self.connection.unchecked_transaction()?;
        let metadata_row = file.query_visible_metadata_row();
        let fulltext_row = file.fulltext_content_row();
        upsert_query_visible_metadata(&transaction, &metadata_row)?;
        upsert_stage_state(&transaction, metadata_row.path, &file.stage_state)?;
        replace_fulltext_content(&transaction, &fulltext_row)?;
        transaction.commit()?;
        Ok(())
    }

    fn upsert_stage_state_by_storage_path(
        &self,
        storage_path: &str,
        stage_state: &IndexedEntryStageState,
    ) -> SearchResult<()> {
        self.connection.execute(
            "INSERT INTO file_stage_state (path, metadata_stage_state, content_stage_state)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET
                metadata_stage_state = excluded.metadata_stage_state,
                content_stage_state = excluded.content_stage_state",
            params![
                storage_path,
                stage_state.metadata.as_storage_value(),
                stage_state.content.as_storage_value(),
            ],
        )?;
        Ok(())
    }

    pub fn upsert_inaccessible_file(&self, file: &IndexedFile) -> SearchResult<()> {
        let transaction = self.connection.unchecked_transaction()?;
        let metadata_row = file.query_visible_metadata_row();
        upsert_inaccessible_metadata(&transaction, &metadata_row)?;
        upsert_stage_state(&transaction, metadata_row.path, &file.stage_state)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn apply_file_batch(&self, mutations: &[ScanFileMutation]) -> SearchResult<()> {
        let transaction = self.connection.unchecked_transaction()?;
        for mutation in mutations {
            match mutation {
                ScanFileMutation::Observable(file) => {
                    let metadata_row = file.query_visible_metadata_row();
                    let fulltext_row = file.fulltext_content_row();
                    upsert_query_visible_metadata(&transaction, &metadata_row)?;
                    upsert_stage_state(&transaction, metadata_row.path, &file.stage_state)?;
                    replace_fulltext_content(&transaction, &fulltext_row)?;
                }
                ScanFileMutation::Inaccessible { file, .. } => {
                    let metadata_row = file.query_visible_metadata_row();
                    upsert_inaccessible_metadata(&transaction, &metadata_row)?;
                    upsert_stage_state(&transaction, metadata_row.path, &file.stage_state)?;
                }
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn checkpoint_and_release_idle_cache(&self) -> SearchResult<()> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA shrink_memory;")?;
        let Some(database_path) = self.backing_path.as_ref() else {
            return Ok(());
        };
        release_clean_file_pages(database_path)?;
        let mut wal_path = database_path.as_os_str().to_os_string();
        wal_path.push("-wal");
        let wal_path = PathBuf::from(wal_path);
        if wal_path.exists() {
            release_clean_file_pages(&wal_path)?;
        }
        Ok(())
    }

    pub fn content_status(&self, path: &Path) -> SearchResult<Option<ExtractionStatus>> {
        let status_json: Option<String> = self
            .connection
            .query_row(
                "SELECT content_status FROM files WHERE path = ?1",
                params![path_to_storage(path)],
                |row| row.get(0),
            )
            .optional()?;
        status_json
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .transpose()
    }

    pub fn entry_stage_state(&self, path: &Path) -> SearchResult<Option<IndexedEntryStageState>> {
        let stage_row: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT metadata_stage_state, content_stage_state
                 FROM file_stage_state
                 WHERE path = ?1",
                params![path_to_storage(path)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        stage_row
            .map(|(metadata_stage_state, content_stage_state)| {
                Ok(IndexedEntryStageState {
                    metadata: EntryStageProgress::from_storage_value(&metadata_stage_state)?,
                    content: EntryStageProgress::from_storage_value(&content_stage_state)?,
                })
            })
            .transpose()
    }

    fn initialize(&self) -> SearchResult<()> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS files (
                path TEXT PRIMARY KEY,
                parent_path TEXT NOT NULL,
                display_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                size INTEGER NOT NULL,
                modified_ms INTEGER,
                accessed_ms INTEGER,
                created_ms INTEGER,
                mime_type TEXT,
                content_status TEXT NOT NULL,
                tombstoned INTEGER NOT NULL DEFAULT 0,
                device INTEGER,
                inode INTEGER,
                mtime_ns INTEGER,
                ctime_ns INTEGER,
                observation_state TEXT NOT NULL DEFAULT 'observable'
                    CHECK (observation_state IN ('observable', 'inaccessible')),
                scan_generation INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS file_stage_state (
                path TEXT PRIMARY KEY,
                metadata_stage_state TEXT NOT NULL,
                content_stage_state TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS directory_snapshots (
                path TEXT PRIMARY KEY,
                parent_path TEXT NOT NULL,
                root_path TEXT NOT NULL,
                device INTEGER NOT NULL,
                inode INTEGER NOT NULL,
                mtime_ns INTEGER NOT NULL,
                ctime_ns INTEGER NOT NULL,
                observation_state TEXT NOT NULL DEFAULT 'observable'
                    CHECK (observation_state IN ('observable', 'inaccessible'))
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS file_search_fts
                USING fts5(path UNINDEXED, name, content);",
        )?;
        self.migrate()?;
        self.connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS files_visible_modified_name
                ON files(tombstoned, observation_state, modified_ms DESC, display_name);
             CREATE INDEX IF NOT EXISTS files_parent_visible_modified_name
                ON files(parent_path, tombstoned, observation_state, modified_ms DESC, display_name);
             CREATE INDEX IF NOT EXISTS directory_snapshots_root_path
                ON directory_snapshots(root_path, path);
             CREATE INDEX IF NOT EXISTS directory_snapshots_parent_path
                ON directory_snapshots(parent_path, path);",
        )?;
        self.recover_legacy_tombstones()?;
        Ok(())
    }

    /// Applies forward-only migrations keyed off `PRAGMA user_version`, the same
    /// approach tracker uses to reconcile an old on-disk store with a newer
    /// schema. Adding a column an old database lacks lets existing indexes be
    /// reused instead of rebuilt from scratch on upgrade.
    fn migrate(&self) -> SearchResult<()> {
        let current: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if current < 1 {
            // Databases created before incremental crawling lack the inode and
            // mtime_ns columns. Add them when missing; their NULL values force a
            // one-time re-index of every file, after which crawls go incremental.
            if !self.column_exists("files", "inode")? {
                self.connection
                    .execute_batch("ALTER TABLE files ADD COLUMN inode INTEGER;")?;
            }
            if !self.column_exists("files", "mtime_ns")? {
                self.connection
                    .execute_batch("ALTER TABLE files ADD COLUMN mtime_ns INTEGER;")?;
            }
        }
        if current < 2 {
            self.backfill_stage_state_rows()?;
        }
        if current < 3 && !self.column_exists("files", "observation_state")? {
            self.connection.execute_batch(
                "ALTER TABLE files ADD COLUMN observation_state TEXT NOT NULL DEFAULT 'observable'
                    CHECK (observation_state IN ('observable', 'inaccessible'));",
            )?;
        }
        if current < 4 && !self.column_exists("files", "scan_generation")? {
            self.connection.execute_batch(
                "ALTER TABLE files ADD COLUMN scan_generation INTEGER NOT NULL DEFAULT 0;
                 CREATE TABLE IF NOT EXISTS index_scans (
                    scan_generation INTEGER PRIMARY KEY AUTOINCREMENT,
                    state TEXT NOT NULL CHECK (state IN ('running', 'complete', 'aborted')),
                    started_ms INTEGER NOT NULL,
                    finished_ms INTEGER
                 );
                 CREATE TABLE IF NOT EXISTS scan_scopes (
                    scan_generation INTEGER NOT NULL,
                    scope_path TEXT NOT NULL,
                    scope_kind TEXT NOT NULL CHECK (scope_kind IN ('exact', 'tree')),
                    state TEXT NOT NULL CHECK (
                        state IN ('complete', 'inaccessible', 'missing', 'policy_excluded')
                    ),
                    PRIMARY KEY (scan_generation, scope_path, state)
                 ) WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS files_scan_generation
                    ON files(scan_generation, tombstoned);",
            )?;
        }
        if current < 5 {
            self.migrate_fts_rowids()?;
        }
        if current < 6 && !self.column_exists("files", "ctime_ns")? {
            self.connection
                .execute_batch("ALTER TABLE files ADD COLUMN ctime_ns INTEGER;")?;
        }
        if current < 7 {
            if !self.column_exists("files", "device")? {
                self.connection
                    .execute_batch("ALTER TABLE files ADD COLUMN device INTEGER;")?;
            }
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS directory_snapshots (
                    path TEXT PRIMARY KEY,
                    parent_path TEXT NOT NULL,
                    root_path TEXT NOT NULL,
                    device INTEGER NOT NULL,
                    inode INTEGER NOT NULL,
                    mtime_ns INTEGER NOT NULL,
                    ctime_ns INTEGER NOT NULL,
                    observation_state TEXT NOT NULL DEFAULT 'observable'
                        CHECK (observation_state IN ('observable', 'inaccessible'))
                 );
                 CREATE INDEX IF NOT EXISTS directory_snapshots_root_path
                    ON directory_snapshots(root_path, path);
                 CREATE INDEX IF NOT EXISTS directory_snapshots_parent_path
                    ON directory_snapshots(parent_path, path);
                 DROP INDEX IF EXISTS files_scan_generation;
                 DROP TABLE IF EXISTS scan_scopes;
                 DROP TABLE IF EXISTS index_scans;",
            )?;
        }
        if current < SCHEMA_VERSION {
            self.connection
                .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }

    fn migrate_fts_rowids(&self) -> SearchResult<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute_batch(
            "DROP TABLE IF EXISTS file_search_fts_rowid_migration;
             CREATE VIRTUAL TABLE file_search_fts_rowid_migration
                USING fts5(path UNINDEXED, name, content);
             INSERT INTO file_search_fts_rowid_migration(rowid, path, name, content)
                SELECT f.rowid, old.path, old.name, old.content
                FROM file_search_fts AS old
                JOIN files AS f ON f.path = old.path;
             DROP TABLE file_search_fts;
             ALTER TABLE file_search_fts_rowid_migration RENAME TO file_search_fts;",
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn recover_legacy_tombstones(&self) -> SearchResult<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM file_search_fts
             WHERE path IN (SELECT path FROM files WHERE tombstoned <> 0)",
            [],
        )?;
        transaction.execute(
            "DELETE FROM file_stage_state
             WHERE path IN (SELECT path FROM files WHERE tombstoned <> 0)",
            [],
        )?;
        transaction.execute("DELETE FROM files WHERE tombstoned <> 0", [])?;
        transaction.commit()?;
        Ok(())
    }

    fn backfill_stage_state_rows(&self) -> SearchResult<()> {
        let mut statement = self
            .connection
            .prepare("SELECT path, content_status FROM files")?;
        let rows = statement.query_map([], |row| {
            let path: String = row.get(0)?;
            let content_status: String = row.get(1)?;
            Ok((path, content_status))
        })?;

        for row in rows {
            let (path, content_status) = row?;
            let extraction_status: ExtractionStatus = serde_json::from_str(&content_status)?;
            let stage_state =
                IndexedEntryStageState::from_legacy_content_status(&extraction_status);
            self.upsert_stage_state_by_storage_path(&path, &stage_state)?;
        }

        Ok(())
    }

    fn column_exists(&self, table: &str, column: &str) -> SearchResult<bool> {
        let mut statement = self
            .connection
            .prepare(&format!("PRAGMA table_info({table})"))?;
        let exists = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == column);
        Ok(exists)
    }
}

fn configure_writer_connection(connection: &Connection) -> SearchResult<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "busy_timeout", 5_000)?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "cache_size", -WRITER_PAGE_CACHE_KIB)?;
    connection.pragma_update(None, "mmap_size", 0_i64)?;
    connection.pragma_update(None, "temp_store", "FILE")?;
    connection.pragma_update(None, "wal_autocheckpoint", WAL_AUTOCHECKPOINT_PAGES)?;
    connection.pragma_update(None, "journal_size_limit", WAL_JOURNAL_LIMIT_BYTES)?;
    Ok(())
}

fn release_clean_file_pages(path: &Path) -> SearchResult<()> {
    let file = File::open(path).map_err(|source| SearchError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let advice_status =
        unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
    if advice_status != 0 {
        return Err(SearchError::Io {
            path: path.to_path_buf(),
            source: std::io::Error::from_raw_os_error(advice_status),
        });
    }
    Ok(())
}

fn configure_read_only_connection(connection: &Connection) -> SearchResult<()> {
    connection.pragma_update(None, "busy_timeout", 5_000)?;
    connection.pragma_update(None, "cache_size", -READER_PAGE_CACHE_KIB)?;
    connection.pragma_update(None, "mmap_size", 0_i64)?;
    connection.pragma_update(None, "temp_store", "FILE")?;
    Ok(())
}

fn upsert_query_visible_metadata(
    connection: &Connection,
    metadata_row: &QueryVisibleMetadataRow<'_>,
) -> SearchResult<()> {
    let path = path_to_storage(metadata_row.path);
    let parent_path = path_to_storage(metadata_row.parent_path);
    let content_status = serde_json::to_string(metadata_row.extraction_status)?;
    connection
        .prepare_cached(
            "INSERT INTO files (
            path, parent_path, display_name, kind, size, modified_ms, accessed_ms,
            created_ms, mime_type, content_status, device, inode, mtime_ns, ctime_ns, tombstoned,
            observation_state
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0, 'observable')
         ON CONFLICT(path) DO UPDATE SET
            parent_path = excluded.parent_path,
            display_name = excluded.display_name,
            kind = excluded.kind,
            size = excluded.size,
            modified_ms = excluded.modified_ms,
            accessed_ms = excluded.accessed_ms,
            created_ms = excluded.created_ms,
            mime_type = excluded.mime_type,
            content_status = excluded.content_status,
            device = excluded.device,
            inode = excluded.inode,
            mtime_ns = excluded.mtime_ns,
            ctime_ns = excluded.ctime_ns,
            tombstoned = 0,
            observation_state = 'observable'",
        )?
        .execute(params![
            path,
            parent_path,
            metadata_row.display_name,
            metadata_row.kind.as_storage_value(),
            metadata_row.size as i64,
            metadata_row.modified_ms,
            metadata_row.accessed_ms,
            metadata_row.created_ms,
            metadata_row.mime_type,
            content_status,
            metadata_row.signature.device.map(|device| device as i64),
            metadata_row.signature.inode.map(|inode| inode as i64),
            metadata_row.signature.mtime_ns,
            metadata_row.signature.ctime_ns,
        ])?;
    Ok(())
}

fn upsert_inaccessible_metadata(
    connection: &Connection,
    metadata_row: &QueryVisibleMetadataRow<'_>,
) -> SearchResult<()> {
    let path = path_to_storage(metadata_row.path);
    let parent_path = path_to_storage(metadata_row.parent_path);
    let content_status = serde_json::to_string(metadata_row.extraction_status)?;
    connection
        .prepare_cached(
            "INSERT INTO files (
            path, parent_path, display_name, kind, size, modified_ms, accessed_ms,
            created_ms, mime_type, content_status, device, inode, mtime_ns, ctime_ns, tombstoned,
            observation_state
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0, 'inaccessible')
         ON CONFLICT(path) DO UPDATE SET
            parent_path = excluded.parent_path,
            display_name = excluded.display_name,
            kind = excluded.kind,
            size = excluded.size,
            modified_ms = excluded.modified_ms,
            accessed_ms = excluded.accessed_ms,
            created_ms = excluded.created_ms,
            mime_type = excluded.mime_type,
            device = excluded.device,
            inode = excluded.inode,
            mtime_ns = excluded.mtime_ns,
            ctime_ns = excluded.ctime_ns,
            tombstoned = 0,
            observation_state = 'inaccessible'",
        )?
        .execute(params![
            path,
            parent_path,
            metadata_row.display_name,
            metadata_row.kind.as_storage_value(),
            metadata_row.size as i64,
            metadata_row.modified_ms,
            metadata_row.accessed_ms,
            metadata_row.created_ms,
            metadata_row.mime_type,
            content_status,
            metadata_row.signature.device.map(|device| device as i64),
            metadata_row.signature.inode.map(|inode| inode as i64),
            metadata_row.signature.mtime_ns,
            metadata_row.signature.ctime_ns,
        ])?;
    Ok(())
}

fn upsert_stage_state(
    connection: &Connection,
    path: &Path,
    stage_state: &IndexedEntryStageState,
) -> SearchResult<()> {
    connection
        .prepare_cached(
            "INSERT INTO file_stage_state (path, metadata_stage_state, content_stage_state)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(path) DO UPDATE SET
            metadata_stage_state = excluded.metadata_stage_state,
            content_stage_state = excluded.content_stage_state",
        )?
        .execute(params![
            path_to_storage(path),
            stage_state.metadata.as_storage_value(),
            stage_state.content.as_storage_value(),
        ])?;
    Ok(())
}

fn replace_fulltext_content(
    connection: &Connection,
    fulltext_row: &FulltextContentRow<'_>,
) -> SearchResult<()> {
    let path = path_to_storage(fulltext_row.path);
    let file_rowid: i64 = connection
        .prepare_cached("SELECT rowid FROM files WHERE path = ?1")?
        .query_row(params![path], |row| row.get(0))?;
    connection
        .prepare_cached("DELETE FROM file_search_fts WHERE rowid = ?1")?
        .execute(params![file_rowid])?;
    connection
        .prepare_cached(
            "INSERT INTO file_search_fts (rowid, path, name, content) VALUES (?1, ?2, ?3, ?4)",
        )?
        .execute(params![
            file_rowid,
            path,
            fulltext_row.display_name,
            fulltext_row.content.unwrap_or("")
        ])?;
    Ok(())
}

fn path_to_storage(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn content_stage_progress_for_status(extraction_status: &ExtractionStatus) -> EntryStageProgress {
    match extraction_status.durable_content_stage_state() {
        DurableContentStageState::Complete => EntryStageProgress::Complete,
        DurableContentStageState::Skipped => EntryStageProgress::Skipped,
    }
}

#[cfg(test)]
#[path = "database/tests.rs"]
mod tests;
