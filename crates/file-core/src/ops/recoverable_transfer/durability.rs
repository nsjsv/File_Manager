use std::path::{Path, PathBuf};

use super::RecoverableTransferError;

pub fn sync_tree_blocking(root: &Path) -> Result<(), RecoverableTransferError> {
    let mut pending = vec![(root.to_path_buf(), false)];
    while let Some((path, children_synced)) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&path).map_err(|source| {
            RecoverableTransferError::file_system("read staged metadata for", &path, source)
        })?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_file() {
            sync_open_path(&path, "sync staged file")?;
            continue;
        }
        if !file_type.is_dir() {
            return Err(RecoverableTransferError::UnsupportedObject { path });
        }
        if children_synced {
            sync_open_path(&path, "sync staged directory")?;
            continue;
        }

        pending.push((path.clone(), true));
        let entries = std::fs::read_dir(&path).map_err(|source| {
            RecoverableTransferError::file_system("read staged directory", &path, source)
        })?;
        let mut children = entries
            .map(|entry| {
                entry.map(|entry| entry.path()).map_err(|source| {
                    RecoverableTransferError::file_system(
                        "read staged directory entry",
                        &path,
                        source,
                    )
                })
            })
            .collect::<Result<Vec<PathBuf>, RecoverableTransferError>>()?;
        children.sort();
        for child in children.into_iter().rev() {
            pending.push((child, false));
        }
    }
    Ok(())
}

pub fn sync_parent_blocking(path: &Path) -> Result<(), RecoverableTransferError> {
    let parent = path
        .parent()
        .ok_or_else(|| RecoverableTransferError::ArtifactOwnership {
            path: path.to_path_buf(),
            reason: "path has no parent directory to sync".to_owned(),
        })?;
    sync_open_path(parent, "sync parent directory")
}

fn sync_open_path(path: &Path, action: &'static str) -> Result<(), RecoverableTransferError> {
    std::fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|source| RecoverableTransferError::file_system(action, path, source))
}
