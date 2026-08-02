use super::*;
use file_core::TransferCheckpoint;
use file_operation_store::StoredProgress;

fn sample_operation() -> QueuedFileOperation {
    QueuedFileOperation::CreateDirectory {
        parent: PathBuf::from("/tmp"),
    }
}

fn sample_transfer_operation() -> QueuedFileOperation {
    QueuedFileOperation::Copy {
        transfers: vec![QueuedTransfer::new(
            PathBuf::from("/tmp/source"),
            PathBuf::from("/tmp/target"),
        )],
        verification: FileOperationVerification::BasicMetadata,
    }
}

fn completed_checkpoint(path: &std::path::Path) -> TransferCheckpoint {
    TransferCheckpoint::Completed(file_core::CompletedTarget {
        path: path.to_path_buf(),
        identity: file_core::FileIdentity {
            device: 1,
            inode: 2,
            object_kind: file_core::FileObjectKind::RegularFile,
            size: 3,
            modified_seconds: 4,
            modified_nanoseconds: 5,
            changed_seconds: 6,
            changed_nanoseconds: 7,
            symbolic_link_target: None,
        },
        fingerprint: file_core::ObjectFingerprint([8; 32]),
    })
}

fn persist_terminal_checkpoint(
    store: &TaskQueueStore,
    task_id: u64,
    checkpoint: &TransferCheckpoint,
) {
    let kind = match checkpoint {
        TransferCheckpoint::Completed(_) => {
            file_operation_store::StoredTransferCheckpointKind::Completed
        }
        TransferCheckpoint::Failed { .. } => {
            file_operation_store::StoredTransferCheckpointKind::Failed
        }
        _ => panic!("test terminal checkpoint must be completed or failed"),
    };
    let checkpoint = file_operation_store::StoredTransferCheckpoint::new(
        kind,
        serde_json::to_string(checkpoint).unwrap(),
    )
    .unwrap();
    store
        .compare_and_swap_transfer_checkpoint(
            task_id,
            &file_operation_store::StoredTransferWorkKey::top_level(0),
            0,
            &checkpoint,
        )
        .unwrap();
}

#[test]
fn copy_and_move_fail_closed_without_operation_store() {
    let mut queue = FileOperationQueue::new();

    let FileOperationEnqueueOutcome::Rejected { error } =
        queue.enqueue(sample_transfer_operation())
    else {
        panic!("transfer without a store must be rejected");
    };

    assert!(error.contains("storage is unavailable"));
    assert!(queue.tasks().is_empty());
}

#[test]
fn recoverable_transfer_enqueue_atomically_creates_journal() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut queue = FileOperationQueue::new();
    queue.set_store(store.clone());

    assert!(queue.enqueue(sample_transfer_operation()).error().is_none());

    let task = &queue.tasks()[0];
    let snapshot = store.read_transfer_recovery(task.id).unwrap();
    assert_eq!(snapshot.journal_entries.len(), 1);
    assert_eq!(
        snapshot.journal_entries[0].checkpoint.kind,
        file_operation_store::StoredTransferCheckpointKind::AwaitingManifest
    );
    assert!(queue.active_subscription().unwrap().store.is_some());
}

#[test]
fn terminal_completion_releases_recoverable_runner_lease() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut queue = FileOperationQueue::new();
    queue.set_store(store.clone());
    assert!(queue.enqueue(sample_transfer_operation()).error().is_none());
    let task_id = queue.tasks()[0].id;
    persist_terminal_checkpoint(
        &store,
        task_id,
        &completed_checkpoint(std::path::Path::new("/tmp/target")),
    );

    assert_eq!(
        queue.finish(task_id, FileOperationFinish::Succeeded).0,
        Some(FileOperationTerminalStatus::Completed)
    );
    assert!(store
        .try_acquire_recoverable_task_runner(task_id)
        .unwrap()
        .is_some());
    assert_eq!(queue.tasks().len(), 1);
}

