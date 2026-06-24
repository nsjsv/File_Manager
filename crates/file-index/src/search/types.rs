use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use file_core::{FileKind, ScanWarning};

use crate::profile::{MediaMetadataScope, SearchMode};

pub(crate) const DEFAULT_SEARCH_LIMIT: usize = 50;
pub(crate) const EXTRACTOR_VERSION: u32 = 2;
pub(crate) const INDEX_FORMAT_VERSION: u32 = 7;
pub(crate) const IGNORE_POLICY_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchOptions {
    pub include_hidden: bool,
    pub exclude_patterns: Vec<String>,
    pub directory_error_policy: DirectoryErrorPolicy,
    pub limit: usize,
    pub mode: SearchMode,
    pub content_max_file_bytes: u64,
    pub content_index_enabled: bool,
    pub media_metadata_scope: MediaMetadataScope,
}

impl Default for FileSearchOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
            exclude_patterns: Vec::new(),
            directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
            limit: DEFAULT_SEARCH_LIMIT,
            mode: SearchMode::Files,
            content_max_file_bytes: 16 * 1024 * 1024,
            content_index_enabled: false,
            media_metadata_scope: MediaMetadataScope::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchOutcome {
    pub root: PathBuf,
    pub matches: Vec<FileSearchMatch>,
    pub skipped: Vec<ScanWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchIndexOptions {
    pub include_hidden: bool,
    pub exclude_patterns: Vec<String>,
    pub directory_error_policy: DirectoryErrorPolicy,
    pub excluded_index_dir: Option<PathBuf>,
    pub mode: FileSearchIndexMode,
    pub content_index_enabled: bool,
    pub content_max_file_bytes: u64,
    pub media_metadata_scope: MediaMetadataScope,
}

impl Default for FileSearchIndexOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
            exclude_patterns: Vec::new(),
            directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
            excluded_index_dir: None,
            mode: FileSearchIndexMode::FullRebuild,
            content_index_enabled: false,
            content_max_file_bytes: 16 * 1024 * 1024,
            media_metadata_scope: MediaMetadataScope::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum DirectoryErrorPolicy {
    Abort,
    #[default]
    SkipUnreadable,
}

impl DirectoryErrorPolicy {
    pub fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "abort" => Some(Self::Abort),
            "skip_unreadable" => Some(Self::SkipUnreadable),
            _ => None,
        }
    }

    pub fn config_value(self) -> &'static str {
        match self {
            Self::Abort => "abort",
            Self::SkipUnreadable => "skip_unreadable",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FileSearchIndexMode {
    #[default]
    FullRebuild,
    Incremental,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchIndexOutcome {
    pub root: PathBuf,
    pub index_dir: PathBuf,
    pub indexed_count: usize,
    pub index_size_bytes: u64,
    pub updated_at_ms: i64,
    pub failed_count: usize,
    pub skipped: Vec<ScanWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSearchIndexProgress {
    IndexedPaths {
        completed_paths: usize,
        total_paths: usize,
        indexed_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchMatch {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub name: OsString,
    pub kind: FileKind,
    pub rank_score: u32,
    pub source: SearchResultSource,
    pub snippet: Option<String>,
    pub media: Option<MediaSearchMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchResultSource {
    Files,
    Contents,
    Media,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSearchMetadata {
    pub media_kind: MediaSearchKind,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub codec: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub exif: Vec<MediaExifField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaExifField {
    pub tag: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSearchKind {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchIndexStatus {
    pub root: PathBuf,
    pub index_dir: PathBuf,
    pub exists: bool,
    pub stale: bool,
    pub reason: Option<String>,
    pub include_hidden: bool,
    pub content_index_enabled: bool,
    pub content_max_file_bytes: u64,
    pub media_metadata_scope: MediaMetadataScope,
    pub record_count: usize,
    pub index_size_bytes: u64,
    pub built_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub failed_count: usize,
    pub exclude_rules_hash: Option<String>,
    pub extractor_version: Option<u32>,
    pub failures: Vec<FileSearchIndexFailure>,
}

impl FileSearchIndexStatus {
    pub(crate) fn missing(
        root: PathBuf,
        index_dir: PathBuf,
        include_hidden: bool,
        content_index_enabled: bool,
        content_max_file_bytes: u64,
        media_metadata_scope: MediaMetadataScope,
    ) -> Self {
        Self {
            root,
            index_dir,
            exists: false,
            stale: false,
            reason: None,
            include_hidden,
            content_index_enabled,
            content_max_file_bytes,
            media_metadata_scope,
            record_count: 0,
            index_size_bytes: 0,
            built_at_ms: None,
            updated_at_ms: None,
            failed_count: 0,
            exclude_rules_hash: None,
            extractor_version: None,
            failures: Vec::new(),
        }
    }

    pub(crate) fn stale(
        root: PathBuf,
        index_dir: PathBuf,
        include_hidden: bool,
        content_index_enabled: bool,
        content_max_file_bytes: u64,
        media_metadata_scope: MediaMetadataScope,
        reason: String,
    ) -> Self {
        Self {
            root,
            index_dir,
            exists: true,
            stale: true,
            reason: Some(reason),
            include_hidden,
            content_index_enabled,
            content_max_file_bytes,
            media_metadata_scope,
            record_count: 0,
            index_size_bytes: 0,
            built_at_ms: None,
            updated_at_ms: None,
            failed_count: 0,
            exclude_rules_hash: None,
            extractor_version: None,
            failures: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchIndexFailure {
    pub path: PathBuf,
    pub message: String,
    pub first_failed_at_ms: i64,
    pub last_failed_at_ms: i64,
    pub retry_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchIndexFileRecord {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub kind: FileKind,
    pub mtime_ms: Option<i64>,
    pub size_bytes: Option<u64>,
}

impl FileSearchMatch {
    pub fn name(&self) -> &OsStr {
        &self.name
    }
}

pub(crate) fn file_kind_key(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Directory => "directory",
        FileKind::File => "file",
        FileKind::Symlink => "symlink",
        FileKind::Other => "other",
    }
}

pub(crate) fn file_kind_from_key(key: &str) -> Option<FileKind> {
    match key {
        "directory" => Some(FileKind::Directory),
        "file" => Some(FileKind::File),
        "symlink" => Some(FileKind::Symlink),
        "other" => Some(FileKind::Other),
        _ => None,
    }
}
