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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DirectoryMetadataAvailability {
    Pending,
    #[default]
    Complete,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntryMetadata {
    pub filesystem_availability: DirectoryMetadataAvailability,
    pub identity_names_availability: DirectoryMetadataAvailability,
    pub len: u64,
    pub modified: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub readonly: bool,
    pub owner_name: Option<String>,
    pub group_name: Option<String>,
    pub permissions_mode: Option<u32>,
}

impl EntryMetadata {
    pub(crate) fn pending() -> Self {
        Self {
            filesystem_availability: DirectoryMetadataAvailability::Pending,
            identity_names_availability: DirectoryMetadataAvailability::Pending,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_entry_metadata_leaves_optional_fields_empty() {
        let metadata = EntryMetadata::default();

        assert_eq!(metadata.len, 0);
        assert_eq!(
            metadata.filesystem_availability,
            DirectoryMetadataAvailability::Complete
        );
        assert_eq!(
            metadata.identity_names_availability,
            DirectoryMetadataAvailability::Complete
        );
        assert_eq!(metadata.modified, None);
        assert_eq!(metadata.accessed, None);
        assert_eq!(metadata.created, None);
        assert_eq!(metadata.owner_name, None);
        assert_eq!(metadata.group_name, None);
        assert_eq!(metadata.permissions_mode, None);
        assert!(!metadata.readonly);
    }
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
    pub discovery_index: Option<usize>,
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
            discovery_index: None,
        }
    }

    pub fn with_discovery_index(mut self, index: usize) -> Self {
        self.discovery_index = Some(index);
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> &OsStr {
        &self.name
    }
}
