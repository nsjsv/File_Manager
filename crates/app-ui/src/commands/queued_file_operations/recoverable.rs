use super::*;
use file_operation_store::TransferManifestCheckpointUpdate;

mod progress;
mod store_codec;

use progress::TransferBatchProgress;
pub(super) use progress::{
    send_archive_creation_progress, send_archive_extraction_progress, send_file_operation_progress,
};

#[derive(Clone)]
struct TaskQueueTransferJournal {
    store: TaskQueueStore,
    controls: FileOperationControls,
}

impl TransferJournal for TaskQueueTransferJournal {
    fn commit(
        &self,
        mutation: TransferJournalMutation,
    ) -> Pin<Box<dyn Future<Output = Result<u64, TransferJournalError>> + Send + '_>> {
        let store = self.store.clone();
        let controls = self.controls.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || match mutation {
                TransferJournalMutation::InstallManifestAndCheckpoint {
                    task_id,
                    key,
                    expected_revision,
                    manifest,
                    replacement_manifest,
                    checkpoint,
                } => {
                    let stored_key = store_codec::encode_work_key(&key);
                    let encoding_controls = controls.clone();
                    let manifest_entries = store_codec::encode_manifest_entries_while(
                        key.transfer_index,
                        &manifest,
                        || encoding_controls.checkpoint_now().is_ok(),
                    )
                    .ok_or_else(|| interrupted_journal_error(&controls))?;
                    let replacement_manifest_entries = match replacement_manifest.as_ref() {
                        Some(manifest) => store_codec::encode_manifest_entries_while(
                            key.transfer_index,
                            manifest,
                            || encoding_controls.checkpoint_now().is_ok(),
                        )
                        .ok_or_else(|| interrupted_journal_error(&controls))?,
                        None => Vec::new(),
                    };
                    let stored_checkpoint =
                        store_codec::encode_checkpoint(&checkpoint).map_err(store_journal_error)?;
                    let transaction_controls = controls.clone();
                    let revision = store
                        .install_transfer_manifests_and_checkpoint_while(
                            task_id,
                            TransferManifestCheckpointUpdate {
                                key: &stored_key,
                                expected_revision,
                                manifest_entries: &manifest_entries,
                                replacement_manifest_entries: &replacement_manifest_entries,
                                checkpoint: &stored_checkpoint,
                            },
                            || transaction_controls.checkpoint_now().is_ok(),
                        )
                        .map_err(store_journal_error)?;
                    revision.ok_or_else(|| interrupted_journal_error(&controls))
                }
                TransferJournalMutation::CompareAndSwapCheckpoint {
                    task_id,
                    key,
                    expected_revision,
                    checkpoint,
                } => store
                    .compare_and_swap_transfer_checkpoint(
                        task_id,
                        &store_codec::encode_work_key(&key),
                        expected_revision,
                        &store_codec::encode_checkpoint(&checkpoint)
                            .map_err(store_journal_error)?,
                    )
                    .map_err(store_journal_error),
                TransferJournalMutation::PersistMergeCompletionAndCheckpoint {
                    task_id,
                    key,
                    expected_revision,
                    completion,
                    checkpoint,
                } => store
                    .compare_and_swap_transfer_merge_completion(
                        task_id,
                        &store_codec::encode_work_key(&key),
                        expected_revision,
                        &store_codec::encode_merge_completion(&completion)
                            .map_err(store_journal_error)?,
                        &store_codec::encode_checkpoint(&checkpoint)
                            .map_err(store_journal_error)?,
                    )
                    .map_err(store_journal_error),
            })
            .await
            .map_err(|error| TransferJournalError::Storage(error.to_string()))?
        })
    }
}

fn store_journal_error(error: file_operation_store::StoreError) -> TransferJournalError {
    match error {
        file_operation_store::StoreError::StaleTransferRevision { .. } => {
            TransferJournalError::StaleRevision
        }
        error => TransferJournalError::Storage(error.to_string()),
    }
}

fn interrupted_journal_error(controls: &FileOperationControls) -> TransferJournalError {
    match controls.checkpoint_now() {
        Err(FileError::ApplicationStopping) => TransferJournalError::ApplicationStopping,
        Err(FileError::Cancelled) => TransferJournalError::UserCancelled,
        _ => TransferJournalError::Storage(
            "manifest transaction stopped without an operation control signal".to_owned(),
        ),
    }
}