#[test]
fn second_process_does_not_restore_task_with_active_runner_lease() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut owner = FileOperationQueue::new();
    owner.set_store(store.clone());
    assert!(owner.enqueue(sample_transfer_operation()).error().is_none());
    let task_id = owner.tasks()[0].id;

    let mut observer = FileOperationQueue::new();
    assert!(observer.set_store_and_restore(store.clone()).is_none());

    assert!(observer.tasks().is_empty());
    assert_eq!(
        store.read_task(task_id).unwrap().unwrap().status,
        StoredTaskStatus::Running
    );
    assert_eq!(owner.tasks().len(), 1);
}

#[test]
fn restore_coordinator_keeps_restart_task_claims_in_one_process() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut original = FileOperationQueue::new();
    original.set_store(store.clone());
    assert!(original
        .enqueue(sample_transfer_operation())
        .error()
        .is_none());
    assert!(original
        .enqueue(sample_transfer_operation())
        .error()
        .is_none());
    assert!(original.prepare_for_shutdown().is_none());
    let coordinator = store
        .try_acquire_recoverable_restore_coordinator()
        .unwrap()
        .unwrap();

    let mut losing_process = FileOperationQueue::new();
    assert!(losing_process
        .set_store_and_restore(store.clone())
        .is_none());
    assert!(losing_process.tasks().is_empty());
    drop(coordinator);

    let mut recovery_owner = FileOperationQueue::new();
    assert!(recovery_owner.set_store_and_restore(store).is_none());
    assert_eq!(recovery_owner.tasks().len(), 2);
}

#[test]
fn restore_reloads_task_state_after_coordinator_acquisition() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut original = FileOperationQueue::new();
    original.set_store(store.clone());
    assert!(original
        .enqueue(sample_transfer_operation())
        .error()
        .is_none());
    let task_id = original.tasks()[0].id;
    let stale_snapshot = store.read_tasks().unwrap();
    assert_eq!(stale_snapshot[0].status, StoredTaskStatus::Running);
    store
        .update_task_state(
            task_id,
            StoredTaskStatus::Failed,
            StoredProgress::pending(),
            Some("recovery blocked in owning process"),
        )
        .unwrap();
    drop(original);

    let mut restored = FileOperationQueue::new();
    assert!(restored.set_store_and_restore(store).is_none());
    assert!(restored.tasks().is_empty());
}

#[test]
fn current_recovery_journal_is_requeued_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut original = FileOperationQueue::new();
    original.set_store(store.clone());
    assert!(original
        .enqueue(sample_transfer_operation())
        .error()
        .is_none());
    let task_id = original.tasks()[0].id;
    drop(original);

    let mut restored = FileOperationQueue::new();
    assert!(restored.set_store_and_restore(store.clone()).is_none());

    assert_eq!(restored.tasks().len(), 1);
    assert_eq!(restored.tasks()[0].id, task_id);
    assert_eq!(restored.tasks()[0].status, FileOperationStatus::Running);
}

