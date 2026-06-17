use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::{FileKind, ScanWarning};

pub(crate) const DEFAULT_SEARCH_LIMIT: usize = 50;
pub(crate) const INDEX_FORMAT_VERSION: u32 = 2;
pub(crate) const IGNORE_POLICY_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchOptions {
    pub include_hidden: bool,
    pub exclude_patterns: Vec<String>,
    pub limit: usize,
}

impl Default for FileSearchOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
            exclude_patterns: Vec::new(),
            limit: DEFAULT_SEARCH_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchOutcome {
    pub root: PathBuf,
    pub matches: Vec<FileSearchMatch>,
    pub skipped: Vec<ScanWarning>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSearchIndexOptions {
    pub include_hidden: bool,
    pub exclude_patterns: Vec<String>,
    pub mode: FileSearchIndexMode,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchIndexStatus {
    pub root: PathBuf,
    pub index_dir: PathBuf,
    pub exists: bool,
    pub include_hidden: bool,
    pub record_count: usize,
    pub index_size_bytes: u64,
    pub built_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub failed_count: usize,
    pub exclude_rules_hash: Option<String>,
    pub failures: Vec<FileSearchIndexFailure>,
}

impl FileSearchIndexStatus {
    pub(crate) fn missing(root: PathBuf, index_dir: PathBuf, include_hidden: bool) -> Self {
        Self {
            root,
            index_dir,
            exists: false,
            include_hidden,
            record_count: 0,
            index_size_bytes: 0,
            built_at_ms: None,
            updated_at_ms: None,
            failed_count: 0,
            exclude_rules_hash: None,
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
