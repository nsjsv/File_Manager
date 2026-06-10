use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::{FileKind, ScanWarning};

pub(crate) const DEFAULT_SEARCH_LIMIT: usize = 50;
pub(crate) const INDEX_FORMAT_VERSION: u32 = 1;
pub(crate) const IGNORE_POLICY_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchOptions {
    pub include_hidden: bool,
    pub limit: usize,
}

impl Default for FileSearchOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchIndexOutcome {
    pub root: PathBuf,
    pub index_dir: PathBuf,
    pub indexed_count: usize,
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