#[test]
fn paused_and_canceling_recovery_states_preserve_controls() {
    for (stored_status, expected_status, cancellation_expected) in [
        (StoredTaskStatus::Paused, FileOperationStatus::Paused, false),
        (
            StoredTaskStatus::Canceling,
            FileOperationStatus::Canceling,
            true,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
        let mut original = FileOperationQueue::new();
        original.set_store(store.clone());
        assert!(original
            .enqueue(sample_transfer_operation())
            .error()
            .is_none());
        let task_id = original.tasks()[0].id;
        store.update_status(task_id, stored_status).unwrap();
        drop(original);

        let mut restored = FileOperationQueue::new();
        assert!(restored.set_store_and_restore(store.clone()).is_none());

        assert_eq!(restored.tasks()[0].status, expected_status);
        assert_eq!(
            restored
                .active_subscription()
                .unwrap()
                .controls
                .cancellation_token()
                .is_cancelled(),
            cancellation_expected
        );
    }
}

#[test]
fn recovery_pending_transfer_with_unfinished_checkpoint_is_requeued_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut original = FileOperationQueue::new();
    original.set_store(store.clone());
    assert!(original
        .enqueue(sample_transfer_operation())
        .error()
        .is_none());
    let task_id = original.tasks()[0].id;
    store
        .update_task_state(
            task_id,
            StoredTaskStatus::RecoveryPending,
            StoredProgress::pending(),
            Some("transfer journal failed"),
        )
        .unwrap();
    drop(original);

    let mut restored = FileOperationQueue::new();
    assert!(restored.set_store_and_restore(store.clone()).is_none());

    assert_eq!(restored.tasks().len(), 1);
    assert_eq!(restored.tasks()[0].status, FileOperationStatus::Running);
}

#[test]
fn pending_recoverable_cancel_uses_recovery_runner() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut queue = FileOperationQueue::new();
    queue.set_store(store.clone());
    queue.enqueue(sample_operation());
    queue.enqueue(sample_transfer_operation());
    let transfer_id = queue.tasks()[1].id;
    assert_eq!(queue.tasks()[1].status, FileOperationStatus::Pending);

    assert!(queue.cancel(transfer_id).is_none());

    assert_eq!(queue.tasks()[1].status, FileOperationStatus::Canceling);
    assert!(queue.tasks()[1].cancel.is_cancelled());
    assert_eq!(
        store.read_task(transfer_id).unwrap().unwrap().status,
        StoredTaskStatus::Canceling
    );
    assert!(!store
        .read_transfer_recovery(transfer_id)
        .unwrap()
        .journal_entries
        .is_empty());
}

#[test]
fn shutdown_preserves_nonterminal_recoverable_task() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut queue = FileOperationQueue::new();
    queue.set_store(store.clone());
    queue.enqueue(sample_transfer_operation());
    let task_id = queue.tasks()[0].id;

    assert!(queue.prepare_for_shutdown().is_none());

    assert!(queue.tasks().is_empty());
    assert_eq!(
        store.read_task(task_id).unwrap().unwrap().status,
        StoredTaskStatus::Running
    );
    assert!(!store
        .read_transfer_recovery(task_id)
        .unwrap()
        .journal_entries
        .is_empty());

    let mut restored = FileOperationQueue::new();
    assert!(restored.set_store_and_restore(store.clone()).is_none());
    assert_eq!(restored.tasks()[0].status, FileOperationStatus::Running);
}

#[test]
fn recovery_interruption_persists_distinct_restart_state() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut queue = FileOperationQueue::new();
    queue.set_store(store.clone());
    assert!(queue.enqueue(sample_transfer_operation()).error().is_none());
    let task_id = queue.tasks()[0].id;

    assert_eq!(
        queue.finish(
            task_id,
            FileOperationFinish::RecoveryInterrupted("checkpoint write failed".to_owned()),
        ),
        (Some(FileOperationTerminalStatus::Failed), None)
    );
    assert_eq!(
        store.read_task(task_id).unwrap().unwrap().status,
        StoredTaskStatus::RecoveryPending
    );
    drop(queue);

    let mut restored = FileOperationQueue::new();
    assert!(restored.set_store_and_restore(store.clone()).is_none());
    assert_eq!(restored.tasks()[0].status, FileOperationStatus::Running);
}

