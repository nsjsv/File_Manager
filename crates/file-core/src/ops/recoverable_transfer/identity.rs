use std::path::{Path, PathBuf};

use std::os::unix::fs::MetadataExt;
use tokio::fs;

use super::RecoverableTransferError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileObjectKind {
    RegularFile,
    Directory,
    SymbolicLink,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
    pub object_kind: FileObjectKind,
    pub size: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
    pub changed_seconds: i64,
    pub changed_nanoseconds: i64,
    #[serde(with = "super::path_codec::optional")]
    pub symbolic_link_target: Option<PathBuf>,
}

impl FileIdentity {
    pub fn same_object(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.object_kind == other.object_kind
    }
}

pub async fn inspect_file_identity(path: &Path) -> Result<FileIdentity, RecoverableTransferError> {
    let metadata = fs::symlink_metadata(path).await.map_err(|source| {
        RecoverableTransferError::file_system("read metadata for", path, source)
    })?;
    let file_type = metadata.file_type();
    let (object_kind, symbolic_link_target) = if file_type.is_file() {
        (FileObjectKind::RegularFile, None)
    } else if file_type.is_dir() {
        (FileObjectKind::Directory, None)
    } else if file_type.is_symlink() {
        let target = fs::read_link(path).await.map_err(|source| {
            RecoverableTransferError::file_system("read symbolic link", path, source)
        })?;
        (FileObjectKind::SymbolicLink, Some(target))
    } else {
        return Err(RecoverableTransferError::UnsupportedObject {
            path: path.to_path_buf(),
        });
    };

    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        object_kind,
        size: metadata.size(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
        symbolic_link_target,
    })
}
