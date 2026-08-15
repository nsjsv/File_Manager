use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::{SearchPathPreferences, VersionedSearchPathPreferences};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query_id: u64,
    pub terms: String,
    pub text_scope: SearchTextScope,
    pub scope: SearchScope,
    pub recursive: bool,
    pub filters: SearchFilters,
    pub limit: usize,
    pub cursor: Option<SearchCursor>,
}

impl SearchQuery {
    pub fn global(query_id: u64, terms: impl Into<String>) -> Self {
        Self {
            query_id,
            terms: terms.into(),
            text_scope: SearchTextScope::NameAndContent,
            scope: SearchScope::Global,
            recursive: true,
            filters: SearchFilters::default(),
            limit: 50,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchTextScope {
    NameAndContent,
    NameOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchScope {
    Global,
    Directory(#[serde(with = "crate::path_encoding::serde_path")] PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SearchFilters {
    pub entry_type_rules: Vec<SearchEntryTypeRule>,
    pub modified: Option<TimeRange>,
    pub accessed: Option<TimeRange>,
    pub created: Option<TimeRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchEntryTypeRule {
    Kind(SearchFileKind),
    Mime(MimePattern),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MimePattern {
    Exact(String),
    Prefix(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCursor {
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResultBatch {
    pub query_id: u64,
    pub hits: Vec<SearchHit>,
    pub next_cursor: Option<SearchCursor>,
    pub finished: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    #[serde(with = "crate::path_encoding::serde_path")]
    pub path: PathBuf,
    pub display_name: String,
    pub kind: SearchFileKind,
    pub size: u64,
    pub modified_ms: Option<i64>,
    pub accessed_ms: Option<i64>,
    pub created_ms: Option<i64>,
    pub rank: f64,
    pub snippet: Option<String>,
    pub match_source: MatchSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchFileKind {
    Directory,
    File,
    Symlink,
    Other,
}

impl SearchFileKind {
    pub fn as_storage_value(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::Symlink => "symlink",
            Self::Other => "other",
        }
    }

    pub fn from_storage_value(value: &str) -> Self {
        match value {
            "directory" => Self::Directory,
            "file" => Self::File,
            "symlink" => Self::Symlink,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchSource {
    Name,
    Content,
    Metadata,
}

/// 索引阶段、查询可见计数与维护健康度必须正交，避免瞬时状态互相覆盖。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStatus {
    pub phase: IndexPhase,
    pub visible_indexed_files: u64,
    pub health: IndexHealth,
    pub capabilities: Vec<ExtractorCapability>,
    pub path_configuration: SearchPathConfigurationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPathConfigurationStatus {
    pub desired_revision: u64,
    pub effective_revision: u64,
    pub effective_preferences: SearchPathPreferences,
    pub phase: SearchPathConfigurationPhase,
    pub roots: Vec<SearchRootStatus>,
}

impl Default for SearchPathConfigurationStatus {
    fn default() -> Self {
        Self {
            desired_revision: 0,
            effective_revision: 0,
            effective_preferences: SearchPathPreferences::default(),
            phase: SearchPathConfigurationPhase::Ready,
            roots: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchPathConfigurationPhase {
    Ready,
    Applying,
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRootStatus {
    #[serde(with = "crate::path_encoding::serde_path")]
    pub path: PathBuf,
    pub availability: SearchRootAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchRootAvailability {
    Available,
    Unavailable { message: String },
    MountChanged { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexPhase {
    Starting,
    Checking {
        checked_entries: u64,
        changed_entries: u64,
    },
    Crawling {
        scanned_entries: u64,
        #[serde(with = "crate::path_encoding::serde_path")]
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

/// 持续监听与维护链路的健康度；索引任务自身失败由 `IndexPhase::Failed` 承载。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexHealth {
    Healthy,
    Degraded { message: String },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractorCapability {
    pub extension: String,
    pub tool: String,
    pub available: bool,
}

/// 服务阶段、查询可用性和索引进度必须正交，避免客户端用索引进度猜测端点是否可查询。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchServiceStatus {
    pub phase: SearchServicePhase,
    pub query_availability: IndexedQueryAvailability,
    pub index_status: Option<IndexStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchServicePhase {
    Starting,
    Ready,
    Degraded { message: String },
    Failed { message: String },
    ShuttingDown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexedQueryAvailability {
    Unavailable { message: String },
    Available,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum SearchProviderFailure {
    #[error("search provider unavailable: {message}")]
    Unavailable { message: String },
    #[error("invalid search query: {message}")]
    InvalidQuery { message: String },
    #[error("search provider failed: {message}")]
    Fatal { message: String },
}

/// 协议 payload 含义变化时提升版本，让新客户端能退休仍在运行的旧 daemon。
pub const PROTOCOL_VERSION: u32 = 10;

/// app 更新后用构建标识识别遗留 daemon；本任务保留现有包版本策略。
pub fn daemon_build_id() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchServiceRequest {
    Status,
    GetPathConfiguration,
    ConfigurePathPreferences {
        expected_revision: u64,
        preferences: SearchPathPreferences,
    },
    Search(SearchQuery),
    Cancel {
        query_id: u64,
    },
    /// 查询 daemon 的协议版本和构建标识。
    Version,
    /// 请求 daemon 完成待处理写入后退出，用于退休不兼容的旧进程。
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SearchServiceEvent {
    Status(SearchServiceStatus),
    PathConfiguration {
        configuration: VersionedSearchPathPreferences,
        status: SearchPathConfigurationStatus,
    },
    PathConfigurationFailed {
        failure: SearchProviderFailure,
        status: Option<SearchPathConfigurationStatus>,
    },
    Results(SearchResultBatch),
    SearchFailed {
        query_id: u64,
        failure: SearchProviderFailure,
    },
    Cancelled {
        query_id: u64,
    },
    /// `Version` 的响应同时携带协议版本和 daemon 构建标识。
    Version {
        protocol: u32,
        build: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        IndexHealth, IndexPhase, IndexStatus, IndexedQueryAvailability, SearchServiceEvent,
        SearchServicePhase, SearchServiceStatus,
    };

    #[test]
    fn service_phase_and_query_availability_round_trip_independently() {
        let event = SearchServiceEvent::Status(SearchServiceStatus {
            phase: SearchServicePhase::Degraded {
                message: "watcher unavailable".to_owned(),
            },
            query_availability: IndexedQueryAvailability::Available,
            index_status: Some(IndexStatus {
                phase: IndexPhase::Checking {
                    checked_entries: 128,
                    changed_entries: 3,
                },
                visible_indexed_files: 64,
                health: IndexHealth::Degraded {
                    message: "watcher unavailable".to_owned(),
                },
                capabilities: Vec::new(),
                path_configuration: Default::default(),
            }),
        });

        let encoded = serde_json::to_vec(&event).unwrap();
        let decoded: SearchServiceEvent = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(decoded, event);
    }
}