#[test]
fn late_cancel_does_not_mask_recorded_recoverable_failure() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("late-cancel-state.sqlite")).unwrap();
    let mut queue = FileOperationQueue::new();
    queue.set_store(store.clone());
    assert!(queue.enqueue(sample_transfer_operation()).error().is_none());
    let task_id = queue.tasks()[0].id;
    persist_terminal_checkpoint(
        &store,
        task_id,
        &TransferCheckpoint::Failed {
            final_target: None,
            diagnostic: "source changed".to_owned(),
        },
    );
    assert!(queue.cancel(task_id).is_none());

    assert_eq!(
        queue.finish(
            task_id,
            FileOperationFinish::Failed("source changed".to_owned()),
        ),
        (Some(FileOperationTerminalStatus::Failed), None)
    );
    assert_eq!(
        store.read_task(task_id).unwrap().unwrap().status,
        StoredTaskStatus::Failed
    );
}

#[test]
fn cancellation_checkpoint_interruption_stays_recoverable() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("cancel-state.sqlite")).unwrap();
    let mut queue = FileOperationQueue::new();
    queue.set_store(store.clone());
    assert!(queue.enqueue(sample_transfer_operation()).error().is_none());
    let task_id = queue.tasks()[0].id;
    assert!(queue.cancel(task_id).is_none());
    assert_eq!(queue.tasks()[0].status, FileOperationStatus::Canceling);

    assert_eq!(
        queue.finish(
            task_id,
            FileOperationFinish::RecoveryInterrupted("cancel checkpoint failed".to_owned()),
        ),
        (Some(FileOperationTerminalStatus::Failed), None)
    );
    assert_eq!(
        store.read_task(task_id).unwrap().unwrap().status,
        StoredTaskStatus::RecoveryPending
    );
    assert!(!store
        .read_transfer_recovery(task_id)
        .unwrap()
        .journal_entries
        .is_empty());
}

#[test]
fn terminal_recoverable_failure_discards_terminal_recovery_details() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut queue = FileOperationQueue::new();
    queue.set_store(store.clone());
    queue.enqueue(sample_transfer_operation());
    let task_id = queue.tasks()[0].id;
    persist_terminal_checkpoint(
        &store,
        task_id,
        &TransferCheckpoint::Failed {
            final_target: None,
            diagnostic: "source changed".to_owned(),
        },
    );

    assert_eq!(
        queue.finish(
            task_id,
            FileOperationFinish::Failed("source changed".to_owned()),
        ),
        (Some(FileOperationTerminalStatus::Failed), None)
    );
    assert!(store
        .read_transfer_recovery(task_id)
        .unwrap()
        .journal_entries
        .is_empty());
}

#[test]
fn blocked_recovery_preserves_details_without_automatic_restart() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut queue = FileOperationQueue::new();
    queue.set_store(store.clone());
    queue.enqueue(sample_transfer_operation());
    let task_id = queue.tasks()[0].id;

    assert_eq!(
        queue.finish(
            task_id,
            FileOperationFinish::RecoveryBlocked("owner marker changed".to_owned()),
        ),
        (Some(FileOperationTerminalStatus::Failed), None)
    );
    assert_eq!(
        store.read_task(task_id).unwrap().unwrap().status,
        StoredTaskStatus::Failed
    );
    assert!(!store
        .read_transfer_recovery(task_id)
        .unwrap()
        .journal_entries
        .is_empty());

    let mut restored = FileOperationQueue::new();
    assert!(restored.set_store_and_restore(store.clone()).is_none());
    assert!(restored.tasks().is_empty());
}

#[test]
fn terminal_failed_transfer_with_unfinished_checkpoint_is_not_requeued() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut original = FileOperationQueue::new();
    original.set_store(store.clone());
    assert!(original
        .enqueue(sample_transfer_operation())
        .error()
        .is_none());
    let task_id = original.tasks()[0].id;
    store
        .update_task_state(
            task_id,
            StoredTaskStatus::Failed,
            StoredProgress::pending(),
            Some("source changed"),
        )
        .unwrap();
    drop(original);

    let mut restored = FileOperationQueue::new();
    assert!(restored.set_store_and_restore(store.clone()).is_none());

    assert!(restored.tasks().is_empty());
}

