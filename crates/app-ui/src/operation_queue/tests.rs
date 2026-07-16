use super::*;

fn sample_operation() -> QueuedFileOperation {
    QueuedFileOperation::CreateDirectory {
        parent: PathBuf::from("/tmp"),
    }
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
fn transfer_byte_progress_is_aggregated_by_transfer() {
    let mut progress = FileOperationProgress::pending();

    progress.update(FileOperationProgressUpdate::Bytes {
        bytes_done: 100,
        bytes_total: 100,
        completed_transfers: 0,
        total_transfers: 2,
    });
    assert!(progress.fraction().unwrap() < 0.5);

    progress.update(FileOperationProgressUpdate::Items {
        completed: 1,
        total: 2,
    });
    assert_eq!(progress.fraction(), Some(0.5));

    progress.update(FileOperationProgressUpdate::Bytes {
        bytes_done: 50,
        bytes_total: 100,
        completed_transfers: 1,
        total_transfers: 2,
    });
    let second_transfer_progress = progress.fraction().unwrap();
    assert!(second_transfer_progress > 0.5);
    assert!(second_transfer_progress < 1.0);

    progress.update(FileOperationProgressUpdate::Bytes {
        bytes_done: 100,
        bytes_total: 100,
        completed_transfers: 1,
        total_transfers: 2,
    });
    assert!(progress.fraction().unwrap() < 1.0);

    progress.update(FileOperationProgressUpdate::Items {
        completed: 2,
        total: 2,
    });
    assert_eq!(progress.fraction(), Some(1.0));
}

#[test]
fn indeterminate_failed_task_does_not_display_as_complete() {
    let progress = FileOperationProgress::pending();

    assert_eq!(progress.display_fraction(), 0.0);
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

    let (terminal_status, error) = queue.finish(task_id, Ok(()));

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

    assert!(queue.cancel_all().is_none());
    assert!(queue.tasks().is_empty());
    assert!(store.read_tasks().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn pending_and_terminal_tasks_reject_completion_messages() {
    let mut queue = FileOperationQueue::new();
    queue.enqueue(sample_operation());
    queue.enqueue(sample_operation());
    let running_task_id = queue.tasks()[0].id;
    let pending_task_id = queue.tasks()[1].id;

    assert_eq!(queue.finish(pending_task_id, Ok(())), (None, None));
    assert_eq!(
        queue.tasks()[1].status,
        FileOperationStatus::Pending,
        "pending task must not accept another task's completion"
    );
    assert_eq!(
        queue.finish(running_task_id, Ok(())).0,
        Some(FileOperationTerminalStatus::Completed)
    );
    assert_eq!(queue.finish(running_task_id, Ok(())), (None, None));
}

#[test]
fn late_cancel_request_keeps_successful_completion_semantics() {
    let mut queue = FileOperationQueue::new();
    queue.enqueue(sample_operation());
    let task_id = queue.tasks()[0].id;
    queue.cancel(task_id);

    assert_eq!(
        queue.finish(task_id, Ok(())),
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
        failed_queue.finish(failed_task_id, Err("create failed".to_owned())),
        (Some(FileOperationTerminalStatus::Failed), None)
    );
    assert_eq!(
        failed_queue.finish(failed_task_id, Err("duplicate failure".to_owned())),
        (None, None)
    );
    assert_eq!(
        canceled_queue.finish(canceled_task_id, Err("cancelled".to_owned())),
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
