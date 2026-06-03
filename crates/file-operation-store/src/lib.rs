use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS task_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL,
    progress_fraction REAL,
    error TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
"#;

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    InvalidStatus(String),
    InvalidTaskId(i64),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Sqlite(error) => write!(formatter, "SQLite error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::InvalidStatus(status) => write!(formatter, "invalid task status: {status}"),
            Self::InvalidTaskId(id) => write!(formatter, "invalid SQLite task id: {id}"),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::InvalidStatus(_) | Self::InvalidTaskId(_) => None,
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "encoding", rename_all = "snake_case")]
pub enum StoredPath {
    #[cfg(unix)]
    UnixBytes {
        bytes: Vec<u8>,
    },
    #[cfg(windows)]
    WindowsWide {
        units: Vec<u16>,
    },
    Utf8 {
        value: String,
    },
}

impl StoredPath {
    pub fn from_path(path: &Path) -> Self {
        #[cfg(unix)]
        {
            return Self::UnixBytes {
                bytes: path.as_os_str().as_bytes().to_vec(),
            };
        }

        #[cfg(windows)]
        {
            return Self::WindowsWide {
                units: path.as_os_str().encode_wide().collect(),
            };
        }

        #[cfg(not(any(unix, windows)))]
        {
            Self::Utf8 {
                value: path.to_string_lossy().into_owned(),
            }
        }
    }

    pub fn to_path_buf(&self) -> PathBuf {
        match self {
            #[cfg(unix)]
            Self::UnixBytes { bytes } => PathBuf::from(OsString::from_vec(bytes.clone())),
            #[cfg(windows)]
            Self::WindowsWide { units } => PathBuf::from(OsString::from_wide(units)),
            Self::Utf8 { value } => PathBuf::from(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTransfer {
    pub source: StoredPath,
    pub target: StoredPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTrashEntry {
    pub trash_path: StoredPath,
    pub info_path: StoredPath,
    pub original_path: StoredPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoredOperation {
    Rename { path: StoredPath, new_name: String },
    CreateDirectory { parent: StoredPath },
    CreateEmptyFile { parent: StoredPath },
    Trash { paths: Vec<StoredPath> },
    Restore { entries: Vec<StoredTrashEntry> },
    DeleteTrashEntries { entries: Vec<StoredTrashEntry> },
    EmptyTrash,
    Copy { transfers: Vec<StoredTransfer> },
    Move { transfers: Vec<StoredTransfer> },
}

impl StoredOperation {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Rename { .. } => "rename",
            Self::CreateDirectory { .. } => "create_directory",
            Self::CreateEmptyFile { .. } => "create_empty_file",
            Self::Trash { .. } => "trash",
            Self::Restore { .. } => "restore",
            Self::DeleteTrashEntries { .. } => "delete_trash_entries",
            Self::EmptyTrash => "empty_trash",
            Self::Copy { .. } => "copy",
            Self::Move { .. } => "move",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredTaskStatus {
    Pending,
    Running,
    Paused,
    Canceling,
    Failed,
    Completed,
    Canceled,
}

impl StoredTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Canceling => "canceling",
            Self::Failed => "failed",
            Self::Completed => "completed",
            Self::Canceled => "canceled",
        }
    }

    fn parse(status: String) -> StoreResult<Self> {
        match status.as_str() {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "canceling" => Ok(Self::Canceling),
            "failed" => Ok(Self::Failed),
            "completed" => Ok(Self::Completed),
            "canceled" => Ok(Self::Canceled),
            _ => Err(StoreError::InvalidStatus(status)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StoredProgress {
    pub fraction: Option<f64>,
}

impl StoredProgress {
    pub fn pending() -> Self {
        Self { fraction: None }
    }

    pub fn with_fraction(fraction: f64) -> Self {
        Self {
            fraction: Some(fraction.clamp(0.0, 1.0)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredTask {
    pub id: u64,
    pub operation: StoredOperation,
    pub status: StoredTaskStatus,
    pub progress: StoredProgress,
    pub error: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct TaskQueueStore {
    db_path: PathBuf,
}

impl TaskQueueStore {
    pub fn new(db_path: impl Into<PathBuf>) -> StoreResult<Self> {
        let store = Self {
            db_path: db_path.into(),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn initialize(&self) -> StoreResult<()> {
        if let Some(parent) = self
            .db_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        self.connection()?.execute_batch(SCHEMA_SQL)?;
        Ok(())
    }

    pub fn insert_task(&self, operation: &StoredOperation) -> StoreResult<u64> {
        let connection = self.connection()?;
        let now = current_time_ms();
        let payload_json = serde_json::to_string(operation)?;
        connection.execute(
            "INSERT INTO task_queue (
                operation_kind, payload_json, status, progress_fraction, error,
                created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?4)",
            params![
                operation.kind(),
                payload_json,
                StoredTaskStatus::Pending.as_str(),
                now
            ],
        )?;
        sqlite_id_to_u64(connection.last_insert_rowid())
    }

    pub fn read_task(&self, id: u64) -> StoreResult<Option<StoredTask>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, payload_json, status, progress_fraction, error, created_at_ms, updated_at_ms
             FROM task_queue
             WHERE id = ?1",
        )?;
        let row = statement
            .query_row(params![id], task_row_from_sql)
            .optional()?;
        row.map(StoredTask::try_from).transpose()
    }

    pub fn read_tasks(&self) -> StoreResult<Vec<StoredTask>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, payload_json, status, progress_fraction, error, created_at_ms, updated_at_ms
             FROM task_queue
             ORDER BY id ASC",
        )?;
        let rows = statement.query_map([], task_row_from_sql)?;
        rows.map(|row| row.map_err(StoreError::from).and_then(StoredTask::try_from))
            .collect()
    }

    pub fn update_status(&self, id: u64, status: StoredTaskStatus) -> StoreResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE task_queue SET status = ?1, updated_at_ms = ?2 WHERE id = ?3",
            params![status.as_str(), current_time_ms(), id],
        )?;
        Ok(())
    }

    pub fn update_progress(&self, id: u64, progress: StoredProgress) -> StoreResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE task_queue SET progress_fraction = ?1, updated_at_ms = ?2 WHERE id = ?3",
            params![progress.fraction, current_time_ms(), id],
        )?;
        Ok(())
    }

    pub fn update_error(&self, id: u64, error: Option<&str>) -> StoreResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE task_queue SET error = ?1, updated_at_ms = ?2 WHERE id = ?3",
            params![error, current_time_ms(), id],
        )?;
        Ok(())
    }

    pub fn update_task_state(
        &self,
        id: u64,
        status: StoredTaskStatus,
        progress: StoredProgress,
        error: Option<&str>,
    ) -> StoreResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE task_queue
             SET status = ?1, progress_fraction = ?2, error = ?3, updated_at_ms = ?4
             WHERE id = ?5",
            params![
                status.as_str(),
                progress.fraction,
                error,
                current_time_ms(),
                id
            ],
        )?;
        Ok(())
    }

    pub fn delete_task(&self, id: u64) -> StoreResult<()> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM task_queue WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear_tasks(&self) -> StoreResult<()> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM task_queue", [])?;
        Ok(())
    }

    pub fn mark_unfinished_tasks_failed(&self, error: &str) -> StoreResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE task_queue
             SET status = ?1, error = ?2, updated_at_ms = ?3
             WHERE status IN (?4, ?5, ?6, ?7)",
            params![
                StoredTaskStatus::Failed.as_str(),
                error,
                current_time_ms(),
                StoredTaskStatus::Pending.as_str(),
                StoredTaskStatus::Running.as_str(),
                StoredTaskStatus::Paused.as_str(),
                StoredTaskStatus::Canceling.as_str(),
            ],
        )?;
        Ok(())
    }

    fn connection(&self) -> StoreResult<Connection> {
        Ok(Connection::open(&self.db_path)?)
    }
}

