use super::*;

use tempfile::tempdir;

#[tokio::test]
async fn strong_verification_rejects_matching_size_hash_mismatch() {
    let directory = tempdir().unwrap();
    let source_path = directory.path().join("source.bin");
    let target = directory.path().join("target.bin");
    let source_contents = b"same length data";
    let target_contents = b"same length DATA";

    fs::write(&source_path, source_contents).await.unwrap();
    fs::write(&target, target_contents).await.unwrap();

    let source_metadata = fs::metadata(&source_path).await.unwrap();
    let mut controls = FileOperationControls::running(CancellationToken::new());
    let mut buffer = vec![0; COPY_BUFFER_SIZE];
    let error = verify_copied_file(
        &source_path,
        &target,
        &source_metadata,
        &mut controls,
        &mut buffer,
        Some(blake3::hash(source_contents)),
        true,
    )
    .await
    .unwrap_err();

    match error {
        FileError::Copy { from, to, source } => {
            assert_eq!(from, source_path);
            assert_eq!(to, target);
            assert_eq!(source.kind(), io::ErrorKind::InvalidData);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn operation_not_supported_permission_error_is_not_fatal() {
    let unsupported = io::Error::from(io::ErrorKind::Unsupported);
    let permission_denied = io::Error::from(io::ErrorKind::PermissionDenied);

    assert!(copy_permission_unsupported(&unsupported));
    assert!(!copy_permission_unsupported(&permission_denied));
}

#[cfg(unix)]
#[test]
fn os_error_95_permission_error_is_not_fatal() {
    let unsupported = io::Error::from_raw_os_error(95);

    assert!(copy_permission_unsupported(&unsupported));
}

#[tokio::test]
async fn readonly_mismatch_is_skipped_only_when_permissions_were_not_preserved() {
    let directory = tempdir().unwrap();
    let source_path = directory.path().join("source.txt");
    let target = directory.path().join("target.txt");

    fs::write(&source_path, b"same").await.unwrap();
    fs::write(&target, b"same").await.unwrap();
    let mut source_permissions = fs::metadata(&source_path).await.unwrap().permissions();
    source_permissions.set_readonly(true);
    fs::set_permissions(&source_path, source_permissions.clone())
        .await
        .unwrap();

    let source_metadata = fs::metadata(&source_path).await.unwrap();
    let mut controls = FileOperationControls::running(CancellationToken::new());
    let mut buffer = vec![0; COPY_BUFFER_SIZE];

    verify_copied_file(
        &source_path,
        &target,
        &source_metadata,
        &mut controls,
        &mut buffer,
        None,
        false,
    )
    .await
    .unwrap();
    let error = verify_copied_file(
        &source_path,
        &target,
        &source_metadata,
        &mut controls,
        &mut buffer,
        None,
        true,
    )
    .await
    .unwrap_err();
    source_permissions.set_readonly(false);
    fs::set_permissions(&source_path, source_permissions)
        .await
        .unwrap();

    assert!(
        matches!(error, FileError::Copy { source, .. } if source.kind() == io::ErrorKind::InvalidData)
    );
}