#[test]
fn failed_transfer_with_terminal_checkpoint_is_not_requeued() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let mut original = FileOperationQueue::new();
    original.set_store(store.clone());
    assert!(original
        .enqueue(sample_transfer_operation())
        .error()
        .is_none());
    let task_id = original.tasks()[0].id;
    persist_terminal_checkpoint(
        &store,
        task_id,
        &completed_checkpoint(std::path::Path::new("/tmp/target")),
    );
    store
        .update_task_state(
            task_id,
            StoredTaskStatus::Failed,
            StoredProgress::pending(),
            Some("late task state write"),
        )
        .unwrap();
    drop(original);

    let mut restored = FileOperationQueue::new();
    assert!(restored.set_store_and_restore(store.clone()).is_none());

    assert!(restored.tasks().is_empty());
}

#[test]
fn legacy_transfer_is_marked_failed_instead_of_requeued() {
    let directory = tempfile::tempdir().unwrap();
    let store = TaskQueueStore::new(directory.path().join("state.sqlite")).unwrap();
    let task_id = store
        .insert_task(&StoredOperation::Copy {
            transfers: vec![file_operation_store::StoredTransfer {
                source: file_operation_store::StoredPath::from_path(
                    PathBuf::from("/tmp/source").as_path(),
                ),
                target: file_operation_store::StoredPath::from_path(
                    PathBuf::from("/tmp/target").as_path(),
                ),
                conflict_strategy: file_operation_store::StoredTransferConflictStrategy::Fail,
            }],
            verification: file_operation_store::StoredFileOperationVerification::BasicMetadata,
            recovery_version: None,
        })
        .unwrap();
    let mut restored = FileOperationQueue::new();

    assert!(restored.set_store_and_restore(store.clone()).is_none());

    assert!(restored.tasks().is_empty());
    assert_eq!(
        store.read_task(task_id).unwrap().unwrap().status,
        StoredTaskStatus::Failed
    );
}

