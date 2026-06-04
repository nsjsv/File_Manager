use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use file_core::{
    copy_path_with_options, create_directory, create_empty_file, delete_trash_entry, empty_trash,
    move_path_with_options, rename_path, restore_trash_entry, trash_path_with_restore_entry,
    CopyProgress, FileOperationControls, FileTransferOptions, TransferConflictStrategy,
    TrashRestoreEntry,
};
use iced::futures::channel::mpsc::Sender as IcedSender;
use iced::futures::SinkExt;
use iced::Subscription;

use crate::model::Message;
use crate::operation_history::{CompletedTransfer, FileOperationOutcome};
use crate::operation_queue::{
    FileOperationProgressUpdate, QueuedFileOperation, QueuedTransfer, RunningFileOperation,
    NEW_DIRECTORY_NAME, NEW_FILE_NAME,
};

const FILE_OPERATION_CHANNEL_SIZE: usize = 32;
const COPY_PROGRESS_UI_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn file_operation_subscription(task: RunningFileOperation) -> Subscription<Message> {
    let task_id = task.id;
    iced::subscription::channel(
        ("file-operation", task_id),
        FILE_OPERATION_CHANNEL_SIZE,
        move |mut output| async move {
            let result =
                run_queued_file_operation(task.operation, task.controls, task_id, &mut output)
                    .await;
            let _ = output
                .send(Message::FileOperationFinished(task_id, result))
                .await;
            iced::futures::future::pending().await
        },
    )
}

