use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::crawler::IndexMaintenanceProgress;
use crate::database::SearchDatabase;
use crate::error::{SearchError, SearchResult};
use crate::model::{
    ExtractorCapability, IndexHealth, IndexPhase, IndexStatus, SearchQuery, SearchResultBatch,
};
use crate::writer::IndexWriter;
use crate::{SearchIndexConfig, SearchIndexer};

mod bounded_paths;
mod memory;
mod watch_coverage;
mod watch_ingress;

use self::bounded_paths::BoundedPathSet;
use self::memory::release_allocator_idle_pages;
use self::watch_coverage::WatchCoverageHealth;
#[cfg(test)]
use self::watch_ingress::changed_paths_from_watch_event;
use self::watch_ingress::{DaemonWatchIngress, DaemonWatchIngressBootstrap};

const DIRTY_ROOT_QUIET_WINDOW: Duration = Duration::from_secs(2);
const DIRTY_ROOT_RETRY_BASE: Duration = Duration::from_secs(30);
const DIRTY_ROOT_RETRY_MAX: Duration = Duration::from_secs(15 * 60);
const MAX_PENDING_LOCAL_PATHS: usize = 2_048;
const MAX_PENDING_LOCAL_PATH_BYTES: usize = 1_048_576;
const MAX_PENDING_COVERAGE_SCOPES: usize = 256;
const MAX_PENDING_COVERAGE_SCOPE_BYTES: usize = 262_144;

#[derive(Debug, Clone, PartialEq, Eq)]
enum DaemonLifecyclePhase {
    Starting,
    Checking {
        checked_entries: u64,
        changed_entries: u64,
    },
    Crawling {
        scanned_entries: u64,
        current_scope: PathBuf,
    },
    Applying {
        pending_mutations: u64,
    },
    Complete,
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DaemonLifecycleSnapshot {
    phase: DaemonLifecyclePhase,
    maintenance_backend_failure: Option<String>,
    watch_coverage_health: WatchCoverageHealth,
    visible_indexed_files: u64,
    capabilities: Vec<ExtractorCapability>,
}

impl DaemonLifecycleSnapshot {
    fn starting(visible_indexed_files: u64) -> Self {
        Self {
            phase: DaemonLifecyclePhase::Starting,
            maintenance_backend_failure: None,
            watch_coverage_health: WatchCoverageHealth::Healthy,
            visible_indexed_files,
            capabilities: Vec::new(),
        }
    }

    fn begin_checking(&mut self) {
        self.phase = DaemonLifecyclePhase::Checking {
            checked_entries: 0,
            changed_entries: 0,
        };
    }

    fn apply_progress(&mut self, progress: IndexMaintenanceProgress) {
        self.phase = match progress {
            IndexMaintenanceProgress::Checking {
                checked_entries,
                changed_entries,
            } => DaemonLifecyclePhase::Checking {
                checked_entries,
                changed_entries,
            },
            IndexMaintenanceProgress::Crawling {
                scanned_entries,
                current_scope,
            } => DaemonLifecyclePhase::Crawling {
                scanned_entries,
                current_scope,
            },
            IndexMaintenanceProgress::Applying { pending_mutations } => {
                DaemonLifecyclePhase::Applying { pending_mutations }
            }
        };
    }

    fn finish_watch_cycle(&mut self, visible_indexed_files: u64) {
        self.phase = DaemonLifecyclePhase::Complete;
        self.visible_indexed_files = visible_indexed_files;
    }

    fn record_error(&mut self, message: String) {
        self.phase = DaemonLifecyclePhase::Failed { message };
    }

    fn record_maintenance_failure(&mut self, message: String) {
        self.maintenance_backend_failure = Some(message);
    }

    fn update_watch_coverage(&mut self, health: WatchCoverageHealth) {
        self.watch_coverage_health = health;
    }

