use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

use crate::database::{
    validate_classification_batch, DirectorySnapshot, FileClassification, IndexedFile,
    KnownDirectChild, KnownFileEntry, ObservedFile, ScanFileMutation, SearchDatabase,
};
use crate::error::{SearchError, SearchResult};
use crate::extractor::ExtractionStatus;

pub(crate) const MAX_WRITER_FILE_PAYLOAD_BYTES: usize = 2_500_000;
const MAX_WRITER_SCOPE_PATH_BYTES: usize = 8_192;
const INDEXED_FILE_FIXED_BYTES: usize = 1_024;

/// A single write operation for the dedicated writer thread to apply.
enum WriteCommand {
    UpsertObservable(Box<IndexedFile>),
    UpsertInaccessible(Box<IndexedFile>),
    ClassifyObserved {
        files: Vec<ObservedFile>,
        reply: Sender<SearchResult<Vec<FileClassification>>>,
    },
    ApplyFileBatch(Vec<ScanFileMutation>),
    UpsertDirectorySnapshot(Box<DirectorySnapshot>),
    DirectorySnapshot {
        path: PathBuf,
        reply: Sender<SearchResult<Option<DirectorySnapshot>>>,
    },
    KnownFilesPage {
        scope: PathBuf,
        after_path: Option<PathBuf>,
        limit: usize,
        reply: Sender<SearchResult<Vec<KnownFileEntry>>>,
    },
    DirectorySnapshotsPage {
        scope: PathBuf,
        after_path: Option<PathBuf>,
        limit: usize,
        reply: Sender<SearchResult<Vec<DirectorySnapshot>>>,
    },
    DirectChildrenPage {
        parent: PathBuf,
        after_path: Option<PathBuf>,
        limit: usize,
        reply: Sender<SearchResult<Vec<KnownDirectChild>>>,
    },
    MarkScopeInaccessible {
        scope: PathBuf,
        reply: Sender<SearchResult<bool>>,
    },
    DeleteScope(PathBuf),
    ReleaseIdleCache(Sender<SearchResult<()>>),
    /// 统计当前可查询条目，并继续复用 writer 持有的数据库连接。
    Count(Sender<SearchResult<u64>>),
    /// Drain everything queued before this point, then report the first error
    /// seen since the previous flush.
    Flush(Sender<SearchResult<()>>),
    /// Stop the writer after every previously queued command has been applied.
    /// Used when a stale daemon is retiring itself during a version handoff so
    /// the next daemon never races it for the writable SQLite connection.
    Shutdown(Option<Sender<SearchResult<()>>>),
}

