use super::*;

#[tokio::test]
async fn complete_replacement_backup_becomes_visible_when_committed_target_changes() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::write(&source, b"replacement").await.unwrap();
    fs::write(&target, b"previous-target").await.unwrap();
    let request = transfer_request(
        source,
        target.clone(),
        RecoverableTransferOperation::Copy,
        TransferConflictStrategy::Replace,
    );
    let journal = MemoryJournal::new(87, TransferWorkKey::top_level(0), None);
    let mut record = journal.record(request.clone());
    loop {
        if matches!(record.checkpoint, TransferCheckpoint::TargetCommitted(_)) {
            break;
        }
        assert!(matches!(
            advance_recoverable_transfer(&mut record, &journal, &running_transfer_options())
                .await
                .unwrap(),
            TransferAdvance::Continue
        ));
        record = journal.record(request.clone());
    }

    fs::write(&target, b"externally-modified").await.unwrap();
    let error = run_recoverable_transfer(record, &journal, running_transfer_options())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        RecoverableTransferError::RecoveryBlocked { .. }
    ));
    assert_eq!(fs::read(&target).await.unwrap(), b"externally-modified");
    assert_eq!(
        fs::read(target.with_file_name("target.recovered1"))
            .await
            .unwrap(),
        b"previous-target"
    );
    assert!(matches!(
        journal.record(request).checkpoint,
        TransferCheckpoint::TargetCommitted(_)
    ));
}

#[tokio::test]
async fn replacement_backup_modified_after_deletion_intent_is_preserved() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::write(&source, b"replacement").await.unwrap();
    fs::create_dir(&target).await.unwrap();
    fs::write(target.join("old.txt"), b"old-target")
        .await
        .unwrap();
    let request = transfer_request(
        source,
        target.clone(),
        RecoverableTransferOperation::Copy,
        TransferConflictStrategy::Replace,
    );
    let journal = MemoryJournal::new(1_500, TransferWorkKey::top_level(0), None);
    let mut record = journal.record(request);

    for _ in 0..12 {
        if matches!(
            &record.checkpoint,
            TransferCheckpoint::TargetCommitted(committed)
                if committed.backup_cleanup_intent.is_some()
        ) {
            break;
        }
        assert_eq!(
            advance_recoverable_transfer(&mut record, &journal, &running_transfer_options())
                .await
                .unwrap(),
            TransferAdvance::Continue
        );
    }
    let TransferCheckpoint::TargetCommitted(committed) = &record.checkpoint else {
        panic!("replace did not reach backup cleanup intent");
    };
    let artifact = committed.artifact.as_ref().unwrap();
    let intent = committed.backup_cleanup_intent.as_ref().unwrap();
    let backup_entry = artifact
        .plan
        .backup_path()
        .join(&intent.entry.relative_path);
    let deletion_slot = artifact.plan.payload_path();
    fs::rename(&backup_entry, &deletion_slot).await.unwrap();
    fs::write(&deletion_slot, b"externally-modified")
        .await
        .unwrap();

    assert!(matches!(
        run_recoverable_transfer(record, &journal, running_transfer_options()).await,
        Err(RecoverableTransferError::RecoveryBlocked { .. })
    ));

    assert_eq!(fs::read(&target).await.unwrap(), b"replacement");
    assert_eq!(
        fs::read(&backup_entry).await.unwrap(),
        b"externally-modified"
    );
    assert!(fs::symlink_metadata(&deletion_slot).await.is_err());
    assert!(matches!(
        journal
            .record(transfer_request(
                directory.path().join("source"),
                target,
                RecoverableTransferOperation::Copy,
                TransferConflictStrategy::Replace,
            ))
            .checkpoint,
        TransferCheckpoint::TargetCommitted(_)
    ));
}
