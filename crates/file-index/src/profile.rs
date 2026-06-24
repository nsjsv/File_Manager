use std::path::PathBuf;

mod control_store;
pub use control_store::ProfileStore;

use crate::search::{DirectoryErrorPolicy, FileSearchIndexFailure, SearchIndexFileRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Files,
    Contents,
    Media,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentIndexPolicy {
    pub enabled: bool,
    pub max_file_bytes: u64,
}

impl Default for ContentIndexPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_file_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaMetadataPolicy {
    pub scope: MediaMetadataScope,
}

impl Default for MediaMetadataPolicy {
    fn default() -> Self {
        Self {
            scope: MediaMetadataScope::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaMetadataScope {
    Off,
    Images,
    All,
}

impl MediaMetadataScope {
    pub fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "images" => Some(Self::Images),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    pub fn config_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Images => "images",
            Self::All => "all",
        }
    }

    pub fn includes_media(self) -> bool {
        self != Self::Off
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexProfile {
    pub id: String,
    pub roots: Vec<PathBuf>,
    pub include_hidden: bool,
    pub exclude_patterns: Vec<String>,
    pub directory_error_policy: DirectoryErrorPolicy,
    pub content: ContentIndexPolicy,
    pub media: MediaMetadataPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexTaskPhase {
    Queued,
    Running,
    Paused,
    Finished,
    Failed,
    Deleted,
}

impl IndexTaskPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::Deleted => "deleted",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "finished" => Some(Self::Finished),
            "failed" => Some(Self::Failed),
            "deleted" => Some(Self::Deleted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexTaskStatus {
    pub profile_id: String,
    pub root: Option<PathBuf>,
    pub phase: IndexTaskPhase,
    pub message: Option<String>,
    pub updated_at_ms: i64,
    pub extractor_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRootSnapshot {
    pub profile_id: String,
    pub root: PathBuf,
    pub records: Vec<SearchIndexFileRecord>,
    pub failures: Vec<FileSearchIndexFailure>,
}

impl IndexProfile {
    pub fn new(id: impl Into<String>, roots: Vec<PathBuf>) -> Self {
        Self {
            id: id.into(),
            roots: unique_index_roots(roots),
            include_hidden: false,
            exclude_patterns: Vec::new(),
            directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
            content: ContentIndexPolicy::default(),
            media: MediaMetadataPolicy::default(),
        }
    }

    pub fn normalize_roots(&mut self) {
        self.roots = unique_index_roots(std::mem::take(&mut self.roots));
    }
}

fn unique_index_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique = Vec::new();
    for root in roots {
        if unique.contains(&root) {
            continue;
        }
        unique.push(root);
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_profile_removes_duplicate_roots_without_collapsing_explicit_children() {
        let profile = IndexProfile::new(
            "main",
            vec![
                PathBuf::from("/workspace/project/src"),
                PathBuf::from("/workspace/project"),
                PathBuf::from("/workspace/project"),
                PathBuf::from("/workspace/archive"),
            ],
        );

        assert_eq!(
            profile.roots,
            vec![
                PathBuf::from("/workspace/project/src"),
                PathBuf::from("/workspace/project"),
                PathBuf::from("/workspace/archive"),
            ]
        );
    }
}