/// Serializes every write to the search database through one owned connection
/// on a dedicated thread. This is the same discipline tracker-miner-fs uses:
/// a single writer funnels all store operations, so two threads can never race
/// for the write lock. Read paths use `SearchDatabase::open_read_only` instead
/// and, under WAL, never block this writer.
pub struct IndexWriter {
    sender: SyncSender<WriteCommand>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl IndexWriter {
    /// Takes ownership of the one writable database connection and moves it onto
    /// a background thread that applies commands in the order they arrive.
    pub fn spawn(database: SearchDatabase) -> Self {
        // 零容量通道让 crawler 与唯一 writer 直接交接，正文不能在内存队列中累积。
        let (sender, receiver) = mpsc::sync_channel(0);
        let join = thread::Builder::new()
            .name("file-search-writer".to_owned())
            .spawn(move || writer_loop(database, receiver))
            .expect("spawn search writer thread");
        Self {
            sender,
            join: Mutex::new(Some(join)),
        }
    }

    /// Blocks until the dedicated writer accepts the file, preventing payload backlog.
    pub fn upsert(&self, file: IndexedFile) -> SearchResult<()> {
        validate_indexed_file_payload(&file)?;
        self.sender
            .send(WriteCommand::UpsertObservable(Box::new(file)))
            .map_err(|_| writer_gone())
    }

    pub fn upsert_inaccessible(&self, file: IndexedFile) -> SearchResult<()> {
        validate_indexed_file_payload(&file)?;
        self.sender
            .send(WriteCommand::UpsertInaccessible(Box::new(file)))
            .map_err(|_| writer_gone())
    }

    pub(crate) fn classify_observed(
        &self,
        files: Vec<ObservedFile>,
    ) -> SearchResult<Vec<FileClassification>> {
        validate_classification_batch(&files)?;
        let (reply, outcome) = mpsc::channel();
        self.sender
            .send(WriteCommand::ClassifyObserved { files, reply })
            .map_err(|_| writer_gone())?;
        outcome.recv().map_err(|_| writer_gone())?
    }

    pub(crate) fn apply_file_batch(&self, mutations: Vec<ScanFileMutation>) -> SearchResult<()> {
        validate_scan_file_batch(&mutations)?;
        self.sender
            .send(WriteCommand::ApplyFileBatch(mutations))
            .map_err(|_| writer_gone())
    }

    pub(crate) fn upsert_directory_snapshot(
        &self,
        snapshot: DirectorySnapshot,
    ) -> SearchResult<()> {
        let path_bytes = snapshot.path.as_os_str().as_encoded_bytes().len();
        if path_bytes > MAX_WRITER_SCOPE_PATH_BYTES {
            return Err(SearchError::PayloadTooLarge {
                boundary: "directory snapshot path",
                actual_bytes: path_bytes,
                max_bytes: MAX_WRITER_SCOPE_PATH_BYTES,
            });
        }
        self.sender
            .send(WriteCommand::UpsertDirectorySnapshot(Box::new(snapshot)))
            .map_err(|_| writer_gone())
    }

    pub(crate) fn directory_snapshot(
        &self,
        path: PathBuf,
    ) -> SearchResult<Option<DirectorySnapshot>> {
        let (reply, outcome) = mpsc::channel();
        self.sender
            .send(WriteCommand::DirectorySnapshot { path, reply })
            .map_err(|_| writer_gone())?;
        outcome.recv().map_err(|_| writer_gone())?
    }

    pub(crate) fn known_files_page(
        &self,
        scope: PathBuf,
        after_path: Option<PathBuf>,
        limit: usize,
    ) -> SearchResult<Vec<KnownFileEntry>> {
        let (reply, outcome) = mpsc::channel();
        self.sender
            .send(WriteCommand::KnownFilesPage {
                scope,
                after_path,
                limit,
                reply,
            })
            .map_err(|_| writer_gone())?;
        outcome.recv().map_err(|_| writer_gone())?
    }

    pub(crate) fn directory_snapshots_page(
        &self,
        scope: PathBuf,
        after_path: Option<PathBuf>,
        limit: usize,
    ) -> SearchResult<Vec<DirectorySnapshot>> {
        let (reply, outcome) = mpsc::channel();
        self.sender
            .send(WriteCommand::DirectorySnapshotsPage {
                scope,
                after_path,
                limit,
                reply,
            })
            .map_err(|_| writer_gone())?;
        outcome.recv().map_err(|_| writer_gone())?
    }

    pub(crate) fn direct_children_page(
        &self,
        parent: PathBuf,
        after_path: Option<PathBuf>,
        limit: usize,
    ) -> SearchResult<Vec<KnownDirectChild>> {
        let (reply, outcome) = mpsc::channel();
        self.sender
            .send(WriteCommand::DirectChildrenPage {
                parent,
                after_path,
                limit,
                reply,
            })
            .map_err(|_| writer_gone())?;
        outcome.recv().map_err(|_| writer_gone())?
    }

    pub(crate) fn mark_scope_inaccessible(&self, scope: PathBuf) -> SearchResult<bool> {
        let (reply, outcome) = mpsc::channel();
        self.sender
            .send(WriteCommand::MarkScopeInaccessible { scope, reply })
            .map_err(|_| writer_gone())?;
        outcome.recv().map_err(|_| writer_gone())?
    }

    pub(crate) fn delete_scope(&self, scope: PathBuf) -> SearchResult<()> {
        self.sender
            .send(WriteCommand::DeleteScope(scope))
            .map_err(|_| writer_gone())
    }

    pub(crate) fn release_idle_cache(&self) -> SearchResult<()> {
        let (reply, outcome) = mpsc::channel();
        self.sender
            .send(WriteCommand::ReleaseIdleCache(reply))
            .map_err(|_| writer_gone())?;
        outcome.recv().map_err(|_| writer_gone())?
    }

    /// 返回当前可查询的索引条目数，并阻塞等待 writer 回复。
    pub fn count(&self) -> SearchResult<u64> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(WriteCommand::Count(tx))
            .map_err(|_| writer_gone())?;
        rx.recv().map_err(|_| writer_gone())?
    }

