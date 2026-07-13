use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::daemon::SearchDaemonCore;
use crate::error::{SearchError, SearchResult};
use crate::model::{
    IndexedQueryAvailability, SearchProviderFailure, SearchQuery, SearchResultBatch,
    SearchServicePhase, SearchServiceStatus,
};
use crate::protocol::SearchSocketService;
use crate::SearchIndexConfig;

struct SearchServiceRuntimeState {
    phase: SearchServicePhase,
    query_core: Option<Arc<SearchDaemonCore>>,
    initializer_started: bool,
}

impl Default for SearchServiceRuntimeState {
    fn default() -> Self {
        Self {
            phase: SearchServicePhase::Starting,
            query_core: None,
            initializer_started: false,
        }
    }
}

/// socket endpoint 后唯一的服务阶段和 query core 所有者。
pub struct SearchServiceRuntime {
    state: Mutex<SearchServiceRuntimeState>,
    initializer_join: Mutex<Option<thread::JoinHandle<()>>>,
    lifecycle_serialization: Mutex<()>,
}

impl SearchServiceRuntime {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SearchServiceRuntimeState::default()),
            initializer_join: Mutex::new(None),
            lifecycle_serialization: Mutex::new(()),
        }
    }

    pub fn start_in_background(
        self: &Arc<Self>,
        database_path: PathBuf,
        config: SearchIndexConfig,
    ) {
        self.spawn_initializer(move |runtime| {
            runtime.initialize_core(database_path, config);
        });
    }

    pub fn shutdown(&self) -> SearchResult<()> {
        let _lifecycle = self
            .lifecycle_serialization
            .lock()
            .expect("search service lifecycle mutex poisoned");
        {
            let mut state = self
                .state
                .lock()
                .expect("search service state mutex poisoned");
            state.phase = SearchServicePhase::ShuttingDown;
        }

        let initializer_outcome = self.join_initializer();
        let query_core = self
            .state
            .lock()
            .expect("search service state mutex poisoned")
            .query_core
            .take();
        let core_outcome = query_core.map_or(Ok(()), |core| core.shutdown());

        initializer_outcome.and(core_outcome)
    }

    fn spawn_initializer(self: &Arc<Self>, initialize: impl FnOnce(Arc<Self>) + Send + 'static) {
        let _lifecycle = self
            .lifecycle_serialization
            .lock()
            .expect("search service lifecycle mutex poisoned");
        {
            let mut state = self
                .state
                .lock()
                .expect("search service state mutex poisoned");
            if state.initializer_started || matches!(state.phase, SearchServicePhase::ShuttingDown)
            {
                return;
            }
            state.initializer_started = true;
        }

        let runtime = Arc::clone(self);
        match thread::Builder::new()
            .name("file-search-service-initializer".to_owned())
            .spawn(move || {
                let panic_outcome = panic::catch_unwind(AssertUnwindSafe(|| {
                    initialize(Arc::clone(&runtime));
                }));
                if let Err(payload) = panic_outcome {
                    runtime.record_initializer_panic(payload);
                }
            }) {
            Ok(join) => {
                *self
                    .initializer_join
                    .lock()
                    .expect("search service initializer mutex poisoned") = Some(join);
            }
            Err(error) => self.record_initialization_failure(format!(
                "could not spawn search service initializer: {error}"
            )),
        }
    }

    fn initialize_core(self: &Arc<Self>, database_path: PathBuf, config: SearchIndexConfig) {
        let daemon_core = match SearchDaemonCore::new(database_path, config) {
            Ok(daemon_core) => Arc::new(daemon_core),
            Err(error) => {
                self.record_initialization_failure(error.to_string());
                return;
            }
        };

        if !self.publish_query_core(Arc::clone(&daemon_core)) {
            let _ = daemon_core.shutdown();
            return;
        }

        let _ = daemon_core.start_index_maintenance();
    }

    fn publish_query_core(&self, daemon_core: Arc<SearchDaemonCore>) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("search service state mutex poisoned");
        if !matches!(state.phase, SearchServicePhase::Starting) {
            return false;
        }
        state.query_core = Some(daemon_core);
        state.phase = SearchServicePhase::Ready;
        true
    }

    fn record_initialization_failure(&self, message: String) {
        let mut state = self
            .state
            .lock()
            .expect("search service state mutex poisoned");
        if !matches!(state.phase, SearchServicePhase::Starting) {
            return;
        }
        state.phase = if state.query_core.is_some() {
            SearchServicePhase::Degraded { message }
        } else {
            SearchServicePhase::Failed { message }
        };
    }

    fn record_initializer_panic(&self, payload: Box<dyn Any + Send>) {
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic payload".to_owned());
        self.record_initialization_failure(format!(
            "search service initializer panicked: {message}"
        ));
    }

    fn join_initializer(&self) -> SearchResult<()> {
        let initializer_join = self
            .initializer_join
            .lock()
            .expect("search service initializer mutex poisoned")
            .take();
        let Some(initializer_join) = initializer_join else {
            return Ok(());
        };
        initializer_join.join().map_err(|_| {
            SearchError::WorkerFailed("search service initializer thread panicked".to_owned())
        })
    }

    fn status_snapshot(&self) -> SearchServiceStatus {
        let state = self
            .state
            .lock()
            .expect("search service state mutex poisoned");
        let index_status = state
            .query_core
            .as_ref()
            .map(|query_core| query_core.current_status());
        SearchServiceStatus {
            phase: state.phase.clone(),
            query_availability: query_availability(&state.phase, state.query_core.is_some()),
            index_status,
        }
    }
}

