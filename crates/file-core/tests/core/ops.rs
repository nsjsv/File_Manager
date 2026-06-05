use super::*;

#[tokio::test]
async fn create_directory_and_rename_path_update_filesystem() {
    let dir = tempdir().unwrap();
    let folder = dir.path().join("folder");

    let created = create_directory(&folder).await.unwrap();
    assert!(created.is_dir());

    let renamed = rename_path(&folder, "renamed").await.unwrap();
    assert_eq!(renamed, dir.path().join("renamed"));
    assert!(renamed.is_dir());
    assert!(!folder.exists());
}

#[tokio::test]
async fn create_empty_file_writes_zero_length_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("empty.txt");

    let created = create_empty_file(&file).await.unwrap();

    assert_eq!(created, file);
    assert!(created.is_file());
    assert_eq!(fs::metadata(&created).unwrap().len(), 0);
}

#[tokio::test]
async fn create_file_with_contents_writes_exact_bytes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("clipboard.bin");

    let created = create_file_with_contents(&file, b"clipboard bytes")
        .await
        .unwrap();

    assert_eq!(created, file);
    assert_eq!(fs::read(&created).unwrap(), b"clipboard bytes");
}

#[tokio::test]
async fn create_empty_file_existing_path_returns_structured_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, b"taken").unwrap();

    let error = create_empty_file(&file).await.unwrap_err();

    match error {
        FileError::CreateFile { path, source } => {
            assert_eq!(path, file);
            assert_eq!(source.kind(), io::ErrorKind::AlreadyExists);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn copy_and_move_file_operations_update_filesystem() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let copied = dir.path().join("copied.txt");
    let moved = dir.path().join("moved.txt");
    fs::write(&source, b"copy me").unwrap();

    copy_path(
        &source,
        &copied,
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(fs::read(&copied).unwrap(), b"copy me");

    move_path(
        &copied,
        &moved,
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await
    .unwrap();
    assert!(!copied.exists());
    assert_eq!(fs::read(&moved).unwrap(), b"copy me");
}

#[tokio::test]
async fn copy_conflict_replace_overwrites_existing_file() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    fs::write(&source, b"new").unwrap();
    fs::write(&target, b"old").unwrap();

    copy_path_with_options(
        &source,
        &target,
        FileTransferOptions::running(tokio_util::sync::CancellationToken::new())
            .with_conflict_strategy(TransferConflictStrategy::Replace),
    )
    .await
    .unwrap();

    assert_eq!(fs::read(&target).unwrap(), b"new");
}

#[tokio::test]
async fn copy_conflict_skip_preserves_existing_file() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    fs::write(&source, b"new").unwrap();
    fs::write(&target, b"old").unwrap();

    copy_path_with_options(
        &source,
        &target,
        FileTransferOptions::running(tokio_util::sync::CancellationToken::new())
            .with_conflict_strategy(TransferConflictStrategy::Skip),
    )
    .await
    .unwrap();

    assert_eq!(fs::read(&target).unwrap(), b"old");
}

#[tokio::test]
async fn copy_conflict_keep_both_writes_alternate_path() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    let alternate = dir.path().join("target.txt.copy1");
    fs::write(&source, b"new").unwrap();
    fs::write(&target, b"old").unwrap();

    copy_path_with_options(
        &source,
        &target,
        FileTransferOptions::running(tokio_util::sync::CancellationToken::new())
            .with_conflict_strategy(TransferConflictStrategy::KeepBoth),
    )
    .await
    .unwrap();

    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(fs::read(&alternate).unwrap(), b"new");
}

#[tokio::test]
async fn copy_conflict_merge_directory_adds_missing_children() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let target = dir.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("same.txt"), b"new").unwrap();
    fs::write(source.join("new.txt"), b"added").unwrap();
    fs::write(target.join("same.txt"), b"old").unwrap();

    copy_path_with_options(
        &source,
        &target,
        FileTransferOptions::running(tokio_util::sync::CancellationToken::new())
            .with_conflict_strategy(TransferConflictStrategy::Merge),
    )
    .await
    .unwrap();

    assert_eq!(fs::read(target.join("same.txt")).unwrap(), b"old");
    assert_eq!(fs::read(target.join("new.txt")).unwrap(), b"added");
}

#[tokio::test]
async fn move_conflict_keep_both_moves_to_alternate_path() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    let alternate = dir.path().join("target.txt.copy1");
    fs::write(&source, b"new").unwrap();
    fs::write(&target, b"old").unwrap();

    move_path_with_options(
        &source,
        &target,
        FileTransferOptions::running(tokio_util::sync::CancellationToken::new())
            .with_conflict_strategy(TransferConflictStrategy::KeepBoth),
    )
    .await
    .unwrap();

    assert!(!source.exists());
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(fs::read(&alternate).unwrap(), b"new");
}

#[tokio::test]
async fn transfer_conflict_check_ignores_missing_target() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    fs::write(&source, b"source").unwrap();

    let conflicts =
        check_transfer_conflicts(vec![TransferConflictCheck::new(source, target)]).await;

    assert!(conflicts.is_empty());
}