    /// Blocks until every previously queued write has been applied, then returns
    /// the first error the writer hit since the last flush (clearing it).
    pub fn flush(&self) -> SearchResult<()> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(WriteCommand::Flush(tx))
            .map_err(|_| writer_gone())?;
        rx.recv().map_err(|_| writer_gone())?
    }

    /// 排空此前接收的写入、停止并等待 writer 线程。首次调用持有完整关闭所有权，
    /// 并发或后续调用会等待首次调用结束后直接成功。
    pub fn shutdown(&self) -> SearchResult<()> {
        let mut join = self
            .join
            .lock()
            .expect("search index writer join mutex poisoned");
        let Some(writer_thread) = join.take() else {
            return Ok(());
        };

        let (tx, rx) = mpsc::channel();
        let writer_outcome = self
            .sender
            .send(WriteCommand::Shutdown(Some(tx)))
            .map_err(|_| writer_gone())
            .and_then(|()| rx.recv().map_err(|_| writer_gone()))
            .and_then(|outcome| outcome);
        let join_outcome = writer_thread.join().map_err(|_| {
            SearchError::WorkerFailed("search index writer thread panicked".to_owned())
        });

        writer_outcome.and(join_outcome)
    }
}

impl Drop for IndexWriter {
    fn drop(&mut self) {
        let _ = self.sender.send(WriteCommand::Shutdown(None));
        let join = self
            .join
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(join) = join.take() {
            let _ = join.join();
        }
    }
}

fn writer_loop(database: SearchDatabase, receiver: Receiver<WriteCommand>) {
    // The first write error since the last flush. Individual writes don't have a
    // caller waiting on them, so we stash the error and surface it at flush time.
    let mut pending_error: Option<SearchError> = None;
    while let Ok(command) = receiver.recv() {
        match command {
            WriteCommand::UpsertObservable(file) => {
                record_error(&mut pending_error, database.upsert_file(&file));
            }
            WriteCommand::UpsertInaccessible(file) => {
                record_error(&mut pending_error, database.upsert_inaccessible_file(&file));
            }
            WriteCommand::ClassifyObserved { files, reply } => {
                let outcome = match pending_error.take() {
                    Some(error) => Err(error),
                    None => database.classify_observed_files(&files),
                };
                let _ = reply.send(outcome);
            }
            WriteCommand::ApplyFileBatch(mutations) => {
                record_error(&mut pending_error, database.apply_file_batch(&mutations));
            }
            WriteCommand::UpsertDirectorySnapshot(snapshot) => {
                record_error(
                    &mut pending_error,
                    database.upsert_directory_snapshot(&snapshot),
                );
            }
            WriteCommand::DirectorySnapshot { path, reply } => {
                let outcome = match pending_error.take() {
                    Some(error) => Err(error),
                    None => database.directory_snapshot(&path),
                };
                let _ = reply.send(outcome);
            }
            WriteCommand::KnownFilesPage {
                scope,
                after_path,
                limit,
                reply,
            } => {
                let outcome = match pending_error.take() {
                    Some(error) => Err(error),
                    None => database.known_files_page(&scope, after_path.as_deref(), limit),
                };
                let _ = reply.send(outcome);
            }
            WriteCommand::DirectorySnapshotsPage {
                scope,
                after_path,
                limit,
                reply,
            } => {
                let outcome = match pending_error.take() {
                    Some(error) => Err(error),
                    None => database.directory_snapshots_page(&scope, after_path.as_deref(), limit),
                };
                let _ = reply.send(outcome);
            }
            WriteCommand::DirectChildrenPage {
                parent,
                after_path,
                limit,
                reply,
            } => {
                let outcome = match pending_error.take() {
                    Some(error) => Err(error),
                    None => database.direct_children_page(&parent, after_path.as_deref(), limit),
                };
                let _ = reply.send(outcome);
            }
            WriteCommand::MarkScopeInaccessible { scope, reply } => {
                let outcome = match pending_error.take() {
                    Some(error) => Err(error),
                    None => database.mark_scope_inaccessible(&scope),
                };
                let _ = reply.send(outcome);
            }
            WriteCommand::DeleteScope(scope) => {
                record_error(&mut pending_error, database.delete_scope(&scope));
            }
            WriteCommand::ReleaseIdleCache(reply) => {
                let outcome = match pending_error.take() {
                    Some(error) => Err(error),
                    None => database.checkpoint_and_release_idle_cache(),
                };
                let _ = reply.send(outcome);
            }
            WriteCommand::Count(reply) => {
                let _ = reply.send(database.indexed_file_count());
            }
            WriteCommand::Flush(reply) => {
                let _ = reply.send(match pending_error.take() {
                    Some(error) => Err(error),
                    None => Ok(()),
                });
            }
            WriteCommand::Shutdown(reply) => {
                if let Some(reply) = reply {
                    let _ = reply.send(match pending_error.take() {
                        Some(error) => Err(error),
                        None => Ok(()),
                    });
                }
                break;
            }
        }
    }
}

