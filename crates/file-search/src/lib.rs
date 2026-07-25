mod config;
mod crawler;
mod daemon;
mod database;
mod error;
mod extractor;
mod fallback;
mod filesystem;
mod logging;
mod model;
mod path_encoding;
mod protocol;
mod runtime_identity;
mod service_runtime;
mod writer;

pub use config::{SearchExcludeRules, SearchIndexConfig};
pub use crawler::{IndexMaintenanceProgress, RebuildStats, SearchIndexer};
pub use daemon::SearchDaemonCore;
pub use database::{
    EntryStageProgress, FileSignature, IndexedEntryStageState, IndexedFile, KnownEntryState,
    SearchDatabase,
};
pub use error::{SearchError, SearchResult};
pub use extractor::{
    extract_content, extract_with_system_command, CommandSpec, ExtractionOutcome, ExtractionStatus,
};
pub use fallback::search_directory_fallback;
pub use model::{
    daemon_build_id, ExtractorCapability, IndexHealth, IndexPhase, IndexStatus,
    IndexedQueryAvailability, MatchSource, SearchCursor, SearchFileKind, SearchFilters, SearchHit,
    SearchProviderFailure, SearchQuery, SearchResultBatch, SearchScope, SearchServiceEvent,
    SearchServicePhase, SearchServiceRequest, SearchServiceStatus, TimeRange, PROTOCOL_VERSION,
};
pub use protocol::{
    default_socket_path, read_service_event, read_service_request, search_via_socket,
    search_via_socket_with_cancellation, serve_bound_search_socket, serve_search_socket,
    serve_search_socket_with_core, serve_search_socket_with_status, shutdown_connected_service,
    shutdown_via_socket, status_via_socket, version_via_socket, write_service_event,
    write_service_request, BoundSearchSocket, SearchSocketService,
};
pub use runtime_identity::{SearchRuntimeIdentity, SEARCH_RUNTIME_IDENTITY_ENV};
pub use service_runtime::SearchServiceRuntime;
pub use writer::IndexWriter;