#[test]
fn local_and_persisted_task_ids_do_not_collide() {
    let mut queue = FileOperationQueue::new();
    queue.enqueue(sample_operation());
    let local_id = queue.tasks()[0].id;

    let root = std::env::temp_dir().join(format!(
        "app-ui-operation-queue-test-{}-{}",
        std::process::id(),
        local_id
    ));
    let store = TaskQueueStore::new(root.join("state.sqlite")).unwrap();
    queue.set_store(store);
    queue.enqueue(sample_operation());

    let persisted_id = queue.tasks()[1].id;
    assert!(local_id > i64::MAX as u64);
    assert!(persisted_id <= i64::MAX as u64);
    assert_ne!(local_id, persisted_id);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn finished_tasks_stay_until_queue_is_cleared() {
    let root = std::env::temp_dir().join(format!(
        "app-ui-operation-queue-finish-test-{}",
        std::process::id()
    ));
    let store = TaskQueueStore::new(root.join("state.sqlite")).unwrap();
    let mut queue = FileOperationQueue::new();
    queue.set_store(store.clone());
    queue.enqueue(sample_operation());
    let task_id = queue.tasks()[0].id;

    let (terminal_status, error) = queue.finish(task_id, FileOperationFinish::Succeeded);

    assert_eq!(
        terminal_status,
        Some(FileOperationTerminalStatus::Completed)
    );
    assert!(error.is_none());
    assert_eq!(queue.tasks().len(), 1);
    assert_eq!(queue.tasks()[0].status, FileOperationStatus::Completed);
    assert_eq!(queue.tasks()[0].progress.fraction(), Some(1.0));
    assert!(!queue.has_active_task());
    assert_eq!(queue.unread_count(), 1);
    assert_eq!(
        store.read_task(task_id).unwrap().unwrap().status,
        StoredTaskStatus::Completed
    );

    queue.open_panel();
    assert_eq!(queue.unread_count(), 0);

    assert!(queue.prepare_for_shutdown().is_none());
    assert!(queue.tasks().is_empty());
    assert!(store.read_tasks().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn completed_warning_keeps_completed_status_and_marks_the_task_for_attention() {
    let mut queue = FileOperationQueue::new();
    queue.enqueue(sample_operation());
    let task_id = queue.tasks()[0].id;

    let (terminal_status, error) = queue.finish(
        task_id,
        FileOperationFinish::SucceededWithWarning("undo tracking failed".to_owned()),
    );

    assert_eq!(
        terminal_status,
        Some(FileOperationTerminalStatus::Completed)
    );
    assert!(error.is_none());
    assert_eq!(queue.tasks()[0].status, FileOperationStatus::Completed);
    assert_eq!(
        queue.tasks()[0].completion_warning.as_deref(),
        Some("undo tracking failed")
    );
    assert!(queue.has_unread_warning_task());
}

#[test]
fn pending_and_terminal_tasks_reject_completion_messages() {
    let mut queue = FileOperationQueue::new();
    queue.enqueue(sample_operation());
    queue.enqueue(sample_operation());
    let running_task_id = queue.tasks()[0].id;
    let pending_task_id = queue.tasks()[1].id;

    assert_eq!(
        queue.finish(pending_task_id, FileOperationFinish::Succeeded),
        (None, None)
    );
    assert_eq!(
        queue.tasks()[1].status,
        FileOperationStatus::Pending,
        "pending task must not accept another task's completion"
    );
    assert_eq!(
        queue
            .finish(running_task_id, FileOperationFinish::Succeeded)
            .0,
        Some(FileOperationTerminalStatus::Completed)
    );
    assert_eq!(
        queue.finish(running_task_id, FileOperationFinish::Succeeded),
        (None, None)
    );
}

#[test]
fn late_cancel_request_keeps_successful_completion_semantics() {
    let mut queue = FileOperationQueue::new();
    queue.enqueue(sample_operation());
    let task_id = queue.tasks()[0].id;
    queue.cancel(task_id);

    assert_eq!(
        queue.finish(task_id, FileOperationFinish::Succeeded),
        (Some(FileOperationTerminalStatus::Completed), None)
    );
    assert_eq!(queue.tasks()[0].status, FileOperationStatus::Completed);
}

#[test]
fn failed_operation_records_once_but_cancel_completion_does_not() {
    RECORDED_FILE_OPERATION_FAILURES.with(|count| count.set(0));
    let mut failed_queue = FileOperationQueue::new();
    failed_queue.enqueue(sample_operation());
    let failed_task_id = failed_queue.tasks()[0].id;
    let mut canceled_queue = FileOperationQueue::new();
    canceled_queue.enqueue(sample_operation());
    let canceled_task_id = canceled_queue.tasks()[0].id;
    canceled_queue.cancel(canceled_task_id);

    assert_eq!(
        failed_queue.finish(
            failed_task_id,
            FileOperationFinish::Failed("create failed".to_owned()),
        ),
        (Some(FileOperationTerminalStatus::Failed), None)
    );
    assert_eq!(
        failed_queue.finish(
            failed_task_id,
            FileOperationFinish::Failed("duplicate failure".to_owned()),
        ),
        (None, None)
    );
    assert_eq!(
        canceled_queue.finish(
            canceled_task_id,
            FileOperationFinish::Failed("cancelled".to_owned()),
        ),
        (Some(FileOperationTerminalStatus::Canceled), None)
    );

    RECORDED_FILE_OPERATION_FAILURES.with(|count| assert_eq!(count.get(), 1));
    assert_eq!(failed_queue.tasks()[0].status, FileOperationStatus::Failed);
    assert_eq!(
        failed_queue.tasks()[0].error.as_deref(),
        Some("create failed")
    );
    assert_eq!(
        canceled_queue.tasks()[0].status,
        FileOperationStatus::Canceled
    );
    assert_eq!(canceled_queue.tasks()[0].error, None);
}