    fn to_index_status(&self) -> IndexStatus {
        let phase = match &self.phase {
            DaemonLifecyclePhase::Starting => IndexPhase::Starting,
            DaemonLifecyclePhase::Checking {
                checked_entries,
                changed_entries,
            } => IndexPhase::Checking {
                checked_entries: *checked_entries,
                changed_entries: *changed_entries,
            },
            DaemonLifecyclePhase::Crawling {
                scanned_entries,
                current_scope,
            } => IndexPhase::Crawling {
                scanned_entries: *scanned_entries,
                current_scope: current_scope.clone(),
            },
            DaemonLifecyclePhase::Applying { pending_mutations } => IndexPhase::Applying {
                pending_mutations: *pending_mutations,
            },
            DaemonLifecyclePhase::Complete => IndexPhase::Complete {
                indexed_files: self.visible_indexed_files,
            },
            DaemonLifecyclePhase::Failed { message } => IndexPhase::Failed {
                message: message.clone(),
            },
        };
        IndexStatus {
            phase,
            health: self.index_health(),
            capabilities: self.capabilities.clone(),
        }
    }

    fn index_health(&self) -> IndexHealth {
        if let Some(message) = self.maintenance_backend_failure.as_ref() {
            return IndexHealth::Error {
                message: message.clone(),
            };
        }
        match &self.watch_coverage_health {
            WatchCoverageHealth::Healthy => IndexHealth::Healthy,
            WatchCoverageHealth::Incomplete { message, .. } => IndexHealth::Degraded {
                message: message.clone(),
            },
            WatchCoverageHealth::BackendUnavailable { message } => IndexHealth::Error {
                message: message.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DaemonWorkRequest {
    StartupCheck,
    ChangedPaths { changed_paths: Vec<PathBuf> },
    CoverageRepair { scopes: Vec<PathBuf> },
    DirtyRootRecovery { roots: Vec<PathBuf> },
    Shutdown,
}

#[derive(Debug, Clone)]
struct DirtyRootRecoveryState {
    ready_at: Instant,
    retry_attempts: u32,
    running: bool,
    dirtied_while_running: bool,
}

#[derive(Debug)]
struct PendingDaemonWork {
    roots: Vec<PathBuf>,
    startup_check_requested: bool,
    changed_watch_paths: BoundedPathSet,
    coverage_repair_scopes: BoundedPathSet,
    dirty_roots: BTreeMap<PathBuf, DirtyRootRecoveryState>,
    shutdown_requested: bool,
}

impl PendingDaemonWork {
    fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            startup_check_requested: false,
            changed_watch_paths: BoundedPathSet::new(
                MAX_PENDING_LOCAL_PATHS,
                MAX_PENDING_LOCAL_PATH_BYTES,
            ),
            coverage_repair_scopes: BoundedPathSet::new(
                MAX_PENDING_COVERAGE_SCOPES,
                MAX_PENDING_COVERAGE_SCOPE_BYTES,
            ),
            dirty_roots: BTreeMap::new(),
            shutdown_requested: false,
        }
    }

    fn absorb_request(&mut self, work_request: DaemonWorkRequest, now: Instant) {
        match work_request {
            DaemonWorkRequest::StartupCheck => self.startup_check_requested = true,
            DaemonWorkRequest::ChangedPaths { changed_paths } => {
                self.absorb_paths(changed_paths, false, now)
            }
            DaemonWorkRequest::CoverageRepair { scopes } => self.absorb_paths(scopes, true, now),
            DaemonWorkRequest::DirtyRootRecovery { roots } => self.request_dirty_roots(roots, now),
            DaemonWorkRequest::Shutdown => self.shutdown_requested = true,
        }
    }

    fn absorb_paths(&mut self, paths: Vec<PathBuf>, coverage_repair: bool, now: Instant) {
        for path in paths {
            let insertion = if coverage_repair {
                self.coverage_repair_scopes.insert(path.clone())
            } else {
                self.changed_watch_paths.insert(path.clone())
            };
            if insertion.is_ok() {
                continue;
            }
            let roots = self.containing_roots(&path);
            for root in &roots {
                self.changed_watch_paths
                    .retain(|pending| !path_is_same_or_descendant(pending, root));
                self.coverage_repair_scopes
                    .retain(|pending| !path_is_same_or_descendant(pending, root));
            }
            self.request_dirty_roots(roots, now);
        }
    }

    fn request_dirty_roots(&mut self, roots: Vec<PathBuf>, now: Instant) {
        for root in roots {
            let Some(state) = self.dirty_roots.get_mut(&root) else {
                self.dirty_roots.insert(
                    root,
                    DirtyRootRecoveryState {
                        ready_at: now + DIRTY_ROOT_QUIET_WINDOW,
                        retry_attempts: 0,
                        running: false,
                        dirtied_while_running: false,
                    },
                );
                continue;
            };
            if state.running {
                state.dirtied_while_running = true;
            } else {
                state.ready_at = state.ready_at.max(now + DIRTY_ROOT_QUIET_WINDOW);
            }
        }
    }

    fn take_next_work(&mut self, now: Instant) -> Option<DaemonWorkRequest> {
        if self.shutdown_requested {
            self.shutdown_requested = false;
            return Some(DaemonWorkRequest::Shutdown);
        }
        if self.startup_check_requested {
            self.startup_check_requested = false;
            return Some(DaemonWorkRequest::StartupCheck);
        }

        let changed_paths = self.changed_watch_paths.take_paths();
        if !changed_paths.is_empty() {
            return Some(DaemonWorkRequest::ChangedPaths { changed_paths });
        }

        let scopes = self.coverage_repair_scopes.take_paths();
        if !scopes.is_empty() {
            return Some(DaemonWorkRequest::CoverageRepair { scopes });
        }

        let ready_roots = self
            .dirty_roots
            .iter_mut()
            .filter_map(|(root, state)| {
                (!state.running && state.ready_at <= now).then(|| {
                    state.running = true;
                    state.dirtied_while_running = false;
                    root.clone()
                })
            })
            .collect::<Vec<_>>();
        (!ready_roots.is_empty())
            .then_some(DaemonWorkRequest::DirtyRootRecovery { roots: ready_roots })
    }

    fn finish_dirty_root_recovery(&mut self, roots: &[PathBuf], succeeded: bool, now: Instant) {
        for root in roots {
            let Some(state) = self.dirty_roots.get_mut(root) else {
                continue;
            };
            if succeeded && !state.dirtied_while_running {
                self.dirty_roots.remove(root);
                continue;
            }
            state.running = false;
            state.retry_attempts = state.retry_attempts.saturating_add(1);
            state.ready_at = now + dirty_root_retry_delay(state.retry_attempts);
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.dirty_roots
            .values()
            .filter(|state| !state.running)
            .map(|state| state.ready_at)
            .min()
    }

    fn containing_roots(&self, path: &Path) -> Vec<PathBuf> {
        let roots = self
            .roots
            .iter()
            .filter(|root| path_is_same_or_descendant(path, root))
            .cloned()
            .collect::<Vec<_>>();
        if roots.is_empty() {
            self.roots.clone()
        } else {
            roots
        }
    }
}

fn dirty_root_retry_delay(retry_attempts: u32) -> Duration {
    let multiplier = 1_u32
        .checked_shl(retry_attempts.saturating_sub(1).min(5))
        .unwrap_or(32);
    DIRTY_ROOT_RETRY_BASE
        .checked_mul(multiplier)
        .unwrap_or(DIRTY_ROOT_RETRY_MAX)
        .min(DIRTY_ROOT_RETRY_MAX)
}

#[derive(Debug)]
struct DaemonWorkQueueState {
    accepting_new_work: bool,
    pending_work: PendingDaemonWork,
}

#[derive(Debug)]
struct DaemonWorkQueue {
    state: Mutex<DaemonWorkQueueState>,
    ready_signal: Condvar,
}

impl DaemonWorkQueue {
    fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            state: Mutex::new(DaemonWorkQueueState {
                accepting_new_work: true,
                pending_work: PendingDaemonWork::new(roots),
            }),
            ready_signal: Condvar::new(),
        }
    }

    fn enqueue(&self, work_request: DaemonWorkRequest) -> SearchResult<()> {
        let mut state = self
            .state
            .lock()
            .expect("search daemon work queue mutex poisoned");
        if !state.accepting_new_work {
            return Err(daemon_core_stopped());
        }
        state
            .pending_work
            .absorb_request(work_request, Instant::now());
        self.ready_signal.notify_one();
        Ok(())
    }

    fn begin_shutdown(&self) {
        let mut state = self
            .state
            .lock()
            .expect("search daemon work queue mutex poisoned");
        state.accepting_new_work = false;
        state.pending_work.shutdown_requested = true;
        self.ready_signal.notify_one();
    }

    fn finish_dirty_root_recovery(&self, roots: &[PathBuf], succeeded: bool) {
        let mut state = self
            .state
            .lock()
            .expect("search daemon work queue mutex poisoned");
        state
            .pending_work
            .finish_dirty_root_recovery(roots, succeeded, Instant::now());
        self.ready_signal.notify_one();
    }

    fn wait_for_next_work(&self) -> DaemonWorkRequest {
        let mut state = self
            .state
            .lock()
            .expect("search daemon work queue mutex poisoned");
        loop {
            if let Some(work_request) = state.pending_work.take_next_work(Instant::now()) {
                return work_request;
            }
            match state.pending_work.next_deadline() {
                Some(deadline) => {
                    let wait = deadline.saturating_duration_since(Instant::now());
                    let (next_state, _) = self
                        .ready_signal
                        .wait_timeout(state, wait)
                        .expect("search daemon work queue mutex poisoned");
                    state = next_state;
                }
                None => {
                    state = self
                        .ready_signal
                        .wait(state)
                        .expect("search daemon work queue mutex poisoned");
                }
            }
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
enum DaemonRuntimePhase {
    #[default]
    Created,
    Maintaining,
    Stopped,
}

#[derive(Default)]
struct DaemonRuntimeState {
    phase: DaemonRuntimePhase,
    watch_ingress: Option<DaemonWatchIngress>,
}

pub struct SearchDaemonCore {
    query_database: Mutex<SearchDatabase>,
    database_path: PathBuf,
    config: SearchIndexConfig,
    writer: Arc<IndexWriter>,
    lifecycle_snapshot: Arc<Mutex<DaemonLifecycleSnapshot>>,
    work_queue: Arc<DaemonWorkQueue>,
    directory_snapshot_epoch: Arc<AtomicU64>,
    crawl_cancellation: CancellationToken,
    worker_join: Mutex<Option<thread::JoinHandle<SearchResult<()>>>>,
    runtime_state: Mutex<DaemonRuntimeState>,
    shutdown_serialization: Mutex<()>,
}

impl SearchDaemonCore {
    pub fn new(database_path: PathBuf, config: SearchIndexConfig) -> SearchResult<Self> {
        config
            .validate()
            .map_err(SearchError::InvalidConfiguration)?;
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| SearchError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let writable_database = SearchDatabase::open(&database_path)?;
        let query_database = SearchDatabase::open_read_only(&database_path)?;
        let writer = Arc::new(IndexWriter::spawn(writable_database));
        let known_count = writer.count()?;
        let lifecycle_snapshot =
            Arc::new(Mutex::new(DaemonLifecycleSnapshot::starting(known_count)));
        let work_queue = Arc::new(DaemonWorkQueue::new(config.roots.clone()));
        let directory_snapshot_epoch = Arc::new(AtomicU64::new(0));
        let crawl_cancellation = CancellationToken::new();

        let worker_join = spawn_daemon_worker(
            Arc::clone(&writer),
            config.clone(),
            Arc::clone(&lifecycle_snapshot),
            Arc::clone(&work_queue),
            Arc::clone(&directory_snapshot_epoch),
            crawl_cancellation.clone(),
        )?;

        Ok(Self {
            query_database: Mutex::new(query_database),
            database_path,
            config,
            writer,
            lifecycle_snapshot,
            work_queue,
            directory_snapshot_epoch,
            crawl_cancellation,
            worker_join: Mutex::new(Some(worker_join)),
            runtime_state: Mutex::new(DaemonRuntimeState::default()),
            shutdown_serialization: Mutex::new(()),
        })
    }

    pub fn start_index_maintenance(&self) -> SearchResult<()> {
        let mut runtime_state = self
            .runtime_state
            .lock()
            .expect("search daemon runtime mutex poisoned");
        match runtime_state.phase {
            DaemonRuntimePhase::Maintaining => return Ok(()),
            DaemonRuntimePhase::Stopped => return Err(daemon_core_stopped()),
            DaemonRuntimePhase::Created => {}
        }

        let (watch_ingress, maintenance_failure) = match DaemonWatchIngressBootstrap::establish(
            self.config.clone(),
            self.database_path.clone(),
            Arc::clone(&self.directory_snapshot_epoch),
        )
        .and_then(|bootstrap| {
            bootstrap.spawn(
                Arc::clone(&self.work_queue),
                Arc::clone(&self.lifecycle_snapshot),
            )
        }) {
            Ok(watch_ingress) => (Some(watch_ingress), None),
            Err(error) => (None, Some(error)),
        };

        if let Some(error) = maintenance_failure.as_ref() {
            self.record_maintenance_failure(error.to_string());
        }

        if let Err(error) = self.enqueue_work(DaemonWorkRequest::StartupCheck) {
            if let Some(watch_ingress) = watch_ingress {
                watch_ingress.shutdown();
            }
            return Err(error);
        }

        runtime_state.watch_ingress = watch_ingress;
        runtime_state.phase = DaemonRuntimePhase::Maintaining;

        match maintenance_failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn current_status(&self) -> IndexStatus {
        self.lifecycle_snapshot
            .lock()
            .expect("search daemon lifecycle mutex poisoned")
            .to_index_status()
    }

    pub fn search(&self, query: &SearchQuery) -> SearchResult<SearchResultBatch> {
        self.query_database
            .lock()
            .expect("search query database mutex poisoned")
            .search(query)
    }

    pub(crate) fn record_maintenance_failure(&self, message: String) {
        self.lifecycle_snapshot
            .lock()
            .expect("search daemon lifecycle mutex poisoned")
            .record_maintenance_failure(message);
    }

    pub fn shutdown(&self) -> SearchResult<()> {
        let _shutdown = self
            .shutdown_serialization
            .lock()
            .expect("search daemon shutdown mutex poisoned");
        let watch_ingress = {
            let mut runtime_state = self
                .runtime_state
                .lock()
                .expect("search daemon runtime mutex poisoned");
            runtime_state.phase = DaemonRuntimePhase::Stopped;
            runtime_state.watch_ingress.take()
        };
        if let Some(watch_ingress) = watch_ingress {
            watch_ingress.shutdown();
        }
        self.crawl_cancellation.cancel();
        self.work_queue.begin_shutdown();
        let worker_outcome = self.join_worker();
        let writer_outcome = self.writer.shutdown();
        worker_outcome.and(writer_outcome)
    }

    fn enqueue_work(&self, work_request: DaemonWorkRequest) -> SearchResult<()> {
        self.work_queue.enqueue(work_request)
    }

    fn join_worker(&self) -> SearchResult<()> {
        let worker_join = self
            .worker_join
            .lock()
            .expect("search daemon worker join mutex poisoned")
            .take();
        let Some(worker_join) = worker_join else {
            return Ok(());
        };
        worker_join.join().map_err(|_| {
            SearchError::WorkerFailed("search daemon worker thread panicked".to_owned())
        })?
    }
}

impl Drop for SearchDaemonCore {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn spawn_daemon_worker(
    writer: Arc<IndexWriter>,
    config: SearchIndexConfig,
    lifecycle_snapshot: Arc<Mutex<DaemonLifecycleSnapshot>>,
    work_queue: Arc<DaemonWorkQueue>,
    directory_snapshot_epoch: Arc<AtomicU64>,
    crawl_cancellation: CancellationToken,
) -> SearchResult<thread::JoinHandle<SearchResult<()>>> {
    thread::Builder::new()
        .name("file-search-daemon-core".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| {
                    SearchError::WorkerFailed(format!(
                        "could not create search daemon runtime: {error}"
                    ))
                })?;
            loop {
                let work_request = work_queue.wait_for_next_work();
                if matches!(work_request, DaemonWorkRequest::Shutdown) {
                    break;
                }
                let dirty_roots = match &work_request {
                    DaemonWorkRequest::DirtyRootRecovery { roots } => Some(roots.clone()),
                    _ => None,
                };
                let succeeded = run_index_maintenance(
                    &runtime,
                    Arc::clone(&writer),
                    &config,
                    Arc::clone(&lifecycle_snapshot),
                    &directory_snapshot_epoch,
                    work_request,
                    &crawl_cancellation,
                );
                if let Some(roots) = dirty_roots {
                    work_queue.finish_dirty_root_recovery(&roots, succeeded);
                }
            }
            Ok(())
        })
        .map_err(|error| {
            SearchError::WorkerFailed(format!("could not spawn search daemon worker: {error}"))
        })
}

fn run_index_maintenance(
    runtime: &tokio::runtime::Runtime,
    writer: Arc<IndexWriter>,
    config: &SearchIndexConfig,
    lifecycle_snapshot: Arc<Mutex<DaemonLifecycleSnapshot>>,
    directory_snapshot_epoch: &AtomicU64,
    work_request: DaemonWorkRequest,
    crawl_cancellation: &CancellationToken,
) -> bool {
    let work_label = work_request_label(&work_request);
    lifecycle_snapshot
        .lock()
        .expect("search daemon lifecycle mutex poisoned")
        .begin_checking();

    let maintenance_outcome = runtime.block_on(async {
        let indexer = SearchIndexer::new(Arc::clone(&writer), config.clone());
        let progress_snapshot = Arc::clone(&lifecycle_snapshot);
        let on_progress = move |progress| {
            progress_snapshot
                .lock()
                .expect("search daemon lifecycle mutex poisoned")
                .apply_progress(progress);
        };

        let stats = match work_request {
            DaemonWorkRequest::StartupCheck => {
                indexer
                    .rebuild_with_progress_cancelled(crawl_cancellation, on_progress)
                    .await?
            }
            DaemonWorkRequest::ChangedPaths { changed_paths } => {
                indexer
                    .rebuild_paths_with_progress_cancelled(
                        changed_paths,
                        crawl_cancellation,
                        on_progress,
                    )
                    .await?
            }
            DaemonWorkRequest::CoverageRepair { scopes } => {
                indexer
                    .repair_scopes_with_progress_cancelled(scopes, crawl_cancellation, on_progress)
                    .await?
            }
            DaemonWorkRequest::DirtyRootRecovery { roots } => {
                indexer
                    .recover_dirty_roots_with_progress_cancelled(
                        roots,
                        crawl_cancellation,
                        on_progress,
                    )
                    .await?
            }
            DaemonWorkRequest::Shutdown => return Ok::<_, SearchError>((Default::default(), 0)),
        };
        let total = writer.count()?;
        Ok::<_, SearchError>((stats, total))
    });
    release_allocator_idle_pages();
    let mut snapshot = lifecycle_snapshot
        .lock()
        .expect("search daemon lifecycle mutex poisoned");
    match maintenance_outcome {
        Ok((stats, total)) => {
            if stats.directory_snapshots_changed > 0 {
                directory_snapshot_epoch.fetch_add(1, Ordering::AcqRel);
            }
            eprintln!(
                "{work_label} complete: {} checked, {} changed, {} reindexed, {} directories enumerated, {} database mutations, {} content reads ({} total)",
                stats.checked,
                stats.changed,
                stats.reindexed,
                stats.directories_enumerated,
                stats.database_mutations,
                stats.content_reads,
                total
            );
            snapshot.finish_watch_cycle(total);
            true
        }
        Err(SearchError::Cancelled) => false,
        Err(error) => {
            snapshot.record_error(error.to_string());
            false
        }
    }
}

fn work_request_label(work_request: &DaemonWorkRequest) -> String {
    match work_request {
        DaemonWorkRequest::StartupCheck => "startup check".to_owned(),
        DaemonWorkRequest::ChangedPaths { changed_paths } => {
            format!("changed path batch ({})", changed_paths.len())
        }
        DaemonWorkRequest::CoverageRepair { scopes } => {
            format!("coverage repair ({})", scopes.len())
        }
        DaemonWorkRequest::DirtyRootRecovery { roots } => {
            format!("dirty root recovery ({})", roots.len())
        }
        DaemonWorkRequest::Shutdown => "shutdown".to_owned(),
    }
}

fn path_is_same_or_descendant(path: &Path, scope: &Path) -> bool {
    path == scope || path.starts_with(scope)
}

fn daemon_core_stopped() -> SearchError {
    SearchError::WorkerFailed("search daemon core is no longer running".to_owned())
}

#[cfg(test)]
#[path = "daemon/tests.rs"]
mod tests;