fn record_error(slot: &mut Option<SearchError>, result: SearchResult<()>) {
    if let Err(error) = result {
        eprintln!("search index write failed: {error}");
        if slot.is_none() {
            *slot = Some(error);
        }
    }
}

fn writer_gone() -> SearchError {
    SearchError::WorkerFailed("search index writer thread is no longer running".to_owned())
}

fn validate_indexed_file_payload(file: &IndexedFile) -> SearchResult<()> {
    let estimated_bytes = indexed_file_payload_bytes(file);
    if estimated_bytes > MAX_WRITER_FILE_PAYLOAD_BYTES {
        return Err(SearchError::PayloadTooLarge {
            boundary: "indexed file command",
            actual_bytes: estimated_bytes,
            max_bytes: MAX_WRITER_FILE_PAYLOAD_BYTES,
        });
    }
    Ok(())
}

pub(crate) fn scan_file_mutation_bytes(mutation: &ScanFileMutation) -> usize {
    match mutation {
        ScanFileMutation::Observable(file) => indexed_file_payload_bytes(file),
        ScanFileMutation::Inaccessible { scope, file } => indexed_file_payload_bytes(file)
            .saturating_add(scope.as_os_str().as_encoded_bytes().len()),
    }
}

fn validate_scan_file_batch(mutations: &[ScanFileMutation]) -> SearchResult<()> {
    let estimated_bytes = mutations.iter().fold(0_usize, |total, mutation| {
        total.saturating_add(scan_file_mutation_bytes(mutation))
    });
    if mutations.len() > crate::database::MAX_CLASSIFICATION_BATCH_ENTRIES
        || estimated_bytes > MAX_WRITER_FILE_PAYLOAD_BYTES
    {
        return Err(SearchError::PayloadTooLarge {
            boundary: "scan file batch",
            actual_bytes: estimated_bytes,
            max_bytes: MAX_WRITER_FILE_PAYLOAD_BYTES,
        });
    }
    Ok(())
}

fn indexed_file_payload_bytes(file: &IndexedFile) -> usize {
    INDEXED_FILE_FIXED_BYTES
        .saturating_add(file.path.as_os_str().as_encoded_bytes().len())
        .saturating_add(file.parent_path.as_os_str().as_encoded_bytes().len())
        .saturating_add(file.display_name.len())
        .saturating_add(file.mime_type.as_ref().map_or(0, String::len))
        .saturating_add(file.content.as_ref().map_or(0, String::len))
        .saturating_add(extraction_status_bytes(&file.extraction_status))
}

