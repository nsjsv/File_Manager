use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::transfer_conflict::{
    available_transfer_target_path_candidate, transfer_target_metadata_if_exists,
};
use crate::FileError;

mod batch_rename;
mod copy;
pub use batch_rename::{batch_rename_paths, BatchRenameItem, CompletedBatchRename};
pub use copy::{
    copy_path, copy_path_with_options, CopyProgress, FileOperationControls, FileOperationRunState,
    FileOperationVerification, FileTransferOptions, ProgressSender, TransferConflictStrategy,
};

pub async fn rename_path(
    path: impl AsRef<Path>,
    new_name: impl AsRef<OsStr>,
) -> Result<PathBuf, FileError> {
    let path = path.as_ref().to_path_buf();
    let parent = path
        .parent()
        .ok_or_else(|| FileError::InvalidInput {
            path: path.clone(),
            message: "path has no parent".to_owned(),
        })?
        .to_path_buf();
    let target = parent.join(new_name.as_ref());

    fs::rename(&path, &target)
        .await
        .map_err(|source| FileError::Rename {
            from: path.clone(),
            to: target.clone(),
            source,
        })?;

    Ok(target)
}

pub async fn create_directory(path: impl AsRef<Path>) -> Result<PathBuf, FileError> {
    let path = path.as_ref().to_path_buf();
    fs::create_dir(&path)
        .await
        .map_err(|source| FileError::CreateDirectory {
            path: path.clone(),
            source,
        })?;
    Ok(path)
}

pub async fn create_empty_file(path: impl AsRef<Path>) -> Result<PathBuf, FileError> {
    let path = path.as_ref().to_path_buf();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .map_err(|source| FileError::CreateFile {
            path: path.clone(),
            source,
        })?;
    Ok(path)
}