impl Default for SearchServiceRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchSocketService for SearchServiceRuntime {
    fn status(&self) -> SearchServiceStatus {
        self.status_snapshot()
    }

    fn search(&self, query: &SearchQuery) -> Result<SearchResultBatch, SearchProviderFailure> {
        let query_core = {
            let state = self
                .state
                .lock()
                .expect("search service state mutex poisoned");
            if matches!(state.phase, SearchServicePhase::ShuttingDown) {
                return Err(SearchProviderFailure::Unavailable {
                    message: "search service is shutting down".to_owned(),
                });
            }
            state
                .query_core
                .clone()
                .ok_or_else(|| SearchProviderFailure::Unavailable {
                    message: unavailable_message(&state.phase),
                })?
        };

        query_core.search(query).map_err(provider_failure)
    }
}

fn query_availability(
    phase: &SearchServicePhase,
    query_core_available: bool,
) -> IndexedQueryAvailability {
    if query_core_available && !matches!(phase, SearchServicePhase::ShuttingDown) {
        IndexedQueryAvailability::Available
    } else {
        IndexedQueryAvailability::Unavailable {
            message: unavailable_message(phase),
        }
    }
}

fn unavailable_message(phase: &SearchServicePhase) -> String {
    match phase {
        SearchServicePhase::Starting => "search index is still starting".to_owned(),
        SearchServicePhase::Failed { message } | SearchServicePhase::Degraded { message } => {
            message.clone()
        }
        SearchServicePhase::ShuttingDown => "search service is shutting down".to_owned(),
        SearchServicePhase::Ready => "search query core is unavailable".to_owned(),
    }
}

