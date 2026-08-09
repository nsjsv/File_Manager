use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::transfer_conflict::{
    available_transfer_target_path_candidate, transfer_target_metadata_if_exists,
};
use crate::FileError;

use super::copy_verification::{
    verify_copied_directory, verify_copied_file, verify_copied_symbolic_link,
};
use super::transfer_metadata::apply_transfer_metadata_best_effort;
use super::transfer_object::{inspect_transfer_source, TransferSourceKind, TransferSourceObject};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferConflictStrategy {
    Fail,
    Replace,
    Skip,
    KeepBoth,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileOperationVerification {
    #[default]
    BasicMetadata,
    Strong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOperationRunState {
    Running,
    Paused,
    ApplicationStopping,
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

    pub fn checkpoint_now(&self) -> Result<(), FileError> {
        match *self.run_state.borrow() {
            FileOperationRunState::ApplicationStopping => Err(FileError::ApplicationStopping),
            FileOperationRunState::Running if self.cancel.is_cancelled() => {
                Err(FileError::Cancelled)
            }
            FileOperationRunState::Running | FileOperationRunState::Paused => Ok(()),
        }
    }

    pub async fn wait_until_running(&mut self) -> Result<(), FileError> {
        loop {
            match *self.run_state.borrow() {
                FileOperationRunState::ApplicationStopping => {
                    return Err(FileError::ApplicationStopping)
                }
                FileOperationRunState::Running if self.cancel.is_cancelled() => {
                    return Err(FileError::Cancelled)
                }
                FileOperationRunState::Running => return Ok(()),
                FileOperationRunState::Paused => {}
            }

            tokio::select! {
                changed = self.run_state.changed() => {
                    if changed.is_err() {
                        return Ok(());
                    }
                }
                _ = self.cancel.cancelled() => {
                    if *self.run_state.borrow() == FileOperationRunState::ApplicationStopping {
                        return Err(FileError::ApplicationStopping);
                    }
                    return Err(FileError::Cancelled);
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
    let mut controls = transfer_options.controls.clone();
    controls.wait_until_running().await?;
    let source_object = inspect_transfer_source(&from).await?;

    copy_path_with_inspected_source(&from, &to, &source_object, transfer_options).await
}

pub(super) async fn copy_path_with_inspected_source(
    from: &Path,
    to: &Path,
    source_object: &TransferSourceObject,
    transfer_options: FileTransferOptions,
) -> Result<Option<PathBuf>, FileError> {
    let mut controls = transfer_options.controls;
    let progress = transfer_options.progress;
    let conflict_strategy = transfer_options.conflict_strategy;
    let verification = transfer_options.verification;
    controls.wait_until_running().await?;

    let Some(to) =
        prepare_copy_target(from, to, &source_object.metadata, conflict_strategy).await?
    else {
        return Ok(None);
    };

    match &source_object.kind {
        TransferSourceKind::Directory => {
            copy_directory(
                from,
                &to,
                source_object,
                &mut controls,
                progress.as_ref(),
                conflict_strategy,
                verification,
            )
            .await?;
        }
        TransferSourceKind::RegularFile => {
            let mut buffer = vec![0; COPY_BUFFER_SIZE];
            copy_file_to_target(
                from,
                &to,
                source_object,
                &mut controls,
                progress.as_ref(),
                &mut buffer,
                verification,
            )
            .await?;
        }
        TransferSourceKind::SymbolicLink { .. } => {
            copy_symbolic_link_to_target(from, &to, source_object, &mut controls).await?;
        }
    }

    Ok(Some(to))
}

async fn copy_file_to_target(
    from: &Path,
    to: &Path,
    source_object: &TransferSourceObject,
    controls: &mut FileOperationControls,
    progress: Option<&ProgressSender>,
    buffer: &mut [u8],
    verification: FileOperationVerification,
) -> Result<(), FileError> {
    let metadata = &source_object.metadata;
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

    let source_content_hash =
        source_content_hasher.map(|source_content_hasher| source_content_hasher.finalize());
    if let Err(error) =
        verify_copied_file(from, to, metadata, controls, buffer, source_content_hash).await
    {
        let _ = fs::remove_file(to).await;
        return Err(error);
    }

    apply_transfer_metadata_best_effort(from, to, source_object).await;
    if let Err(error) = controls.wait_until_running().await {
        let _ = fs::remove_file(to).await;
        return Err(error);
    }

    Ok(())
}

#[cfg(unix)]
async fn copy_symbolic_link_to_target(
    from: &Path,
    to: &Path,
    source_object: &TransferSourceObject,
    controls: &mut FileOperationControls,
) -> Result<(), FileError> {
    let TransferSourceKind::SymbolicLink {
        target: link_target,
    } = &source_object.kind
    else {
        unreachable!();
    };
    fs::symlink(link_target, to)
        .await
        .map_err(|source| FileError::Copy {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        })?;

    let completion = async {
        verify_copied_symbolic_link(from, to, link_target).await?;
        controls.wait_until_running().await?;
        apply_transfer_metadata_best_effort(from, to, source_object).await;
        controls.wait_until_running().await
    }
    .await;

    if completion.is_err() {
        let _ = fs::remove_file(to).await;
    }
    completion
}

#[cfg(not(unix))]
async fn copy_symbolic_link_to_target(
    from: &Path,
    _to: &Path,
    _source_object: &TransferSourceObject,
    _controls: &mut FileOperationControls,
) -> Result<(), FileError> {
    Err(FileError::InvalidInput {
        path: from.to_path_buf(),
        message: "cannot transfer symbolic links on this platform".to_owned(),
    })
}

enum DirectoryCopyStep {
    CopyChildren {
        source: PathBuf,
        target: PathBuf,
    },
    PreserveMetadata {
        source: PathBuf,
        target: PathBuf,
        source_object: TransferSourceObject,
    },
}

async fn copy_directory(
    from: &Path,
    to: &Path,
    source_object: &TransferSourceObject,
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

    let created_root = ensure_copy_directory_target(from, to).await?;
    let mut pending_steps = Vec::new();
    if created_root {
        pending_steps.push(DirectoryCopyStep::PreserveMetadata {
            source: from.to_path_buf(),
            target: to.to_path_buf(),
            source_object: source_object.clone(),
        });
    }
    pending_steps.push(DirectoryCopyStep::CopyChildren {
        source: from.to_path_buf(),
        target: to.to_path_buf(),
    });

    let copy_outcome = async {
        verify_copied_directory(from, to).await?;
        copy_directory_contents(
            pending_steps,
            controls,
            progress,
            conflict_strategy,
            verification,
        )
        .await
    }
    .await;

    if copy_outcome.is_err() && created_root {
        let _ = fs::remove_dir_all(to).await;
    }
    copy_outcome
}

async fn copy_directory_contents(
    mut pending_steps: Vec<DirectoryCopyStep>,
    controls: &mut FileOperationControls,
    progress: Option<&ProgressSender>,
    conflict_strategy: TransferConflictStrategy,
    verification: FileOperationVerification,
) -> Result<(), FileError> {
    let mut buffer = vec![0; COPY_BUFFER_SIZE];

    while let Some(step) = pending_steps.pop() {
        controls.wait_until_running().await?;
        let (source_directory, target_directory) = match step {
            DirectoryCopyStep::CopyChildren { source, target } => (source, target),
            DirectoryCopyStep::PreserveMetadata {
                source,
                target,
                source_object,
            } => {
                apply_transfer_metadata_best_effort(&source, &target, &source_object).await;
                controls.wait_until_running().await?;
                continue;
            }
        };
        let mut entries =
            fs::read_dir(&source_directory)
                .await
                .map_err(|source| FileError::Copy {
                    from: source_directory.clone(),
                    to: target_directory.clone(),
                    source,
                })?;

        loop {
            controls.wait_until_running().await?;
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
            let source_object = inspect_transfer_source(&source_child).await?;
            let child_conflict_strategy = match &source_object.kind {
                TransferSourceKind::Directory => conflict_strategy,
                TransferSourceKind::RegularFile | TransferSourceKind::SymbolicLink { .. } => {
                    nested_copy_conflict_strategy(conflict_strategy)
                }
            };
            let Some(target_child) = prepare_copy_target(
                &source_child,
                &target_child,
                &source_object.metadata,
                child_conflict_strategy,
            )
            .await?
            else {
                continue;
            };

            match &source_object.kind {
                TransferSourceKind::Directory => {
                    let created_directory =
                        ensure_copy_directory_target(&source_child, &target_child).await?;
                    verify_copied_directory(&source_child, &target_child).await?;
                    if created_directory {
                        pending_steps.push(DirectoryCopyStep::PreserveMetadata {
                            source: source_child.clone(),
                            target: target_child.clone(),
                            source_object: source_object.clone(),
                        });
                    }
                    pending_steps.push(DirectoryCopyStep::CopyChildren {
                        source: source_child,
                        target: target_child,
                    });
                }
                TransferSourceKind::RegularFile => {
                    copy_file_to_target(
                        &source_child,
                        &target_child,
                        &source_object,
                        controls,
                        progress,
                        &mut buffer,
                        verification,
                    )
                    .await?;
                }
                TransferSourceKind::SymbolicLink { .. } => {
                    copy_symbolic_link_to_target(
                        &source_child,
                        &target_child,
                        &source_object,
                        controls,
                    )
                    .await?;
                }
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

async fn ensure_copy_directory_target(from: &Path, to: &Path) -> Result<bool, FileError> {
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