pub async fn create_file_with_contents(
    path: impl AsRef<Path>,
    contents: impl AsRef<[u8]>,
) -> Result<PathBuf, FileError> {
    let path = path.as_ref().to_path_buf();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .map_err(|source| FileError::CreateFile {
            path: path.clone(),
            source,
        })?;
    file.write_all(contents.as_ref())
        .await
        .map_err(|source| FileError::CreateFile {
            path: path.clone(),
            source,
        })?;
    file.flush().await.map_err(|source| FileError::CreateFile {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

pub async fn trash_path(path: impl AsRef<Path>) -> Result<(), FileError> {
    let path = path.as_ref().to_path_buf();
    let path_for_task = path.clone();
    tokio::task::spawn_blocking(move || trash::delete(&path_for_task))
        .await
        .map_err(|source| FileError::Trash {
            path: path.clone(),
            message: source.to_string(),
        })?
        .map_err(|source| FileError::Trash {
            path,
            message: source.to_string(),
        })
}

pub async fn delete_path_permanently(path: impl AsRef<Path>) -> Result<(), FileError> {
    let path = path.as_ref().to_path_buf();
    let metadata = fs::symlink_metadata(&path)
        .await
        .map_err(|source| FileError::Delete {
            path: path.clone(),
            source,
        })?;
    let outcome = if metadata.file_type().is_dir() {
        fs::remove_dir_all(&path).await
    } else {
        fs::remove_file(&path).await
    };
    outcome.map_err(|source| FileError::Delete { path, source })
}

pub async fn trash_path_with_restore_entry(
    path: impl AsRef<Path>,
) -> Result<Option<crate::TrashRestoreEntry>, FileError> {
    let path = path.as_ref().to_path_buf();
    let before = crate::trash_bin::restore_entries_for_original_path(&path)
        .await
        .unwrap_or_default();
    trash_path(&path).await?;
    let after = crate::trash_bin::restore_entries_for_original_path(&path)
        .await
        .unwrap_or_default();
    Ok(after.into_iter().find(|entry| !before.contains(entry)))
}

fn already_exists_error() -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, "target already exists")
}

pub(super) fn ensure_replace_target_does_not_contain_source_path(
    from: &Path,
    to: &Path,
    target_metadata: &std::fs::Metadata,
) -> Result<(), FileError> {
    if target_metadata.is_dir() && from.starts_with(to) {
        return Err(FileError::InvalidInput {
            path: to.to_path_buf(),
            message: "cannot replace a target directory that contains the source path".to_owned(),
        });
    }

    Ok(())
}

pub async fn move_path(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    cancel: CancellationToken,
    progress: Option<ProgressSender>,
) -> Result<(), FileError> {
    move_path_with_options(
        from,
        to,
        FileTransferOptions::running(cancel).with_optional_progress(progress),
    )
    .await
    .map(|_| ())
}

pub async fn move_path_with_options(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    transfer_options: FileTransferOptions,
) -> Result<Option<PathBuf>, FileError> {
    let from = from.as_ref().to_path_buf();
    let to = to.as_ref().to_path_buf();
    let mut controls = transfer_options.controls;
    let progress = transfer_options.progress;
    let conflict_strategy = transfer_options.conflict_strategy;
    let verification = transfer_options.verification;
    controls.wait_until_running().await?;

    if from == to {
        return Ok(None);
    }

    let source_metadata = fs::metadata(&from)
        .await
        .map_err(|source| FileError::Metadata {
            path: from.clone(),
            source,
        })?;

    let total = if progress.is_some() {
        source_metadata.len()
    } else {
        0
    };

    let target_metadata = transfer_target_metadata_if_exists(&to)
        .await
        .map_err(|source| FileError::Move {
            from: from.clone(),
            to: to.clone(),
            source,
        })?;

    if conflict_strategy == TransferConflictStrategy::Merge
        && source_metadata.is_dir()
        && target_metadata
            .as_ref()
            .is_some_and(std::fs::Metadata::is_dir)
    {
        move_directory_merge(&from, &to, &mut controls, verification).await?;
        send_move_progress(progress, from, to.clone(), total);
        return Ok(Some(to));
    }

    let Some(to) =
        prepare_move_target(&from, &to, target_metadata.as_ref(), conflict_strategy).await?
    else {
        send_move_progress(progress, from, to, total);
        return Ok(None);
    };

    move_prepared_path(
        &from,
        &to,
        &source_metadata,
        &mut controls,
        progress.clone(),
        verification,
    )
    .await?;

    send_move_progress(progress, from, to.clone(), total);

    Ok(Some(to))
}

async fn prepare_move_target(
    from: &Path,
    to: &Path,
    target_metadata: Option<&std::fs::Metadata>,
    conflict_strategy: TransferConflictStrategy,
) -> Result<Option<PathBuf>, FileError> {
    let Some(target_metadata) = target_metadata else {
        return Ok(Some(to.to_path_buf()));
    };

    match conflict_strategy {
        TransferConflictStrategy::Fail => Err(FileError::Move {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source: already_exists_error(),
        }),
        TransferConflictStrategy::Replace => {
            ensure_replace_target_does_not_contain_source_path(from, to, target_metadata)?;
            remove_move_target(from, to, target_metadata).await?;
            Ok(Some(to.to_path_buf()))
        }
        TransferConflictStrategy::Skip | TransferConflictStrategy::Merge => Ok(None),
        TransferConflictStrategy::KeepBoth => available_transfer_target_path_candidate(to)
            .await
            .map(Some)
            .map_err(|source| FileError::Move {
                from: from.to_path_buf(),
                to: to.to_path_buf(),
                source,
            }),
    }
}

async fn remove_move_target(
    from: &Path,
    to: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), FileError> {
    let result = if metadata.is_dir() {
        fs::remove_dir_all(to).await
    } else {
        fs::remove_file(to).await
    };

    result.map_err(|source| FileError::Move {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })
}

async fn move_prepared_path(
    from: &Path,
    to: &Path,
    source_metadata: &std::fs::Metadata,
    controls: &mut FileOperationControls,
    progress: Option<ProgressSender>,
    verification: FileOperationVerification,
) -> Result<(), FileError> {
    match fs::rename(from, to).await {
        Ok(()) => Ok(()),
        Err(source) if is_cross_device_rename_error(&source) => {
            copy_then_remove_source(from, to, source_metadata, controls, progress, verification)
                .await
        }
        Err(source) => Err(FileError::Move {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        }),
    }
}

async fn copy_then_remove_source(
    from: &Path,
    to: &Path,
    source_metadata: &std::fs::Metadata,
    controls: &mut FileOperationControls,
    progress: Option<ProgressSender>,
    verification: FileOperationVerification,
) -> Result<(), FileError> {
    let copy_options = FileTransferOptions::new(controls.clone())
        .with_optional_progress(progress)
        .with_conflict_strategy(TransferConflictStrategy::Fail)
        .with_verification(verification);
    copy_path_with_options(from, to, copy_options).await?;
    remove_moved_source(from, to, source_metadata).await
}

async fn remove_moved_source(
    from: &Path,
    to: &Path,
    source_metadata: &std::fs::Metadata,
) -> Result<(), FileError> {
    let result = if source_metadata.is_dir() {
        fs::remove_dir_all(from).await
    } else {
        fs::remove_file(from).await
    };

    result.map_err(|source| FileError::Move {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn is_cross_device_rename_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(18)
}

#[cfg(not(unix))]
fn is_cross_device_rename_error(_error: &io::Error) -> bool {
    false
}

async fn move_directory_merge(
    from: &Path,
    to: &Path,
    controls: &mut FileOperationControls,
    verification: FileOperationVerification,
) -> Result<(), FileError> {
    if to.starts_with(from) {
        return Err(FileError::InvalidInput {
            path: to.to_path_buf(),
            message: "cannot move a directory into itself".to_owned(),
        });
    }

    let mut pending_directories = vec![(from.to_path_buf(), to.to_path_buf(), false)];
    while let Some((source_directory, target_directory, cleanup)) = pending_directories.pop() {
        controls.wait_until_running().await?;
        if cleanup {
            remove_empty_moved_directory(&source_directory, &target_directory).await?;
            continue;
        }

        pending_directories.push((source_directory.clone(), target_directory.clone(), true));
        let mut entries =
            fs::read_dir(&source_directory)
                .await
                .map_err(|source| FileError::Move {
                    from: source_directory.clone(),
                    to: target_directory.clone(),
                    source,
                })?;

        loop {
            controls.wait_until_running().await?;
            let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|source| FileError::Move {
                    from: source_directory.clone(),
                    to: target_directory.clone(),
                    source,
                })?
            else {
                break;
            };

            let source_child = entry.path();
            let target_child = target_directory.join(entry.file_name());
            let file_type = entry
                .file_type()
                .await
                .map_err(|source| FileError::Metadata {
                    path: source_child.clone(),
                    source,
                })?;
            let source_metadata =
                fs::metadata(&source_child)
                    .await
                    .map_err(|source| FileError::Metadata {
                        path: source_child.clone(),
                        source,
                    })?;
            let target_metadata = transfer_target_metadata_if_exists(&target_child)
                .await
                .map_err(|source| FileError::Move {
                    from: source_child.clone(),
                    to: target_child.clone(),
                    source,
                })?;

            if let Some(target_metadata) = target_metadata {
                if file_type.is_dir() && target_metadata.is_dir() {
                    pending_directories.push((source_child, target_child, false));
                }
                continue;
            }

            move_prepared_path(
                &source_child,
                &target_child,
                &source_metadata,
                controls,
                None,
                verification,
            )
            .await?;
        }
    }

    Ok(())
}

async fn remove_empty_moved_directory(from: &Path, to: &Path) -> Result<(), FileError> {
    match fs::remove_dir(from).await {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::DirectoryNotEmpty | io::ErrorKind::NotFound
            ) =>
        {
            Ok(())
        }
        Err(source) => Err(FileError::Move {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        }),
    }
}

fn send_move_progress(progress: Option<ProgressSender>, from: PathBuf, to: PathBuf, total: u64) {
    if let Some(progress) = progress {
        let _ = progress.send(CopyProgress {
            from,
            to,
            bytes_done: total,
            bytes_total: total,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[tokio::test]
    async fn copy_keep_both_uses_next_available_path() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source.txt");
        let target = directory.path().join("target.txt");
        let target_copy1 = directory.path().join("target.txt.copy1");
        let target_copy2 = directory.path().join("target.txt.copy2");

        fs::write(&source, b"new").await.unwrap();
        fs::write(&target, b"old").await.unwrap();
        fs::write(&target_copy1, b"old copy").await.unwrap();

        copy_path_with_options(
            &source,
            &target,
            FileTransferOptions::running(CancellationToken::new())
                .with_conflict_strategy(TransferConflictStrategy::KeepBoth),
        )
        .await
        .unwrap();

        assert_eq!(fs::read(&target).await.unwrap(), b"old");
        assert_eq!(fs::read(&target_copy1).await.unwrap(), b"old copy");
        assert_eq!(fs::read(&target_copy2).await.unwrap(), b"new");
    }

    #[tokio::test]
    async fn move_keep_both_uses_next_available_path() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source.txt");
        let target = directory.path().join("target.txt");
        let target_copy1 = directory.path().join("target.txt.copy1");
        let target_copy2 = directory.path().join("target.txt.copy2");

        fs::write(&source, b"new").await.unwrap();
        fs::write(&target, b"old").await.unwrap();
        fs::write(&target_copy1, b"old copy").await.unwrap();

        move_path_with_options(
            &source,
            &target,
            FileTransferOptions::running(CancellationToken::new())
                .with_conflict_strategy(TransferConflictStrategy::KeepBoth),
        )
        .await
        .unwrap();

        assert!(fs::metadata(&source).await.is_err());
        assert_eq!(fs::read(&target).await.unwrap(), b"old");
        assert_eq!(fs::read(&target_copy1).await.unwrap(), b"old copy");
        assert_eq!(fs::read(&target_copy2).await.unwrap(), b"new");
    }
}