fn provider_failure(error: SearchError) -> SearchProviderFailure {
    match error {
        SearchError::InvalidQuery(message) => SearchProviderFailure::InvalidQuery { message },
        SearchError::SearchFailed { failure, .. } => failure,
        error => SearchProviderFailure::Fatal {
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    use tempfile::tempdir;

    use crate::database::{
        EntryStageProgress, IndexedEntryStageState, IndexedFile, SearchDatabase,
    };
    use crate::extractor::ExtractionStatus;
    use crate::model::{
        IndexHealth, IndexPhase, IndexedQueryAvailability, SearchFileKind, SearchQuery,
        SearchServicePhase,
    };
    use crate::protocol::{
        serve_bound_search_socket, shutdown_via_socket, status_via_socket, version_via_socket,
        BoundSearchSocket, SearchSocketService,
    };

    use super::*;

    fn empty_root_config() -> SearchIndexConfig {
        SearchIndexConfig {
            roots: Vec::new(),
            ..SearchIndexConfig::default()
        }
    }

    fn indexed_file(path: PathBuf) -> IndexedFile {
        IndexedFile {
            parent_path: path.parent().unwrap().to_path_buf(),
            display_name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path,
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
            inode: Some(1),
            mtime_ns: Some(1),
            ctime_ns: Some(1),
        }
    }

    #[test]
    fn new_runtime_reports_starting_without_query_core() {
        let runtime = SearchServiceRuntime::new();

        assert_eq!(
            runtime.status(),
            SearchServiceStatus {
                phase: SearchServicePhase::Starting,
                query_availability: IndexedQueryAvailability::Unavailable {
                    message: "search index is still starting".to_owned(),
                },
                index_status: None,
            }
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bound_endpoint_stays_responsive_during_slow_initialization() {
        let directory = tempdir().unwrap();
        let socket_path = directory.path().join("search.sock");
        let database_path = directory.path().join("search.sqlite");
        let bound_socket = BoundSearchSocket::bind(socket_path.clone()).unwrap();
        let runtime = Arc::new(SearchServiceRuntime::new());
        let initializer_entered = Arc::new(Barrier::new(2));
        let initializer_release = Arc::new(Barrier::new(2));

        runtime.spawn_initializer({
            let initializer_entered = Arc::clone(&initializer_entered);
            let initializer_release = Arc::clone(&initializer_release);
            move |runtime| {
                initializer_entered.wait();
                initializer_release.wait();
                runtime.initialize_core(database_path, empty_root_config());
            }
        });
        initializer_entered.wait();

        let socket_service: Arc<dyn SearchSocketService> = runtime.clone();
        let shutdown_runtime = Arc::clone(&runtime);
        let server = tokio::spawn(async move {
            serve_bound_search_socket(bound_socket, socket_service, move || {
                let shutdown_runtime = Arc::clone(&shutdown_runtime);
                async move { shutdown_runtime.shutdown() }
            })
            .await
        });

        let (protocol, _) =
            tokio::time::timeout(Duration::from_secs(1), version_via_socket(&socket_path))
                .await
                .expect("Version should not wait for initialization")
                .unwrap();
        assert_eq!(protocol, crate::PROTOCOL_VERSION);
        assert_eq!(
            status_via_socket(&socket_path).await.unwrap().phase,
            SearchServicePhase::Starting
        );

        tokio::time::sleep(Duration::from_millis(2_600)).await;
        assert_eq!(
            status_via_socket(&socket_path).await.unwrap().phase,
            SearchServicePhase::Starting
        );

        let release_initializer = {
            let runtime = Arc::clone(&runtime);
            let initializer_release = Arc::clone(&initializer_release);
            std::thread::spawn(move || {
                while !matches!(runtime.status().phase, SearchServicePhase::ShuttingDown) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                initializer_release.wait();
            })
        };
        tokio::time::timeout(Duration::from_secs(2), shutdown_via_socket(&socket_path))
            .await
            .expect("Shutdown should join the released initializer")
            .unwrap();
        release_initializer.join().unwrap();
        server.await.unwrap().unwrap();
    }

    #[test]
    fn database_failure_keeps_runtime_online_as_failed() {
        let directory = tempdir().unwrap();
        let runtime = Arc::new(SearchServiceRuntime::new());

        runtime.initialize_core(directory.path().to_path_buf(), empty_root_config());

        let status = runtime.status();
        assert!(matches!(status.phase, SearchServicePhase::Failed { .. }));
        assert!(matches!(
            status.query_availability,
            IndexedQueryAvailability::Unavailable { .. }
        ));
        assert!(status.index_status.is_none());
    }

    #[test]
    fn published_old_snapshot_is_queryable_before_maintenance_finishes() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("search.sqlite");
        let file_path = directory.path().join("note.txt");
        let database = SearchDatabase::open(&database_path).unwrap();
        database.upsert_file(&indexed_file(file_path)).unwrap();
        drop(database);

        let runtime = Arc::new(SearchServiceRuntime::new());
        let daemon_core =
            Arc::new(SearchDaemonCore::new(database_path, empty_root_config()).unwrap());
        assert!(runtime.publish_query_core(daemon_core));

        let status = runtime.status();
        assert_eq!(status.phase, SearchServicePhase::Ready);
        assert_eq!(
            status.query_availability,
            IndexedQueryAvailability::Available
        );
        let batch = runtime.search(&SearchQuery::global(7, "needle")).unwrap();
        assert_eq!(batch.hits.len(), 1);
        assert_eq!(batch.hits[0].display_name, "note.txt");

        runtime.shutdown().unwrap();
    }

    #[test]
    fn maintenance_failure_keeps_query_phase_ready() {
        let directory = tempdir().unwrap();
        let runtime = Arc::new(SearchServiceRuntime::new());
        let daemon_core = Arc::new(
            SearchDaemonCore::new(directory.path().join("search.sqlite"), empty_root_config())
                .unwrap(),
        );
        assert!(runtime.publish_query_core(Arc::clone(&daemon_core)));

        daemon_core.record_maintenance_failure("watcher stopped".to_owned());

        let status = runtime.status();
        assert_eq!(status.phase, SearchServicePhase::Ready);
        assert_eq!(
            status.query_availability,
            IndexedQueryAvailability::Available
        );
        let index_status = status.index_status.unwrap();
        assert_eq!(index_status.phase, IndexPhase::Starting);
        assert!(matches!(index_status.health, IndexHealth::Error { .. }));
        assert!(runtime.search(&SearchQuery::global(9, "")).is_ok());

        runtime.shutdown().unwrap();
    }

    #[test]
    fn shutdown_joins_blocked_initializer_without_late_revival() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("search.sqlite");
        let runtime = Arc::new(SearchServiceRuntime::new());
        let initializer_entered = Arc::new(Barrier::new(2));
        let initializer_release = Arc::new(Barrier::new(2));

        runtime.spawn_initializer({
            let initializer_entered = Arc::clone(&initializer_entered);
            let initializer_release = Arc::clone(&initializer_release);
            move |runtime| {
                initializer_entered.wait();
                initializer_release.wait();
                runtime.initialize_core(database_path, empty_root_config());
            }
        });
        initializer_entered.wait();

        let shutdown = {
            let runtime = Arc::clone(&runtime);
            std::thread::spawn(move || runtime.shutdown())
        };
        let mut shutting_down_seen = false;
        for _ in 0..100 {
            if matches!(runtime.status().phase, SearchServicePhase::ShuttingDown) {
                shutting_down_seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        initializer_release.wait();
        shutdown.join().unwrap().unwrap();

        assert!(shutting_down_seen);
        let status = runtime.status();
        assert_eq!(status.phase, SearchServicePhase::ShuttingDown);
        assert!(matches!(
            status.query_availability,
            IndexedQueryAvailability::Unavailable { .. }
        ));
        assert!(status.index_status.is_none());
    }
}