pub(super) async fn run_queued_transfers(
    transfers: Vec<QueuedTransfer>,
    controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
    store: Option<TaskQueueStore>,
    mode: QueuedTransferMode,
    verification: FileOperationVerification,
) -> FileOperationCompletion {
    let Some(store) = store else {
        return FileOperationCompletion::from_result(Err(
            "File operation queue storage is unavailable; transfer was not started".to_owned(),
        ));
    };
    let mut records = match load_recoverable_transfer_records(store.clone(), task_id).await {
        Ok(records) => records,
        Err(RecoverableRecordLoadError::Interrupted(error)) => {
            return FileOperationCompletion::RecoveryInterrupted(error, Vec::new());
        }
        Err(RecoverableRecordLoadError::Invalid(error)) => {
            return FileOperationCompletion::RecoveryBlocked {
                error,
                completed_move_transfers: Vec::new(),
            };
        }
    };
    if records.len() != transfers.len()
        || records
            .iter()
            .zip(&transfers)
            .enumerate()
            .any(|(transfer_index, (record, transfer))| {
                record.key != file_core::TransferWorkKey::top_level(transfer_index as u64)
                    || record.request.source != transfer.source
                    || record.request.requested_target != transfer.target
                    || record.request.conflict_strategy != transfer.conflict_strategy
                    || record.request.verification != verification
                    || record.request.operation != mode.operation()
            })
    {
        return FileOperationCompletion::RecoveryBlocked {
            error: "Stored transfer journal does not match the queued operation".to_owned(),
            completed_move_transfers: Vec::new(),
        };
    }

    let history_eligibility = if transfers.iter().all(history_safe_transfer) {
        FileOperationHistoryEligibility::Replayable
    } else {
        FileOperationHistoryEligibility::NotReplayable
    };
    let journal = TaskQueueTransferJournal {
        store,
        controls: controls.clone(),
    };
    let mut manifest_controls = controls.clone();
    for record in &mut records {
        if record.manifest.is_some()
            || !matches!(
                record.checkpoint,
                file_core::TransferCheckpoint::AwaitingManifest
            )
        {
            continue;
        }
        if manifest_controls.wait_until_running().await.is_err() {
            break;
        }
        if let Err(error) = persist_recoverable_source_manifest_with_controls(
            record,
            &journal,
            &mut manifest_controls,
        )
        .await
        {
            if matches!(
                error,
                RecoverableTransferError::Journal { .. }
                    | RecoverableTransferError::RecoveryRequired { .. }
            ) {
                return recoverable_transfer_failure(mode, error, Vec::new());
            }
            break;
        }
    }

    let mut batch_progress = TransferBatchProgress::new(&records);
    let mut completed = Vec::new();
    for (index, record) in records.into_iter().enumerate() {
        let transfer_outcome = run_recoverable_record_with_progress(
            record,
            controls.clone(),
            &journal,
            task_id,
            output,
            index,
            &mut batch_progress,
        )
        .await;
        match transfer_outcome {
            Ok(RecoverableTransferOutcome {
                source,
                final_target: Some(target),
            }) => completed.push(CompletedTransfer { source, target }),
            Ok(RecoverableTransferOutcome {
                final_target: None, ..
            }) => {}
            Err(error) => {
                return recoverable_transfer_failure(mode, error, completed);
            }
        }
        send_file_operation_progress(output, task_id, batch_progress.complete_record(index)).await;
    }

    match mode {
        QueuedTransferMode::Copy
            if history_eligibility == FileOperationHistoryEligibility::Replayable =>
        {
            FileOperationCompletion::Succeeded(FileOperationOutcome::Copy {
                transfers: completed,
            })
        }
        QueuedTransferMode::Copy => {
            FileOperationCompletion::Succeeded(FileOperationOutcome::NoHistory)
        }
        QueuedTransferMode::Move => {
            FileOperationCompletion::Succeeded(FileOperationOutcome::Move {
                transfers: completed,
                history_eligibility,
            })
        }
    }
}