#[tokio::test]
async fn transfer_conflict_check_detects_file_conflict() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    fs::write(&source, b"new bytes").unwrap();
    fs::write(&target, b"old").unwrap();

    let conflicts = check_transfer_conflicts(vec![TransferConflictCheck::new(
        source.clone(),
        target.clone(),
    )])
    .await;

    assert_eq!(conflicts.len(), 1);
    let conflict = &conflicts[0];
    assert_eq!(conflict.source, source);
    assert_eq!(conflict.target, target);
    assert!(!conflict.can_merge());
    assert!(!conflict.source_metadata.is_directory);
    assert!(!conflict.target_metadata.is_directory);
    assert_eq!(conflict.source_metadata.len, 9);
    assert_eq!(conflict.target_metadata.len, 3);
}

#[tokio::test]
async fn transfer_conflict_check_marks_directories_mergeable() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let target = dir.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();

    let conflicts =
        check_transfer_conflicts(vec![TransferConflictCheck::new(source, target)]).await;

    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].can_merge());
    assert!(conflicts[0].source_metadata.is_directory);
    assert!(conflicts[0].target_metadata.is_directory);
}

#[tokio::test]
async fn transfer_target_availability_and_candidate_use_copy_suffix() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("target.txt");
    let copy1 = dir.path().join("target.txt.copy1");
    let copy2 = dir.path().join("target.txt.copy2");
    fs::write(&target, b"old").unwrap();
    fs::write(&copy1, b"old copy").unwrap();

    assert!(!is_transfer_target_available(&target).await.unwrap());
    assert!(is_transfer_target_available(&copy2).await.unwrap());
    assert_eq!(
        available_transfer_target_path(&target).await.unwrap(),
        copy2
    );
}

#[tokio::test]
async fn move_conflict_merge_directory_moves_missing_children() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let target = dir.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("same.txt"), b"new").unwrap();
    fs::write(source.join("new.txt"), b"added").unwrap();
    fs::write(target.join("same.txt"), b"old").unwrap();

    move_path_with_options(
        &source,
        &target,
        FileTransferOptions::running(tokio_util::sync::CancellationToken::new())
            .with_conflict_strategy(TransferConflictStrategy::Merge),
    )
    .await
    .unwrap();

    assert_eq!(fs::read(target.join("same.txt")).unwrap(), b"old");
    assert_eq!(fs::read(target.join("new.txt")).unwrap(), b"added");
    assert!(source.join("same.txt").exists());
    assert!(!source.join("new.txt").exists());
}

#[tokio::test]
async fn copy_directory_recursively_copies_children() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let nested = source.join("nested");
    let empty = source.join("empty");
    let copied = dir.path().join("copied");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&nested).unwrap();
    fs::create_dir(&empty).unwrap();
    fs::write(source.join("root.txt"), b"root").unwrap();
    fs::write(nested.join("child.txt"), b"child").unwrap();

    copy_path(
        &source,
        &copied,
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await
    .unwrap();

    assert!(copied.join("empty").is_dir());
    assert_eq!(fs::read(copied.join("root.txt")).unwrap(), b"root");
    assert_eq!(
        fs::read(copied.join("nested").join("child.txt")).unwrap(),
        b"child"
    );
}

#[tokio::test]
async fn copy_directory_rejects_target_inside_source() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    let copied = source.join("copied");
    fs::create_dir(&source).unwrap();

    let error = copy_path(
        &source,
        &copied,
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await
    .unwrap_err();

    match error {
        FileError::InvalidInput { path, message } => {
            assert_eq!(path, copied);
            assert_eq!(message, "cannot copy a directory into itself");
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(!source.join("copied").exists());
}

#[tokio::test]
async fn copy_operation_honors_pre_cancelled_token() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let copied = dir.path().join("copied.txt");
    fs::write(&source, b"copy me").unwrap();
    let token = tokio_util::sync::CancellationToken::new();
    token.cancel();

    let error = copy_path(&source, &copied, token, None).await.unwrap_err();

    assert!(matches!(error, FileError::Cancelled));
    assert!(!copied.exists());
}

#[tokio::test]
async fn copy_operation_waits_while_paused_and_resumes() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let copied = dir.path().join("copied.txt");
    fs::write(&source, b"pause me").unwrap();
    let token = tokio_util::sync::CancellationToken::new();
    let (run_state_sender, run_state_receiver) =
        tokio::sync::watch::channel(FileOperationRunState::Paused);
    let controls = FileOperationControls::new(token, run_state_receiver);

    let copy = tokio::spawn(copy_path_with_options(
        source.clone(),
        copied.clone(),
        FileTransferOptions::new(controls),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    assert!(!copied.exists());

    run_state_sender
        .send(FileOperationRunState::Running)
        .unwrap();
    copy.await.unwrap().unwrap();

    assert_eq!(fs::read(copied).unwrap(), b"pause me");
}
