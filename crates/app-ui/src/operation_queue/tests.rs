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

    let (finished, error) = queue.finish(task_id, Ok(()));

    assert!(finished);
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