fn recoverable_transfer_failure(
    mode: QueuedTransferMode,
    error: RecoverableTransferError,
    completed_move_transfers: Vec<CompletedTransfer>,
) -> FileOperationCompletion {
    let diagnostic = error.to_string();
    match error {
        RecoverableTransferError::FileOperation(file_core::FileError::ApplicationStopping) => {
            FileOperationCompletion::RecoveryInterrupted(diagnostic, completed_move_transfers)
        }
        RecoverableTransferError::FileOperation(file_core::FileError::Cancelled) => {
            FileOperationCompletion::Canceled(match mode {
                QueuedTransferMode::Copy => Vec::new(),
                QueuedTransferMode::Move => completed_move_transfers,
            })
        }
        RecoverableTransferError::Journal { .. }
        | RecoverableTransferError::RecoveryRequired { .. } => {
            FileOperationCompletion::RecoveryInterrupted(diagnostic, completed_move_transfers)
        }
        RecoverableTransferError::RecoveryBlocked { .. } => {
            FileOperationCompletion::RecoveryBlocked {
                error: diagnostic,
                completed_move_transfers,
            }
        }
        _ => match mode {
            QueuedTransferMode::Copy => FileOperationCompletion::from_result(Err(diagnostic)),
            QueuedTransferMode::Move => FileOperationCompletion::failed_after_completed_moves(
                diagnostic,
                completed_move_transfers,
            ),
        },
    }
}

#[derive(Debug)]
enum RecoverableRecordLoadError {
    Interrupted(String),
    Invalid(String),
}

async fn load_recoverable_transfer_records(
    store: TaskQueueStore,
    task_id: u64,
) -> Result<Vec<TransferJournalRecord>, RecoverableRecordLoadError> {
    let records = tokio::task::spawn_blocking(move || {
        let snapshot = store.read_transfer_recovery(task_id)?;
        store_codec::decode_recovery_snapshot(task_id, snapshot)
    })
    .await
    .map_err(|error| {
        RecoverableRecordLoadError::Interrupted(format!("Transfer recovery worker failed: {error}"))
    })?;
    records.map_err(|error| {
        let message = format!("Transfer recovery journal could not be read: {error}");
        if error.is_invalid_recovery_data() {
            RecoverableRecordLoadError::Invalid(message)
        } else {
            RecoverableRecordLoadError::Interrupted(message)
        }
    })
}