async fn run_queued_file_operation(
    operation: QueuedFileOperation,
    controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<FileOperationOutcome, String> {
    match operation {
        QueuedFileOperation::Rename { path, new_name } => {
            run_queued_rename(path, new_name, controls, task_id, output).await
        }
        QueuedFileOperation::CreateDirectory { parent } => {
            run_queued_create_directory(parent, controls, task_id, output).await
        }
        QueuedFileOperation::CreateEmptyFile { parent } => {
            run_queued_create_empty_file(parent, controls, task_id, output).await
        }
        QueuedFileOperation::Trash { paths } => {
            run_queued_trash(paths, controls, task_id, output).await
        }
        QueuedFileOperation::Restore { entries } => {
            run_queued_restore(entries, controls, task_id, output).await
        }
        QueuedFileOperation::DeleteTrashEntries { entries } => {
            run_queued_delete_trash_entries(entries, controls, task_id, output).await
        }
        QueuedFileOperation::EmptyTrash => run_queued_empty_trash(controls, task_id, output).await,
        QueuedFileOperation::Copy { transfers } => {
            run_queued_transfers(
                transfers,
                controls,
                task_id,
                output,
                QueuedTransferMode::Copy,
            )
            .await
        }
        QueuedFileOperation::Move { transfers } => {
            run_queued_transfers(
                transfers,
                controls,
                task_id,
                output,
                QueuedTransferMode::Move,
            )
            .await
        }
    }
}

async fn run_queued_rename(
    path: PathBuf,
    new_name: String,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<FileOperationOutcome, String> {
    send_file_operation_progress(output, task_id, FileOperationProgressUpdate::Indeterminate).await;
    controls
        .wait_until_running()
        .await
        .map_err(|error| error.to_string())?;
    let target = rename_path(path.clone(), OsString::from(new_name))
        .await
        .map_err(|error| error.to_string())?;
    send_file_operation_progress(
        output,
        task_id,
        FileOperationProgressUpdate::Items {
            completed: 1,
            total: 1,
        },
    )
    .await;
    Ok(FileOperationOutcome::Rename {
        from: path,
        to: target,
    })
}

async fn run_queued_create_directory(
    parent: PathBuf,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<FileOperationOutcome, String> {
    controls
        .wait_until_running()
        .await
        .map_err(|error| error.to_string())?;
    let path = parent.join(NEW_DIRECTORY_NAME);
    create_directory(&path)
        .await
        .map_err(|error| error.to_string())?;
    send_file_operation_progress(
        output,
        task_id,
        FileOperationProgressUpdate::Items {
            completed: 1,
            total: 1,
        },
    )
    .await;
    Ok(FileOperationOutcome::CreateDirectory { path })
}

async fn run_queued_create_empty_file(
    parent: PathBuf,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<FileOperationOutcome, String> {
    controls
        .wait_until_running()
        .await
        .map_err(|error| error.to_string())?;
    let path = parent.join(NEW_FILE_NAME);
    create_empty_file(&path)
        .await
        .map_err(|error| error.to_string())?;
    send_file_operation_progress(
        output,
        task_id,
        FileOperationProgressUpdate::Items {
            completed: 1,
            total: 1,
        },
    )
    .await;
    Ok(FileOperationOutcome::CreateEmptyFile { path })
}

async fn run_queued_trash(
    paths: Vec<PathBuf>,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<FileOperationOutcome, String> {
    let total = paths.len();
    let mut entries = Vec::new();
    for (index, path) in paths.iter().cloned().enumerate() {
        controls
            .wait_until_running()
            .await
            .map_err(|error| error.to_string())?;
        if let Some(entry) = trash_path_with_restore_entry(path)
            .await
            .map_err(|error| error.to_string())?
        {
            entries.push(entry);
        }
        send_file_operation_progress(
            output,
            task_id,
            FileOperationProgressUpdate::Items {
                completed: index + 1,
                total,
            },
        )
        .await;
    }

    if entries.len() == total {
        Ok(FileOperationOutcome::Trash { paths, entries })
    } else {
        Ok(FileOperationOutcome::NoHistory)
    }
}

async fn run_queued_restore(
    entries: Vec<TrashRestoreEntry>,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<FileOperationOutcome, String> {
    let total = entries.len();
    let mut restored_paths = Vec::with_capacity(total);
    for (index, entry) in entries.iter().cloned().enumerate() {
        controls
            .wait_until_running()
            .await
            .map_err(|error| error.to_string())?;
        let restored_path = restore_trash_entry(entry, TransferConflictStrategy::KeepBoth)
            .await
            .map_err(|error| error.to_string())?;
        restored_paths.push(restored_path);
        send_file_operation_progress(
            output,
            task_id,
            FileOperationProgressUpdate::Items {
                completed: index + 1,
                total,
            },
        )
        .await;
    }
    Ok(FileOperationOutcome::Restore {
        entries,
        restored_paths,
    })
}

async fn run_queued_delete_trash_entries(
    entries: Vec<TrashRestoreEntry>,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<FileOperationOutcome, String> {
    let total = entries.len();
    for (index, entry) in entries.into_iter().enumerate() {
        controls
            .wait_until_running()
            .await
            .map_err(|error| error.to_string())?;
        delete_trash_entry(entry)
            .await
            .map_err(|error| error.to_string())?;
        send_file_operation_progress(
            output,
            task_id,
            FileOperationProgressUpdate::Items {
                completed: index + 1,
                total,
            },
        )
        .await;
    }
    Ok(FileOperationOutcome::NoHistory)
}

async fn run_queued_empty_trash(
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<FileOperationOutcome, String> {
    send_file_operation_progress(output, task_id, FileOperationProgressUpdate::Indeterminate).await;
    controls
        .wait_until_running()
        .await
        .map_err(|error| error.to_string())?;
    empty_trash().await.map_err(|error| error.to_string())?;
    send_file_operation_progress(
        output,
        task_id,
        FileOperationProgressUpdate::Items {
            completed: 1,
            total: 1,
        },
    )
    .await;
    Ok(FileOperationOutcome::NoHistory)
}

#[derive(Debug, Clone, Copy)]
enum QueuedTransferMode {
    Copy,
    Move,
}

async fn run_queued_transfers(
    transfers: Vec<QueuedTransfer>,
    controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
    mode: QueuedTransferMode,
) -> Result<FileOperationOutcome, String> {
    let can_record_history = transfers.iter().all(history_safe_transfer);
    let total = transfers.len();
    let mut completed = Vec::new();
    for (index, transfer) in transfers.into_iter().enumerate() {
        if let Some(completed_transfer) = run_queued_transfer(
            transfer,
            controls.clone(),
            task_id,
            output,
            mode,
            index,
            total,
        )
        .await?
        {
            completed.push(completed_transfer);
        }
        send_file_operation_progress(
            output,
            task_id,
            FileOperationProgressUpdate::Items {
                completed: index + 1,
                total,
            },
        )
        .await;
    }

    if !can_record_history {
        return Ok(FileOperationOutcome::NoHistory);
    }

    match mode {
        QueuedTransferMode::Copy => Ok(FileOperationOutcome::Copy {
            transfers: completed,
        }),
        QueuedTransferMode::Move => Ok(FileOperationOutcome::Move {
            transfers: completed,
        }),
    }
}

async fn run_queued_transfer(
    transfer: QueuedTransfer,
    controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
    mode: QueuedTransferMode,
    completed_transfers: usize,
    total_transfers: usize,
) -> Result<Option<CompletedTransfer>, String> {
    let source = transfer.source.clone();
    let (progress_sender, mut progress_receiver) = tokio::sync::mpsc::unbounded_channel();
    let transfer = async move {
        let transfer_options = FileTransferOptions::new(controls)
            .with_progress_sender(progress_sender)
            .with_conflict_strategy(transfer.conflict_strategy);
        match mode {
            QueuedTransferMode::Copy => {
                copy_path_with_options(transfer.source, transfer.target, transfer_options).await
            }
            QueuedTransferMode::Move => {
                move_path_with_options(transfer.source, transfer.target, transfer_options).await
            }
        }
    };
    tokio::pin!(transfer);
    let mut latest_copy_progress = None;
    let mut last_copy_progress_sent_at = None;

    loop {
        tokio::select! {
            progress = progress_receiver.recv() => {
                if let Some(progress) = progress {
                    latest_copy_progress = Some(progress);
                    let now = Instant::now();
                    if should_send_copy_progress(last_copy_progress_sent_at, now) {
                        if let Some(progress) = latest_copy_progress.take() {
                            send_copy_progress(
                                output,
                                task_id,
                                progress,
                                completed_transfers,
                                total_transfers,
                            ).await;
                            last_copy_progress_sent_at = Some(now);
                        }
                    }
                }
            }
            transfer_outcome = &mut transfer => {
                if let Some(progress) = latest_copy_progress.take() {
                    send_copy_progress(
                        output,
                        task_id,
                        progress,
                        completed_transfers,
                        total_transfers,
                    ).await;
                }
                return transfer_outcome
                    .map(|target| target.map(|target| CompletedTransfer { source, target }))
                    .map_err(|error| error.to_string());
            }
        }
    }
}

fn history_safe_transfer(transfer: &QueuedTransfer) -> bool {
    matches!(
        transfer.conflict_strategy,
        TransferConflictStrategy::Fail
            | TransferConflictStrategy::KeepBoth
            | TransferConflictStrategy::Skip
    )
}

fn should_send_copy_progress(last_sent_at: Option<Instant>, now: Instant) -> bool {
    match last_sent_at {
        Some(last_sent_at) => now.duration_since(last_sent_at) >= COPY_PROGRESS_UI_INTERVAL,
        None => true,
    }
}

async fn send_copy_progress(
    output: &mut IcedSender<Message>,
    task_id: u64,
    progress: CopyProgress,
    completed_transfers: usize,
    total_transfers: usize,
) {
    send_file_operation_progress(
        output,
        task_id,
        FileOperationProgressUpdate::Bytes {
            bytes_done: progress.bytes_done,
            bytes_total: progress.bytes_total,
            completed_transfers,
            total_transfers,
        },
    )
    .await;
}

async fn send_file_operation_progress(
    output: &mut IcedSender<Message>,
    task_id: u64,
    progress: FileOperationProgressUpdate,
) {
    let _ = output
        .send(Message::FileOperationProgressed(task_id, progress))
        .await;
}