fn extraction_status_bytes(status: &ExtractionStatus) -> usize {
    match status {
        ExtractionStatus::ReadFailed { message } => message.len(),
        ExtractionStatus::ToolUnavailable { tool }
        | ExtractionStatus::TimedOut { tool }
        | ExtractionStatus::ResourceBudgetExceeded { tool } => tool.len(),
        ExtractionStatus::ToolFailed { tool, message } => tool.len().saturating_add(message.len()),
        ExtractionStatus::Indexed
        | ExtractionStatus::Disabled
        | ExtractionStatus::Unsupported
        | ExtractionStatus::TooLarge
        | ExtractionStatus::NonUtf8 => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use tempfile::tempdir;

    use crate::database::{EntryStageProgress, IndexedEntryStageState, SearchDatabase};
    use crate::extractor::ExtractionStatus;
    use crate::model::{SearchFileKind, SearchQuery};

    use super::*;

    fn sample_file(path: &Path, inode: u64, mtime_ns: i64) -> IndexedFile {
        IndexedFile {
            path: path.to_path_buf(),
            parent_path: path.parent().unwrap().to_path_buf(),
            display_name: path.file_name().unwrap().to_string_lossy().into_owned(),
            kind: SearchFileKind::File,
            size: 6,
            modified_ms: Some(1),
            accessed_ms: None,
            created_ms: None,
            mime_type: Some("text/plain".to_owned()),
            stage_state: IndexedEntryStageState {
                metadata: EntryStageProgress::Complete,
                content: EntryStageProgress::Complete,
            },
            content: Some("needle".to_owned()),
            extraction_status: ExtractionStatus::Indexed,
            device: Some(1),
            inode: Some(inode),
            mtime_ns: Some(mtime_ns),
            ctime_ns: Some(mtime_ns),
        }
    }

    #[test]
    fn writes_are_visible_to_a_concurrent_reader_without_locking() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.sqlite");
        let file_path = dir.path().join("note.txt");

        let writer = IndexWriter::spawn(SearchDatabase::open(&db_path).unwrap());
        let reader = SearchDatabase::open_read_only(&db_path).unwrap();

        writer.upsert(sample_file(&file_path, 10, 20)).unwrap();
        writer.flush().unwrap();

        // The read-only connection sees the committed write while the writer
        // thread is still alive and holding the write connection.
        let batch = reader.search(&SearchQuery::global(1, "needle")).unwrap();
        assert_eq!(batch.hits.len(), 1);
        assert_eq!(batch.hits[0].display_name, "note.txt");
    }

    #[test]
    fn signatures_round_trip_through_the_writer() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.sqlite");
        let file_path = dir.path().join("note.txt");

        let writer = IndexWriter::spawn(SearchDatabase::open(&db_path).unwrap());
        writer.upsert(sample_file(&file_path, 42, 99)).unwrap();
        writer.flush().unwrap();

        let stored = writer
            .classify_observed(vec![ObservedFile {
                path: file_path.clone(),
                signature: crate::database::FileSignature {
                    device: Some(1),
                    inode: Some(42),
                    mtime_ns: Some(99),
                    ctime_ns: Some(99),
                    size: 6,
                },
            }])
            .unwrap()
            .pop()
            .unwrap()
            .known_entry
            .expect("signature recorded");
        assert_eq!(
            stored.stage_state,
            IndexedEntryStageState {
                metadata: EntryStageProgress::Complete,
                content: EntryStageProgress::Complete,
            }
        );
        let stored_signature = stored.signature;
        assert_eq!(stored_signature.inode, Some(42));
        assert_eq!(stored_signature.mtime_ns, Some(99));
        assert_eq!(writer.count().unwrap(), 1);
    }

    #[test]
    fn skipped_stage_state_round_trips_through_the_writer() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.sqlite");
        let file_path = dir.path().join("too-large.txt");

        let writer = IndexWriter::spawn(SearchDatabase::open(&db_path).unwrap());
        let mut file = sample_file(&file_path, 42, 99);
        file.display_name = "too-large.txt".to_owned();
        file.content = None;
        file.extraction_status = ExtractionStatus::TooLarge;
        file.stage_state = IndexedEntryStageState {
            metadata: EntryStageProgress::Complete,
            content: EntryStageProgress::Skipped,
        };
        writer.upsert(file).unwrap();
        writer.flush().unwrap();

        let stored = writer
            .classify_observed(vec![ObservedFile {
                path: file_path,
                signature: crate::database::FileSignature {
                    device: Some(1),
                    inode: Some(42),
                    mtime_ns: Some(99),
                    ctime_ns: Some(99),
                    size: 6,
                },
            }])
            .unwrap()
            .pop()
            .unwrap()
            .known_entry
            .expect("signature recorded");
        assert_eq!(
            stored.stage_state,
            IndexedEntryStageState {
                metadata: EntryStageProgress::Complete,
                content: EntryStageProgress::Skipped,
            }
        );
    }

    #[test]
    fn local_delete_only_removes_its_target() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.sqlite");
        let kept = dir.path().join("kept.txt");
        let gone = dir.path().join("gone.txt");

        let writer = IndexWriter::spawn(SearchDatabase::open(&db_path).unwrap());
        writer.upsert(sample_file(&kept, 1, 1)).unwrap();
        writer.upsert(sample_file(&gone, 2, 2)).unwrap();
        writer.flush().unwrap();

        writer.delete_scope(gone).unwrap();
        writer.flush().unwrap();

        assert_eq!(writer.count().unwrap(), 1);
        let reader = SearchDatabase::open_read_only(&db_path).unwrap();
        let batch = reader.search(&SearchQuery::global(1, "needle")).unwrap();
        assert_eq!(batch.hits.len(), 1);
        assert_eq!(batch.hits[0].path, kept);
    }

    #[test]
    fn shutdown_drains_queued_writes_before_stopping_writer() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.sqlite");
        let file_path = dir.path().join("note.txt");

        let writer = IndexWriter::spawn(SearchDatabase::open(&db_path).unwrap());
        writer.upsert(sample_file(&file_path, 7, 11)).unwrap();
        writer.shutdown().unwrap();

        let reader = SearchDatabase::open_read_only(&db_path).unwrap();
        let batch = reader.search(&SearchQuery::global(1, "needle")).unwrap();
        assert_eq!(batch.hits.len(), 1);
        assert_eq!(batch.hits[0].display_name, "note.txt");
    }

    #[test]
    fn shutdown_is_idempotent() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.sqlite");
        let writer = IndexWriter::spawn(SearchDatabase::open(&db_path).unwrap());

        writer.shutdown().unwrap();
        writer.shutdown().unwrap();
    }

    #[test]
    fn concurrent_shutdown_calls_share_one_join() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.sqlite");
        let writer = Arc::new(IndexWriter::spawn(SearchDatabase::open(&db_path).unwrap()));

        let first = {
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || writer.shutdown())
        };
        let second = {
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || writer.shutdown())
        };

        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
    }

    #[test]
    fn oversized_file_command_is_rejected_before_writer_handoff() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("search.sqlite");
        let file_path = directory.path().join("oversized.txt");
        let writer = IndexWriter::spawn(SearchDatabase::open(&database_path).unwrap());
        let mut file = sample_file(&file_path, 1, 1);
        file.content = Some("x".repeat(MAX_WRITER_FILE_PAYLOAD_BYTES));

        assert!(matches!(
            writer.upsert(file),
            Err(SearchError::PayloadTooLarge {
                boundary: "indexed file command",
                ..
            })
        ));
        assert_eq!(writer.count().unwrap(), 0);
    }

    #[test]
    fn file_batch_commits_many_metadata_rows_with_one_handoff() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("search.sqlite");
        let writer = IndexWriter::spawn(SearchDatabase::open(&database_path).unwrap());
        let mutations = (0..crate::database::MAX_CLASSIFICATION_BATCH_ENTRIES)
            .map(|index| {
                let path = directory.path().join(format!("file-{index}.bin"));
                let mut file = sample_file(&path, index as u64, index as i64);
                file.content = None;
                file.extraction_status = ExtractionStatus::Unsupported;
                file.stage_state.content = EntryStageProgress::Skipped;
                ScanFileMutation::Observable(file)
            })
            .collect();

        writer.apply_file_batch(mutations).unwrap();
        writer.flush().unwrap();

        assert_eq!(writer.count().unwrap(), 128);
    }

    #[test]
    fn file_batch_rejects_combined_payload_above_the_fixed_budget() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("search.sqlite");
        let writer = IndexWriter::spawn(SearchDatabase::open(&database_path).unwrap());
        let mutations = (0..2)
            .map(|index| {
                let path = directory.path().join(format!("large-{index}.txt"));
                let mut file = sample_file(&path, index, index as i64);
                file.content = Some("x".repeat(1_300_000));
                ScanFileMutation::Observable(file)
            })
            .collect();

        assert!(matches!(
            writer.apply_file_batch(mutations),
            Err(SearchError::PayloadTooLarge {
                boundary: "scan file batch",
                ..
            })
        ));
        assert_eq!(writer.count().unwrap(), 0);
    }
}
