use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryMetadata {
    pub len: u64,
    pub modified: Option<SystemTime>,
    pub readonly: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub path: PathBuf,
    pub name: OsString,
    pub kind: FileKind,
    pub metadata: EntryMetadata,
    pub is_hidden: bool,
    pub is_symlink: bool,
    pub is_broken_symlink: bool,
}

impl DirectoryEntry {
    pub fn new(
        path: PathBuf,
        kind: FileKind,
        metadata: EntryMetadata,
        is_hidden: bool,
        is_symlink: bool,
        is_broken_symlink: bool,
    ) -> Self {
        let name = path
            .file_name()
            .map(OsStr::to_os_string)
            .unwrap_or_else(|| path.as_os_str().to_os_string());

        Self::with_file_name(
            path,
            name,
            kind,
            metadata,
            is_hidden,
            is_symlink,
            is_broken_symlink,
        )
    }

    pub(crate) fn with_file_name(
        path: PathBuf,
        name: OsString,
        kind: FileKind,
        metadata: EntryMetadata,
        is_hidden: bool,
        is_symlink: bool,
        is_broken_symlink: bool,
    ) -> Self {
        Self {
            path,
            name,
            kind,
            metadata,
            is_hidden,
            is_symlink,
            is_broken_symlink,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> &OsStr {
        &self.name
    }
}
