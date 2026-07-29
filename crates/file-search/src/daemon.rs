use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use tokio_util::sync::CancellationToken;

use crate::crawler::IndexMaintenanceProgress;
use crate::database::SearchDatabase;
use crate::error::{SearchError, SearchResult};
use crate::logging::bounded_search_log_detail;
use crate::managed_search_index::ManagedSearchIndex;
use crate::model::{ExtractorCapability, IndexHealth, IndexPhase, IndexStatus};
use crate::writer::IndexWriter;
use crate::{SearchIndexConfig, SearchIndexer};

mod bounded_paths;
mod memory;
mod watch_budget_patrol;
mod watch_coverage;
mod watch_ingress;
mod work_queue;

#[cfg(test)]
use self::bounded_paths::BoundedPathSet;
use self::memory::release_allocator_idle_pages;
use self::watch_coverage::WatchCoverageHealth;
#[cfg(test)]
use self::watch_ingress::changed_paths_from_watch_event;
use self::watch_ingress::{DaemonWatchIngress, DaemonWatchIngressBootstrap};
#[cfg(test)]
use self::work_queue::{
    dirty_root_retry_delay, PendingDaemonWork, DIRTY_ROOT_QUIET_WINDOW, DIRTY_ROOT_RETRY_BASE,
    DIRTY_ROOT_RETRY_MAX,
};
use self::work_queue::{DaemonWorkQueue, DaemonWorkRequest};

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
    recovery_rebuild_message: Option<String>,
    watch_coverage_health: WatchCoverageHealth,
    visible_indexed_files: u64,
    capabilities: Vec<ExtractorCapability>,
}