struct TaskRow {
    id: i64,
    payload_json: String,
    status: String,
    progress_fraction: Option<f64>,
    error: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

impl TryFrom<TaskRow> for StoredTask {
    type Error = StoreError;

    fn try_from(row: TaskRow) -> StoreResult<Self> {
        Ok(Self {
            id: sqlite_id_to_u64(row.id)?,
            operation: serde_json::from_str(&row.payload_json)?,
            status: StoredTaskStatus::parse(row.status)?,
            progress: StoredProgress {
                fraction: row.progress_fraction,
            },
            error: row.error,
            created_at_ms: row.created_at_ms,
            updated_at_ms: row.updated_at_ms,
        })
    }
}

fn task_row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRow> {
    Ok(TaskRow {
        id: row.get(0)?,
        payload_json: row.get(1)?,
        status: row.get(2)?,
        progress_fraction: row.get(3)?,
        error: row.get(4)?,
        created_at_ms: row.get(5)?,
        updated_at_ms: row.get(6)?,
    })
}

fn sqlite_id_to_u64(id: i64) -> StoreResult<u64> {
    u64::try_from(id).map_err(|_| StoreError::InvalidTaskId(id))
}

fn current_time_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(unix)]
    use std::os::unix::ffi::OsStrExt;

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn test_store() -> (TaskQueueStore, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "file-operation-store-test-{}-{}-{}",
            std::process::id(),
            current_time_ms(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let store = TaskQueueStore::new(root.join("state.sqlite")).unwrap();
        (store, root)
    }

    #[test]
    fn insert_read_update_and_delete_task() {
        let (store, root) = test_store();
        let operation = StoredOperation::Copy {
            transfers: vec![StoredTransfer {
                source: StoredPath::from_path(Path::new("/tmp/source")),
                target: StoredPath::from_path(Path::new("/tmp/target")),
            }],
        };

        let id = store.insert_task(&operation).unwrap();
        let task = store.read_task(id).unwrap().unwrap();
        assert_eq!(task.id, id);
        assert_eq!(task.operation, operation);
        assert_eq!(task.status, StoredTaskStatus::Pending);
        assert_eq!(task.progress, StoredProgress::pending());
        assert_eq!(task.error, None);

        store.update_status(id, StoredTaskStatus::Running).unwrap();
        store
            .update_progress(id, StoredProgress::with_fraction(0.5))
            .unwrap();
        store.update_error(id, Some("boom")).unwrap();

        let updated = store.read_task(id).unwrap().unwrap();
        assert_eq!(updated.status, StoredTaskStatus::Running);
        assert_eq!(updated.progress, StoredProgress::with_fraction(0.5));
        assert_eq!(updated.error.as_deref(), Some("boom"));

        store
            .update_task_state(
                id,
                StoredTaskStatus::Completed,
                StoredProgress::with_fraction(1.0),
                None,
            )
            .unwrap();
        let completed = store.read_task(id).unwrap().unwrap();
        assert_eq!(completed.status, StoredTaskStatus::Completed);
        assert_eq!(completed.progress, StoredProgress::with_fraction(1.0));
        assert_eq!(completed.error, None);

        store.delete_task(id).unwrap();
        assert!(store.read_task(id).unwrap().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn marks_unfinished_tasks_failed_without_touching_failed_rows() {
        let (store, root) = test_store();
        let operation = StoredOperation::CreateDirectory {
            parent: StoredPath::from_path(Path::new("/tmp")),
        };
        let pending_id = store.insert_task(&operation).unwrap();
        let failed_id = store.insert_task(&operation).unwrap();
        store
            .update_status(failed_id, StoredTaskStatus::Failed)
            .unwrap();
        store.update_error(failed_id, Some("old error")).unwrap();
        let completed_id = store.insert_task(&operation).unwrap();
        store
            .update_status(completed_id, StoredTaskStatus::Completed)
            .unwrap();
        let canceled_id = store.insert_task(&operation).unwrap();
        store
            .update_status(canceled_id, StoredTaskStatus::Canceled)
            .unwrap();

        store
            .mark_unfinished_tasks_failed("recovered after restart")
            .unwrap();

        let recovered = store.read_task(pending_id).unwrap().unwrap();
        assert_eq!(recovered.status, StoredTaskStatus::Failed);
        assert_eq!(recovered.error.as_deref(), Some("recovered after restart"));

        let already_failed = store.read_task(failed_id).unwrap().unwrap();
        assert_eq!(already_failed.status, StoredTaskStatus::Failed);
        assert_eq!(already_failed.error.as_deref(), Some("old error"));
        let completed = store.read_task(completed_id).unwrap().unwrap();
        assert_eq!(completed.status, StoredTaskStatus::Completed);
        let canceled = store.read_task(canceled_id).unwrap().unwrap();
        assert_eq!(canceled.status, StoredTaskStatus::Canceled);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clear_tasks_removes_all_rows() {
        let (store, root) = test_store();
        let operation = StoredOperation::CreateDirectory {
            parent: StoredPath::from_path(Path::new("/tmp")),
        };
        store.insert_task(&operation).unwrap();
        store.insert_task(&operation).unwrap();

        store.clear_tasks().unwrap();

        assert!(store.read_tasks().unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_unix_path_roundtrips_through_json_and_database() {
        let (store, root) = test_store();
        let path = PathBuf::from(OsString::from_vec(b"/tmp/non-utf8-\xFF".to_vec()));
        let operation = StoredOperation::Trash {
            paths: vec![StoredPath::from_path(&path)],
        };
        let json = serde_json::to_string(&operation).unwrap();
        let decoded: StoredOperation = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, operation);

        let id = store.insert_task(&operation).unwrap();
        let task = store.read_task(id).unwrap().unwrap();
        let StoredOperation::Trash { paths } = task.operation else {
            panic!("expected trash operation");
        };
        assert_eq!(
            paths[0].to_path_buf().as_os_str().as_bytes(),
            path.as_os_str().as_bytes()
        );
        let _ = fs::remove_dir_all(root);
    }
}