async fn run_recoverable_record_with_progress(
    record: TransferJournalRecord,
    controls: FileOperationControls,
    journal: &TaskQueueTransferJournal,
    task_id: u64,
    output: &mut IcedSender<Message>,
    record_index: usize,
    batch_progress: &mut TransferBatchProgress,
) -> Result<RecoverableTransferOutcome, RecoverableTransferError> {
    let (progress_sender, mut progress_receiver) = tokio::sync::mpsc::unbounded_channel();
    let transfer_options = FileTransferOptions::new(controls).with_progress_sender(progress_sender);
    let transfer = run_recoverable_transfer(record, journal, transfer_options);
    tokio::pin!(transfer);
    let mut latest_copy_progress = None;
    let mut last_copy_progress_sent_at = None;

    loop {
        tokio::select! {
            progress = progress_receiver.recv() => {
                if let Some(progress) = progress {
                    latest_copy_progress = Some(progress);
                    let now = Instant::now();
                    if should_send_byte_progress(last_copy_progress_sent_at, now) {
                        if let Some(progress) = latest_copy_progress.take() {
                            send_copy_progress(
                                output,
                                task_id,
                                record_index,
                                progress,
                                batch_progress,
                            ).await;
                            last_copy_progress_sent_at = Some(now);
                        }
                    }
                }
            }
            transfer_outcome = &mut transfer => {
                latest_copy_progress = progress::drain_latest_copy_progress(
                    &mut progress_receiver,
                    latest_copy_progress,
                );
                if let Some(progress) = latest_copy_progress.take() {
                    send_copy_progress(
                        output,
                        task_id,
                        record_index,
                        progress,
                        batch_progress,
                    ).await;
                }
                return transfer_outcome;
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

async fn send_copy_progress(
    output: &mut IcedSender<Message>,
    task_id: u64,
    record_index: usize,
    progress: CopyProgress,
    batch_progress: &mut TransferBatchProgress,
) {
    let Some(update) = batch_progress.observe_copy_progress(record_index, &progress) else {
        return;
    };
    send_file_operation_progress(output, task_id, update).await;
}

#[cfg(test)]
mod recoverable_transfer_tests {
    use super::*;
    use crate::operation_queue::{
        FileOperationEnqueueOutcome, FileOperationFinish, FileOperationQueue,
    };
    use iced::futures::StreamExt;

    mod cross_filesystem_recovery;

    #[test]
    fn application_stopping_has_recoverable_interruption_completion() {
        assert!(matches!(
            recoverable_transfer_failure(
                QueuedTransferMode::Copy,
                RecoverableTransferError::FileOperation(
                    file_core::FileError::ApplicationStopping,
                ),
                Vec::new(),
            ),
            FileOperationCompletion::RecoveryInterrupted(_, completed)
                if completed.is_empty()
        ));
    }

    #[test]
    fn recoverable_cancellation_has_typed_completion() {
        assert!(matches!(
            recoverable_transfer_failure(
                QueuedTransferMode::Move,
                RecoverableTransferError::FileOperation(file_core::FileError::Cancelled),
                vec![CompletedTransfer {
                    source: PathBuf::from("source"),
                    target: PathBuf::from("target"),
                }],
            ),
            FileOperationCompletion::Canceled(completed_move_transfers)
                if completed_move_transfers.len() == 1
        ));
    }

    #[tokio::test]
    async fn recoverable_copy_uses_one_manifest_byte_denominator_for_the_batch() {
        let directory = tempfile::tempdir().unwrap();
        let first_source = directory.path().join("first-source");
        let second_source = directory.path().join("second-source");
        let first_target = directory.path().join("first-target");
        let second_target = directory.path().join("second-target");
        tokio::fs::write(&first_source, vec![1_u8; 10])
            .await
            .unwrap();
        tokio::fs::write(&second_source, vec![2_u8; 990])
            .await
            .unwrap();
        let transfers = vec![
            QueuedTransfer::new(first_source, first_target),
            QueuedTransfer::new(second_source, second_target),
        ];
        let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
        let mut queue = FileOperationQueue::new();
        queue.set_store(store.clone());
        let FileOperationEnqueueOutcome::Queued { task_id } =
            queue.enqueue(QueuedFileOperation::Copy {
                transfers: transfers.clone(),
                verification: FileOperationVerification::BasicMetadata,
            })
        else {
            panic!("recoverable copy should enqueue");
        };
        let running = queue.active_subscription().unwrap();
        let (mut output, mut messages) = iced::futures::channel::mpsc::channel(32);

        let completion = run_queued_transfers(
            transfers,
            running.controls,
            task_id,
            &mut output,
            running.store,
            QueuedTransferMode::Copy,
            FileOperationVerification::BasicMetadata,
        )
        .await;
        drop(output);

        assert!(matches!(
            completion,
            FileOperationCompletion::Succeeded(FileOperationOutcome::Copy { .. })
        ));
        let mut byte_updates = Vec::new();
        while let Some(message) = messages.next().await {
            if let Message::FileOperationProgressed(
                id,
                FileOperationProgressUpdate::Bytes {
                    completed_bytes,
                    total_bytes,
                    completed_items,
                    total_items,
                },
            ) = message
            {
                assert_eq!(id, task_id);
                byte_updates.push((completed_bytes, total_bytes, completed_items, total_items));
            }
        }

        assert!(byte_updates.contains(&(10, 1_000, 1, 2)));
        assert_eq!(byte_updates.last(), Some(&(1_000, 1_000, 2, 2)));
        assert!(byte_updates
            .iter()
            .all(|(_, total_bytes, _, total_items)| *total_bytes == 1_000 && *total_items == 2));
    }

    #[tokio::test]
    async fn mismatched_verification_blocks_recovery_without_filesystem_effects() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        tokio::fs::write(&source, b"content").await.unwrap();
        let transfers = vec![QueuedTransfer::new(source.clone(), target.clone())];
        let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
        let mut queue = FileOperationQueue::new();
        queue.set_store(store.clone());
        let FileOperationEnqueueOutcome::Queued { task_id } =
            queue.enqueue(QueuedFileOperation::Copy {
                transfers: transfers.clone(),
                verification: FileOperationVerification::BasicMetadata,
            })
        else {
            panic!("recoverable copy should enqueue");
        };
        let running = queue.active_subscription().unwrap();
        let (mut output, _messages) = iced::futures::channel::mpsc::channel(8);

        let completion = run_queued_transfers(
            transfers,
            running.controls,
            task_id,
            &mut output,
            running.store,
            QueuedTransferMode::Copy,
            FileOperationVerification::Strong,
        )
        .await;
        let FileOperationCompletion::RecoveryBlocked { error, .. } = completion else {
            panic!("verification mismatch must block recovery");
        };
        assert!(error.contains("does not match"));
        assert!(tokio::fs::symlink_metadata(&target).await.is_err());

        assert_eq!(
            queue.finish(task_id, FileOperationFinish::RecoveryBlocked(error)),
            (
                Some(crate::operation_queue::FileOperationTerminalStatus::Failed),
                None
            )
        );
        assert_eq!(
            store.read_task(task_id).unwrap().unwrap().status,
            file_operation_store::StoredTaskStatus::Failed
        );
        assert!(!store
            .read_transfer_recovery(task_id)
            .unwrap()
            .journal_entries
            .is_empty());
    }

    #[tokio::test]
    async fn sqlite_replace_runner_persists_backup_manifest_until_terminal_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        tokio::fs::write(&source, b"replacement").await.unwrap();
        tokio::fs::create_dir(&target).await.unwrap();
        tokio::fs::write(target.join("old.txt"), b"old-target")
            .await
            .unwrap();
        let transfers = vec![QueuedTransfer {
            source,
            target: target.clone(),
            conflict_strategy: TransferConflictStrategy::Replace,
        }];
        let store = TaskQueueStore::new(directory.path().join("replace-state.sqlite")).unwrap();
        let mut queue = FileOperationQueue::new();
        queue.set_store(store.clone());
        let FileOperationEnqueueOutcome::Queued { task_id } =
            queue.enqueue(QueuedFileOperation::Copy {
                transfers: transfers.clone(),
                verification: FileOperationVerification::Strong,
            })
        else {
            panic!("recoverable replace copy should enqueue");
        };
        let running = queue.active_subscription().unwrap();
        let (mut output, _messages) = iced::futures::channel::mpsc::channel(32);

        let completion = run_queued_transfers(
            transfers,
            running.controls,
            task_id,
            &mut output,
            running.store,
            QueuedTransferMode::Copy,
            FileOperationVerification::Strong,
        )
        .await;
        assert!(matches!(completion, FileOperationCompletion::Succeeded(_)));
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"replacement");
        let snapshot = store.read_transfer_recovery(task_id).unwrap();
        assert!(!snapshot.replacement_manifest_entries.is_empty());

        assert_eq!(
            queue.finish(task_id, FileOperationFinish::Succeeded),
            (
                Some(crate::operation_queue::FileOperationTerminalStatus::Completed),
                None
            )
        );
        assert!(store
            .read_transfer_recovery(task_id)
            .unwrap()
            .journal_entries
            .is_empty());
    }

    #[tokio::test]
    async fn sqlite_move_runner_preserves_committed_item_and_history_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let first_source = directory.path().join("first-move-source");
        let second_source = directory.path().join("second-move-source");
        let first_target = directory.path().join("first-move-target");
        let second_target = directory.path().join("second-move-target");
        tokio::fs::write(&first_source, b"first-move")
            .await
            .unwrap();
        tokio::fs::write(&second_source, b"second-move")
            .await
            .unwrap();
        let transfers = vec![
            QueuedTransfer::new(first_source.clone(), first_target.clone()),
            QueuedTransfer::new(second_source.clone(), second_target.clone()),
        ];
        let store = TaskQueueStore::new(directory.path().join("move-state.sqlite")).unwrap();
        let mut original_queue = FileOperationQueue::new();
        original_queue.set_store(store.clone());
        assert!(matches!(
            original_queue.enqueue(QueuedFileOperation::Move {
                transfers: transfers.clone(),
                verification: FileOperationVerification::BasicMetadata,
            }),
            FileOperationEnqueueOutcome::Queued { .. }
        ));
        let task_id = original_queue.tasks()[0].id;
        let running = original_queue.active_subscription().unwrap();
        let mut records = load_recoverable_transfer_records(store.clone(), task_id)
            .await
            .unwrap();
        let journal = TaskQueueTransferJournal {
            store: store.clone(),
            controls: running.controls.clone(),
        };
        let (mut output, _messages) = iced::futures::channel::mpsc::channel(64);
        let mut batch_progress = TransferBatchProgress::new(&records);

        run_recoverable_record_with_progress(
            records.remove(0),
            running.controls,
            &journal,
            task_id,
            &mut output,
            0,
            &mut batch_progress,
        )
        .await
        .unwrap();
        assert!(tokio::fs::symlink_metadata(&first_source).await.is_err());
        drop(original_queue);

        let mut restored_queue = FileOperationQueue::new();
        assert!(restored_queue
            .set_store_and_restore(store.clone())
            .is_none());
        let restored_running = restored_queue.active_subscription().unwrap();
        let completion = run_queued_transfers(
            transfers,
            restored_running.controls,
            task_id,
            &mut output,
            restored_running.store,
            QueuedTransferMode::Move,
            FileOperationVerification::BasicMetadata,
        )
        .await;

        let FileOperationCompletion::Succeeded(FileOperationOutcome::Move {
            transfers,
            history_eligibility: FileOperationHistoryEligibility::Replayable,
        }) = completion
        else {
            panic!("expected resumed move completion");
        };
        assert_eq!(transfers.len(), 2);
        assert_eq!(tokio::fs::read(&first_target).await.unwrap(), b"first-move");
        assert_eq!(
            tokio::fs::read(&second_target).await.unwrap(),
            b"second-move"
        );
        assert!(tokio::fs::symlink_metadata(&second_source).await.is_err());
        assert_eq!(
            restored_queue.finish(task_id, FileOperationFinish::Succeeded),
            (
                Some(crate::operation_queue::FileOperationTerminalStatus::Completed),
                None
            )
        );
        assert!(store
            .read_transfer_recovery(task_id)
            .unwrap()
            .journal_entries
            .is_empty());
    }

    #[tokio::test]
    async fn sqlite_runner_skips_committed_batch_item_and_finishes_remaining_item() {
        let directory = tempfile::tempdir().unwrap();
        let first_source = directory.path().join("first-source");
        let second_source = directory.path().join("second-source");
        let first_target = directory.path().join("first-target");
        let second_target = directory.path().join("second-target");
        tokio::fs::write(&first_source, b"first-original")
            .await
            .unwrap();
        tokio::fs::write(&second_source, b"second-original")
            .await
            .unwrap();
        let transfers = vec![
            QueuedTransfer::new(first_source.clone(), first_target.clone()),
            QueuedTransfer::new(second_source.clone(), second_target.clone()),
        ];
        let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
        let mut original_queue = FileOperationQueue::new();
        original_queue.set_store(store.clone());
        assert!(matches!(
            original_queue.enqueue(QueuedFileOperation::Copy {
                transfers: transfers.clone(),
                verification: FileOperationVerification::BasicMetadata,
            }),
            FileOperationEnqueueOutcome::Queued { .. }
        ));
        let task_id = original_queue.tasks()[0].id;
        let running = original_queue.active_subscription().unwrap();
        let mut records = load_recoverable_transfer_records(store.clone(), task_id)
            .await
            .unwrap();
        let journal = TaskQueueTransferJournal {
            store: store.clone(),
            controls: running.controls.clone(),
        };
        let (mut output, _messages) = iced::futures::channel::mpsc::channel(64);
        let mut batch_progress = TransferBatchProgress::new(&records);

        run_recoverable_record_with_progress(
            records.remove(0),
            running.controls,
            &journal,
            task_id,
            &mut output,
            0,
            &mut batch_progress,
        )
        .await
        .unwrap();
        tokio::fs::write(&first_source, b"first-changed-after-commit")
            .await
            .unwrap();
        drop(original_queue);

        let mut restored_queue = FileOperationQueue::new();
        assert!(restored_queue
            .set_store_and_restore(store.clone())
            .is_none());
        let restored_running = restored_queue.active_subscription().unwrap();
        let completion = run_queued_transfers(
            transfers,
            restored_running.controls,
            task_id,
            &mut output,
            restored_running.store,
            QueuedTransferMode::Copy,
            FileOperationVerification::BasicMetadata,
        )
        .await;

        let FileOperationCompletion::Succeeded(FileOperationOutcome::Copy { transfers }) =
            completion
        else {
            panic!("expected resumed copy completion");
        };
        assert_eq!(transfers.len(), 2);
        assert_eq!(
            tokio::fs::read(&first_target).await.unwrap(),
            b"first-original"
        );
        assert_eq!(
            tokio::fs::read(&second_target).await.unwrap(),
            b"second-original"
        );
        assert_eq!(
            restored_queue.finish(task_id, FileOperationFinish::Succeeded),
            (
                Some(crate::operation_queue::FileOperationTerminalStatus::Completed),
                None
            )
        );
        assert_eq!(
            store.read_task(task_id).unwrap().unwrap().status,
            file_operation_store::StoredTaskStatus::Completed
        );
        assert!(store
            .read_transfer_recovery(task_id)
            .unwrap()
            .journal_entries
            .is_empty());
    }
}
