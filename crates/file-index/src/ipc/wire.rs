mod conversions;
mod primitive_conversions;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub const INDEX_PROTOCOL_VERSION: u16 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRequest {
    pub version: u16,
    pub index_base_dir: WirePath,
    pub command: IndexRequestCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexRequestCommand {
    ConfigureProfile(WireIndexProfile),
    LoadProfile(String),
    Query(WireSearchQuery),
    Rebuild { profile_id: String, root: WirePath },
    BuildSelectedPaths(WireBuildSelectedPathsRequest),
    Status { profile_id: String, root: WirePath },
    ClearFailures { profile_id: String, root: WirePath },
    RemoveRoot { profile_id: String, root: WirePath },
    Pause,
    Resume,
    DeleteProfile(String),
    SubscribeMaintenance { profile_id: String },
    Ping,
    StartMaintenance { profile_id: String },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexResponse {
    Event(WireIndexServiceEvent),
    Progress(WireFileSearchIndexProgress),
    Error(String),
    ProtocolMismatch { expected: u16, actual: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WirePath {
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireOsString {
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireSearchMode {
    Files,
    Contents,
    Media,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireDirectoryErrorPolicy {
    Abort,
    SkipUnreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireFileSearchIndexMode {
    FullRebuild,
    Incremental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireFileKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireSearchResultSource {
    Files,
    Contents,
    Media,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireMediaSearchKind {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireMediaMetadataScope {
    Off,
    Images,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireContentIndexPolicy {
    pub enabled: bool,
    pub max_file_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireMediaMetadataPolicy {
    pub scope: WireMediaMetadataScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireIndexProfile {
    pub id: String,
    pub roots: Vec<WirePath>,
    pub include_hidden: bool,
    pub exclude_patterns: Vec<String>,
    pub directory_error_policy: WireDirectoryErrorPolicy,
    pub content: WireContentIndexPolicy,
    pub media: WireMediaMetadataPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireSearchQuery {
    pub profile_id: String,
    pub root: WirePath,
    pub text: String,
    pub mode: WireSearchMode,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireBuildSelectedPathsRequest {
    pub profile_id: String,
    pub root: WirePath,
    pub selected_paths: Vec<WirePath>,
    pub mode: WireFileSearchIndexMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireIndexServiceEvent {
    ProfileConfigured(String),
    ProfileLoaded(Option<WireIndexProfile>),
    QueryFinished(WireFileSearchOutcome),
    RebuildFinished(WireFileSearchIndexOutcome),
    StatusLoaded(WireFileSearchIndexStatus),
    FailuresCleared(WireFileSearchIndexStatus),
    RootRemoved(WireFileSearchIndexStatus),
    IncrementalUpdateStarted {
        profile_id: String,
        root: WirePath,
        changed_paths: usize,
    },
    IncrementalUpdateFinished {
        profile_id: String,
        outcome: WireFileSearchIndexOutcome,
    },
    IncrementalUpdateFailed {
        profile_id: String,
        root: WirePath,
        message: String,
    },
    Paused,
    Resumed,
    ProfileDeleted(String),
    WatchStarted {
        profile_id: String,
        root: WirePath,
    },
    WatchFailed {
        profile_id: String,
        root: WirePath,
        message: String,
    },
    Pong {
        daemon_version: String,
    },
    MaintenanceStarted {
        profile_id: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireScanWarning {
    pub path: WirePath,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFileSearchOutcome {
    pub root: WirePath,
    pub matches: Vec<WireFileSearchMatch>,
    pub skipped: Vec<WireScanWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFileSearchMatch {
    pub path: WirePath,
    pub relative_path: WirePath,
    pub name: WireOsString,
    pub kind: WireFileKind,
    pub rank_score: u32,
    pub source: WireSearchResultSource,
    pub snippet: Option<String>,
    pub media: Option<WireMediaSearchMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireMediaSearchMetadata {
    pub media_kind: WireMediaSearchKind,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub codec: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub exif: Vec<WireMediaExifField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireMediaExifField {
    pub tag: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFileSearchIndexOutcome {
    pub root: WirePath,
    pub index_dir: WirePath,
    pub indexed_count: usize,
    pub index_size_bytes: u64,
    pub updated_at_ms: i64,
    pub failed_count: usize,
    pub skipped: Vec<WireScanWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireFileSearchIndexProgress {
    IndexedPaths {
        completed_paths: usize,
        total_paths: usize,
        indexed_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFileSearchIndexStatus {
    pub root: WirePath,
    pub index_dir: WirePath,
    pub exists: bool,
    pub stale: bool,
    pub reason: Option<String>,
    pub include_hidden: bool,
    pub content_index_enabled: bool,
    pub content_max_file_bytes: u64,
    pub media_metadata_scope: WireMediaMetadataScope,
    pub record_count: usize,
    pub index_size_bytes: u64,
    pub built_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub failed_count: usize,
    pub exclude_rules_hash: Option<String>,
    pub extractor_version: Option<u32>,
    pub failures: Vec<WireFileSearchIndexFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFileSearchIndexFailure {
    pub path: WirePath,
    pub message: String,
    pub first_failed_at_ms: i64,
    pub last_failed_at_ms: i64,
    pub retry_count: u32,
}