impl DaemonLifecycleSnapshot {
    fn starting(visible_indexed_files: u64, recovery_rebuild_message: Option<String>) -> Self {
        Self {
            phase: DaemonLifecyclePhase::Starting,
            maintenance_backend_failure: None,
            recovery_rebuild_message,
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
        self.recovery_rebuild_message = None;
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
            DaemonLifecyclePhase::Complete => IndexPhase::Complete,
            DaemonLifecyclePhase::Failed { message } => IndexPhase::Failed {
                message: message.clone(),
            },
        };
        IndexStatus {
            phase,
            visible_indexed_files: self.visible_indexed_files,
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
            WatchCoverageHealth::Healthy | WatchCoverageHealth::HybridPatrol { .. } => {
                match self.recovery_rebuild_message.as_ref() {
                    Some(message) => IndexHealth::Degraded {
                        message: message.clone(),
                    },
                    None => IndexHealth::Healthy,
                }
            }
            WatchCoverageHealth::Incomplete { message, .. } => IndexHealth::Degraded {
                message: message.clone(),
            },
            WatchCoverageHealth::BackendUnavailable { message } => IndexHealth::Error {
                message: message.clone(),
            },
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

        let managed_index_open = ManagedSearchIndex::new(database_path.clone()).open()?;
        let recovery_rebuild_message = managed_index_open.recovery_notice.as_ref().map(|notice| {
            let message = notice.message();
            let log_message = bounded_search_log_detail(&message);
            tracing::warn!(
                target: "file_search::daemon",
                event = "damaged_index_quarantined",
                detail = %log_message,
                "damaged search index quarantined; rebuilding from filesystem"
            );
            message
        });
        let writer = Arc::new(IndexWriter::spawn(managed_index_open.database));
        let known_count = writer.count()?;
        let lifecycle_snapshot = Arc::new(Mutex::new(DaemonLifecycleSnapshot::starting(
            known_count,
            recovery_rebuild_message,
        )));
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
        {
            let mut runtime_state = self
                .runtime_state
                .lock()
                .expect("search daemon runtime mutex poisoned");
            match runtime_state.phase {
                DaemonRuntimePhase::Maintaining => return Ok(()),
                DaemonRuntimePhase::Stopped => return Err(daemon_core_stopped()),
                DaemonRuntimePhase::Created => {
                    runtime_state.phase = DaemonRuntimePhase::Maintaining;
                }
            }
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

        let mut runtime_state = self
            .runtime_state
            .lock()
            .expect("search daemon runtime mutex poisoned");
        if matches!(runtime_state.phase, DaemonRuntimePhase::Stopped) {
            drop(runtime_state);
            if let Some(watch_ingress) = watch_ingress {
                watch_ingress.shutdown();
            }
            return Err(daemon_core_stopped());
        }
        if let Err(error) = self.enqueue_work(DaemonWorkRequest::StartupCheck) {
            drop(runtime_state);
            if let Some(watch_ingress) = watch_ingress {
                watch_ingress.shutdown();
            }
            return Err(error);
        }
        runtime_state.watch_ingress = watch_ingress;
        drop(runtime_state);

        match maintenance_failure {
            Some(error) => {
                self.record_maintenance_failure(error.to_string());
                Err(error)
            }
            None => Ok(()),
        }
    }

    pub fn current_status(&self) -> IndexStatus {
        self.lifecycle_snapshot
            .lock()
            .expect("search daemon lifecycle mutex poisoned")
            .to_index_status()
    }

    pub fn open_query_reader(&self) -> SearchResult<SearchDatabase> {
        SearchDatabase::open_read_only(&self.database_path)
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
        self.writer.cancel_index_maintenance();
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
                let watch_budget_patrol =
                    matches!(&work_request, DaemonWorkRequest::WatchBudgetPatrol { .. });
                let succeeded = run_index_maintenance(
                    &runtime,
                    Arc::clone(&writer),
                    &config,
                    Arc::clone(&lifecycle_snapshot),
                    &directory_snapshot_epoch,
                    work_request,
                    &crawl_cancellation,
                );
                if watch_budget_patrol {
                    work_queue.finish_watch_budget_patrol();
                }
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
    let high_frequency_maintenance = matches!(
        &work_request,
        DaemonWorkRequest::ChangedPaths { .. } | DaemonWorkRequest::WatchBudgetPatrol { .. }
    );
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
            DaemonWorkRequest::WatchBudgetPatrol { directories } => {
                indexer
                    .patrol_unwatched_directories_with_progress_cancelled(
                        directories,
                        crawl_cancellation,
                        on_progress,
                    )
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
            if high_frequency_maintenance {
                tracing::debug!(
                    target: "file_search::daemon",
                    event = "index_maintenance_completed",
                    maintenance = %work_label,
                    checked = stats.checked,
                    changed = stats.changed,
                    reindexed = stats.reindexed,
                    directories_enumerated = stats.directories_enumerated,
                    database_mutations = stats.database_mutations,
                    content_reads = stats.content_reads,
                    indexed_files = total,
                    "index maintenance completed"
                );
            } else {
                tracing::info!(
                    target: "file_search::daemon",
                    event = "index_maintenance_completed",
                    maintenance = %work_label,
                    checked = stats.checked,
                    changed = stats.changed,
                    reindexed = stats.reindexed,
                    directories_enumerated = stats.directories_enumerated,
                    database_mutations = stats.database_mutations,
                    content_reads = stats.content_reads,
                    indexed_files = total,
                    "index maintenance completed"
                );
            }
            snapshot.finish_watch_cycle(total);
            true
        }
        Err(_) if crawl_cancellation.is_cancelled() => false,
        Err(SearchError::Cancelled) => false,
        Err(error) => {
            let error = error.to_string();
            let log_error = bounded_search_log_detail(&error);
            tracing::error!(
                target: "file_search::daemon",
                event = "index_maintenance_failed",
                maintenance = %work_label,
                error = %log_error,
                "index maintenance failed"
            );
            snapshot.record_error(error);
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
        DaemonWorkRequest::WatchBudgetPatrol { directories } => {
            format!("watch budget patrol ({})", directories.len())
        }
        DaemonWorkRequest::DirtyRootRecovery { roots } => {
            format!("dirty root recovery ({})", roots.len())
        }
        DaemonWorkRequest::Shutdown => "shutdown".to_owned(),
    }
}

fn daemon_core_stopped() -> SearchError {
    SearchError::WorkerFailed("search daemon core is no longer running".to_owned())
}

#[cfg(test)]
#[path = "daemon/tests.rs"]
mod tests;
