use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::transfer_conflict::{
    available_transfer_target_path_candidate, transfer_target_metadata_if_exists,
};
use crate::FileError;

use super::{already_exists_error, ensure_replace_target_does_not_contain_source_path};

const COPY_BUFFER_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyProgress {
    pub from: PathBuf,
    pub to: PathBuf,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

pub type ProgressSender = mpsc::UnboundedSender<CopyProgress>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferConflictStrategy {
    Fail,
    Replace,
    Skip,
    KeepBoth,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOperationVerification {
    BasicMetadata,
    Strong,
}

impl Default for FileOperationVerification {
    fn default() -> Self {
        Self::BasicMetadata
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOperationRunState {
    Running,
    Paused,
}

#[derive(Clone)]
pub struct FileOperationControls {
    cancel: CancellationToken,
    run_state: watch::Receiver<FileOperationRunState>,
}

impl FileOperationControls {
    pub fn new(
        cancel: CancellationToken,
        run_state: watch::Receiver<FileOperationRunState>,
    ) -> Self {
        Self { cancel, run_state }
    }

    pub fn running(cancel: CancellationToken) -> Self {
        let (_run_state_sender, run_state) = watch::channel(FileOperationRunState::Running);
        Self { cancel, run_state }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub async fn wait_until_running(&mut self) -> Result<(), FileError> {
        loop {
            if self.cancel.is_cancelled() {
                return Err(FileError::Cancelled);
            }
            if *self.run_state.borrow() == FileOperationRunState::Running {
                return Ok(());
            }

            tokio::select! {
                _ = self.cancel.cancelled() => return Err(FileError::Cancelled),
                changed = self.run_state.changed() => {
                    if changed.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct FileTransferOptions {
    pub(super) controls: FileOperationControls,
    pub(super) progress: Option<ProgressSender>,
    pub(super) conflict_strategy: TransferConflictStrategy,
    pub(super) verification: FileOperationVerification,
}

impl FileTransferOptions {
    pub fn new(controls: FileOperationControls) -> Self {
        Self {
            controls,
            progress: None,
            conflict_strategy: TransferConflictStrategy::Fail,
            verification: FileOperationVerification::default(),
        }
    }

    pub fn running(cancel: CancellationToken) -> Self {
        Self::new(FileOperationControls::running(cancel))
    }

    pub fn with_progress_sender(mut self, progress: ProgressSender) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn with_optional_progress(mut self, progress: Option<ProgressSender>) -> Self {
        self.progress = progress;
        self
    }

    pub fn with_conflict_strategy(mut self, conflict_strategy: TransferConflictStrategy) -> Self {
        self.conflict_strategy = conflict_strategy;
        self
    }

    pub fn with_verification(mut self, verification: FileOperationVerification) -> Self {
        self.verification = verification;
        self
    }
}

pub async fn copy_path(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    cancel: CancellationToken,
    progress: Option<ProgressSender>,
) -> Result<(), FileError> {
    copy_path_with_options(
        from,
        to,
        FileTransferOptions::running(cancel).with_optional_progress(progress),
    )
    .await
    .map(|_| ())
}

pub async fn copy_path_with_options(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    transfer_options: FileTransferOptions,
) -> Result<Option<PathBuf>, FileError> {
    copy_path_with_transfer_options(from, to, transfer_options).await
}

async fn copy_path_with_transfer_options(
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

    let metadata = fs::metadata(&from)
        .await
        .map_err(|source| FileError::Metadata {
            path: from.clone(),
            source,
        })?;

    let Some(to) = prepare_copy_target(&from, &to, &metadata, conflict_strategy).await? else {
        return Ok(None);
    };

    if metadata.is_dir() {
        copy_directory(
            &from,
            &to,
            &mut controls,
            progress.as_ref(),
            conflict_strategy,
            verification,
        )
        .await?;
        return Ok(Some(to));
    }

    let mut buffer = vec![0; COPY_BUFFER_SIZE];
    copy_file_to_target(
        &from,
        &to,
        &metadata,
        &mut controls,
        progress.as_ref(),
        &mut buffer,
        verification,
    )
    .await?;
    Ok(Some(to))
}

async fn copy_file(
    from: &Path,
    to: &Path,
    controls: &mut FileOperationControls,
    progress: Option<&ProgressSender>,
    conflict_strategy: TransferConflictStrategy,
    buffer: &mut [u8],
    verification: FileOperationVerification,
) -> Result<(), FileError> {
    let metadata = fs::metadata(from)
        .await
        .map_err(|source| FileError::Metadata {
            path: from.to_path_buf(),
            source,
        })?;
    let Some(to) = prepare_copy_target(from, to, &metadata, conflict_strategy).await? else {
        return Ok(());
    };
    copy_file_to_target(
        from,
        &to,
        &metadata,
        controls,
        progress,
        buffer,
        verification,
    )
    .await
}

async fn copy_file_to_target(
    from: &Path,
    to: &Path,
    metadata: &std::fs::Metadata,
    controls: &mut FileOperationControls,
    progress: Option<&ProgressSender>,
    buffer: &mut [u8],
    verification: FileOperationVerification,
) -> Result<(), FileError> {
    let mut reader = fs::File::open(from)
        .await
        .map_err(|source| FileError::Copy {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        })?;
    let mut writer = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&to)
        .await
        .map_err(|source| FileError::Copy {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        })?;

    let mut source_content_hasher =
        (verification == FileOperationVerification::Strong).then(blake3::Hasher::new);
    let mut bytes_done = 0;
    let bytes_total = metadata.len();

    loop {
        if let Err(error) = controls.wait_until_running().await {
            let _ = fs::remove_file(&to).await;
            return Err(error);
        }

        let read = reader
            .read(buffer)
            .await
            .map_err(|source| FileError::Copy {
                from: from.to_path_buf(),
                to: to.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        if let Some(source_content_hasher) = &mut source_content_hasher {
            source_content_hasher.update(&buffer[..read]);
        }

        writer
            .write_all(&buffer[..read])
            .await
            .map_err(|source| FileError::Copy {
                from: from.to_path_buf(),
                to: to.to_path_buf(),
                source,
            })?;
        bytes_done += read as u64;

        if let Some(progress) = &progress {
            let _ = progress.send(CopyProgress {
                from: from.to_path_buf(),
                to: to.to_path_buf(),
                bytes_done,
                bytes_total,
            });
        }
    }

    if let Err(source) = writer.flush().await {
        let _ = fs::remove_file(to).await;
        return Err(FileError::Copy {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        });
    }
    if verification == FileOperationVerification::Strong {
        if let Err(source) = writer.sync_all().await {
            let _ = fs::remove_file(to).await;
            return Err(FileError::Copy {
                from: from.to_path_buf(),
                to: to.to_path_buf(),
                source,
            });
        }
    }
    drop(writer);

    let permissions_preserved = match fs::set_permissions(to, metadata.permissions()).await {
        Ok(()) => true,
        Err(source) if copy_permission_unsupported(&source) => false,
        Err(source) => {
            let _ = fs::remove_file(to).await;
            return Err(FileError::Copy {
                from: from.to_path_buf(),
                to: to.to_path_buf(),
                source,
            });
        }
    };
    let source_content_hash =
        source_content_hasher.map(|source_content_hasher| source_content_hasher.finalize());
    if let Err(error) = verify_copied_file(
        from,
        to,
        metadata,
        controls,
        buffer,
        source_content_hash,
        permissions_preserved,
    )
    .await
    {
        let _ = fs::remove_file(to).await;
        return Err(error);
    }

    Ok(())
}

async fn verify_copied_file(
    from: &Path,
    to: &Path,
    source_metadata: &std::fs::Metadata,
    controls: &mut FileOperationControls,
    buffer: &mut [u8],
    expected_content_hash: Option<blake3::Hash>,
    permissions_preserved: bool,
) -> Result<(), FileError> {
    controls.wait_until_running().await?;
    let target_metadata = fs::metadata(to).await.map_err(|source| FileError::Copy {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })?;
    if !target_metadata.is_file() {
        return Err(copy_verification_error(
            from,
            to,
            "target is not a regular file after copy",
        ));
    }
    if target_metadata.len() != source_metadata.len() {
        return Err(copy_verification_error(
            from,
            to,
            "target file size differs from source after copy",
        ));
    }
    if permissions_preserved
        && target_metadata.permissions().readonly() != source_metadata.permissions().readonly()
    {
        return Err(copy_verification_error(
            from,
            to,
            "target readonly flag differs from source after copy",
        ));
    }
    if let Some(expected_content_hash) = expected_content_hash {
        let target_content_hash = copied_file_content_hash(from, to, controls, buffer).await?;
        if target_content_hash != expected_content_hash {
            return Err(copy_verification_error(
                from,
                to,
                "target file content hash differs from source after copy",
            ));
        }
    }
    Ok(())
}

fn copy_permission_unsupported(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Unsupported || operation_not_supported_os_error(error)
}

#[cfg(unix)]
fn operation_not_supported_os_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(95)
}

#[cfg(not(unix))]
fn operation_not_supported_os_error(_error: &io::Error) -> bool {
    false
}

async fn copied_file_content_hash(
    from: &Path,
    to: &Path,
    controls: &mut FileOperationControls,
    buffer: &mut [u8],
) -> Result<blake3::Hash, FileError> {
    let mut target = fs::File::open(to).await.map_err(|source| FileError::Copy {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })?;
    let mut target_content_hasher = blake3::Hasher::new();

    loop {
        controls.wait_until_running().await?;
        let read = target
            .read(buffer)
            .await
            .map_err(|source| FileError::Copy {
                from: from.to_path_buf(),
                to: to.to_path_buf(),
                source,
            })?;
        if read == 0 {
            return Ok(target_content_hasher.finalize());
        }
        target_content_hasher.update(&buffer[..read]);
    }
}

async fn verify_copied_directory(from: &Path, to: &Path) -> Result<(), FileError> {
    let target_metadata = fs::metadata(to).await.map_err(|source| FileError::Copy {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })?;
    if !target_metadata.is_dir() {
        return Err(copy_verification_error(
            from,
            to,
            "target is not a directory after copy",
        ));
    }
    Ok(())
}

fn copy_verification_error(from: &Path, to: &Path, message: &'static str) -> FileError {
    FileError::Copy {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidData, message),
    }
}

async fn copy_directory(
    from: &Path,
    to: &Path,
    controls: &mut FileOperationControls,
    progress: Option<&ProgressSender>,
    conflict_strategy: TransferConflictStrategy,
    verification: FileOperationVerification,
) -> Result<(), FileError> {
    if to.starts_with(from) {
        return Err(FileError::InvalidInput {
            path: to.to_path_buf(),
            message: "cannot copy a directory into itself".to_owned(),
        });
    }

    let created_root = ensure_copy_directory_target(from, to, conflict_strategy).await?;
    if let Err(error) = verify_copied_directory(from, to).await {
        if created_root {
            let _ = fs::remove_dir_all(to).await;
        }
        return Err(error);
    }

    let mut buffer = vec![0; COPY_BUFFER_SIZE];
    let mut pending_directories = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((source_directory, target_directory)) = pending_directories.pop() {
        if let Err(error) = controls.wait_until_running().await {
            if created_root {
                let _ = fs::remove_dir_all(to).await;
            }
            return Err(error);
        }

        let mut entries =
            fs::read_dir(&source_directory)
                .await
                .map_err(|source| FileError::Copy {
                    from: source_directory.clone(),
                    to: target_directory.clone(),
                    source,
                })?;

        loop {
            if let Err(error) = controls.wait_until_running().await {
                if created_root {
                    let _ = fs::remove_dir_all(to).await;
                }
                return Err(error);
            }

            let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|source| FileError::Copy {
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

            if file_type.is_dir() {
                let source_metadata =
                    fs::metadata(&source_child)
                        .await
                        .map_err(|source| FileError::Metadata {
                            path: source_child.clone(),
                            source,
                        })?;
                if let Some(target_child) = prepare_copy_target(
                    &source_child,
                    &target_child,
                    &source_metadata,
                    conflict_strategy,
                )
                .await?
                {
                    if let Err(error) = ensure_copy_directory_target(
                        &source_child,
                        &target_child,
                        conflict_strategy,
                    )
                    .await
                    {
                        if created_root {
                            let _ = fs::remove_dir_all(to).await;
                        }
                        return Err(error);
                    }
                    if let Err(error) = verify_copied_directory(&source_child, &target_child).await
                    {
                        if created_root {
                            let _ = fs::remove_dir_all(to).await;
                        }
                        return Err(error);
                    }
                    pending_directories.push((source_child, target_child));
                }
            } else if let Err(error) = copy_file(
                &source_child,
                &target_child,
                controls,
                progress,
                nested_copy_conflict_strategy(conflict_strategy),
                &mut buffer,
                verification,
            )
            .await
            {
                if created_root {
                    let _ = fs::remove_dir_all(to).await;
                }
                return Err(error);
            }
        }
    }

    Ok(())
}

async fn prepare_copy_target(
    from: &Path,
    to: &Path,
    source_metadata: &std::fs::Metadata,
    conflict_strategy: TransferConflictStrategy,
) -> Result<Option<PathBuf>, FileError> {
    if from == to && conflict_strategy != TransferConflictStrategy::KeepBoth {
        return Ok(None);
    }

    let Some(target_metadata) = transfer_target_metadata_if_exists(to)
        .await
        .map_err(|source| FileError::Copy {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        })?
    else {
        return Ok(Some(to.to_path_buf()));
    };

    match conflict_strategy {
        TransferConflictStrategy::Fail => Err(FileError::Copy {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source: already_exists_error(),
        }),
        TransferConflictStrategy::Replace => {
            ensure_replace_target_does_not_contain_source_path(from, to, &target_metadata)?;
            remove_copy_target(from, to, &target_metadata).await?;
            Ok(Some(to.to_path_buf()))
        }
        TransferConflictStrategy::Skip => Ok(None),
        TransferConflictStrategy::KeepBoth => available_transfer_target_path_candidate(to)
            .await
            .map(Some)
            .map_err(|source| FileError::Copy {
                from: from.to_path_buf(),
                to: to.to_path_buf(),
                source,
            }),
        TransferConflictStrategy::Merge => {
            if source_metadata.is_dir() && target_metadata.is_dir() {
                Ok(Some(to.to_path_buf()))
            } else {
                Ok(None)
            }
        }
    }
}

async fn ensure_copy_directory_target(
    from: &Path,
    to: &Path,
    _conflict_strategy: TransferConflictStrategy,
) -> Result<bool, FileError> {
    if transfer_target_metadata_if_exists(to)
        .await
        .map_err(|source| FileError::Copy {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        })?
        .is_some()
    {
        return Ok(false);
    }

    fs::create_dir(to).await.map_err(|source| FileError::Copy {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })?;

    Ok(true)
}

fn nested_copy_conflict_strategy(
    conflict_strategy: TransferConflictStrategy,
) -> TransferConflictStrategy {
    if conflict_strategy == TransferConflictStrategy::Merge {
        TransferConflictStrategy::Skip
    } else {
        conflict_strategy
    }
}

async fn remove_copy_target(
    from: &Path,
    to: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), FileError> {
    let result = if metadata.is_dir() {
        fs::remove_dir_all(to).await
    } else {
        fs::remove_file(to).await
    };

    result.map_err(|source| FileError::Copy {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests;
