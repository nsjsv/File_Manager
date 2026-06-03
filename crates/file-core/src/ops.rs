use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::FileError;

mod copy;
pub use copy::{
    copy_path, copy_path_with_conflict_strategy, copy_path_with_controls,
    copy_path_with_controls_and_strategy, CopyProgress, FileOperationControls,
    FileOperationRunState, ProgressSender, TransferConflictStrategy,
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

async fn metadata_if_exists(path: &Path) -> io::Result<Option<std::fs::Metadata>> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn already_exists_error() -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, "target already exists")
}

async fn unique_available_path(path: &Path) -> io::Result<PathBuf> {
    if metadata_if_exists(path).await?.is_none() {
        return Ok(path.to_path_buf());
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let name = path
        .file_name()
        .map(std::ffi::OsString::from)
        .unwrap_or_else(|| std::ffi::OsString::from("item"));

    for index in 1..1000 {
        let mut next = name.clone();
        next.push(format!(".copy{index}"));
        let candidate = parent.join(next);
        if metadata_if_exists(&candidate).await?.is_none() {
            return Ok(candidate);
        }
    }

    Ok(path.to_path_buf())
}

pub async fn move_path(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    cancel: CancellationToken,
    progress: Option<ProgressSender>,
) -> Result<(), FileError> {
    move_path_with_controls(from, to, FileOperationControls::running(cancel), progress).await
}

pub async fn move_path_with_conflict_strategy(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    cancel: CancellationToken,
    progress: Option<ProgressSender>,
    conflict_strategy: TransferConflictStrategy,
) -> Result<(), FileError> {
    move_path_with_controls_and_strategy(
        from,
        to,
        FileOperationControls::running(cancel),
        progress,
        conflict_strategy,
    )
    .await
}

pub async fn move_path_with_controls(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    controls: FileOperationControls,
    progress: Option<ProgressSender>,
) -> Result<(), FileError> {
    move_path_with_controls_and_strategy(
        from,
        to,
        controls,
        progress,
        TransferConflictStrategy::Fail,
    )
    .await
}

pub async fn move_path_with_controls_and_strategy(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    controls: FileOperationControls,
    progress: Option<ProgressSender>,
    conflict_strategy: TransferConflictStrategy,
) -> Result<(), FileError> {
    let from = from.as_ref().to_path_buf();
    let to = to.as_ref().to_path_buf();
    let mut controls = controls;
    controls.wait_until_running().await?;

    if from == to {
        return Ok(());
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

    let target_metadata = metadata_if_exists(&to)
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
        move_directory_merge(&from, &to, &mut controls).await?;
        send_move_progress(progress, from, to, total);
        return Ok(());
    }

    let Some(to) =
        prepare_move_target(&from, &to, target_metadata.as_ref(), conflict_strategy).await?
    else {
        send_move_progress(progress, from, to, total);
        return Ok(());
    };

    fs::rename(&from, &to)
        .await
        .map_err(|source| FileError::Move {
            from: from.clone(),
            to: to.clone(),
            source,
        })?;

    send_move_progress(progress, from, to, total);

    Ok(())
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
            remove_move_target(from, to, target_metadata).await?;
            Ok(Some(to.to_path_buf()))
        }
        TransferConflictStrategy::Skip | TransferConflictStrategy::Merge => Ok(None),
        TransferConflictStrategy::KeepBoth => {
            unique_available_path(to)
                .await
                .map(Some)
                .map_err(|source| FileError::Move {
                    from: from.to_path_buf(),
                    to: to.to_path_buf(),
                    source,
                })
        }
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

async fn move_directory_merge(
    from: &Path,
    to: &Path,
    controls: &mut FileOperationControls,
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
            let target_metadata =
                metadata_if_exists(&target_child)
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

            fs::rename(&source_child, &target_child)
                .await
                .map_err(|source| FileError::Move {
                    from: source_child,
                    to: target_child,
                    source,
                })?;
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

        copy_path_with_conflict_strategy(
            &source,
            &target,
            CancellationToken::new(),
            None,
            TransferConflictStrategy::KeepBoth,
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

        move_path_with_conflict_strategy(
            &source,
            &target,
            CancellationToken::new(),
            None,
            TransferConflictStrategy::KeepBoth,
        )
        .await
        .unwrap();

        assert!(metadata_if_exists(&source).await.unwrap().is_none());
        assert_eq!(fs::read(&target).await.unwrap(), b"old");
        assert_eq!(fs::read(&target_copy1).await.unwrap(), b"old copy");
        assert_eq!(fs::read(&target_copy2).await.unwrap(), b"new");
    }
}
