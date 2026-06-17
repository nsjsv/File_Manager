use super::*;

#[tokio::test]
async fn scan_directory_reads_regular_entries() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("b.txt"), b"hello").unwrap();
    fs::create_dir(dir.path().join("a-dir")).unwrap();

    let scan = scan_directory(dir.path(), ScanOptions::default())
        .await
        .unwrap();

    assert_eq!(scan.path, dir.path());
    assert_eq!(names(&scan.entries), vec!["a-dir", "b.txt"]);
    assert_eq!(scan.entries[0].kind, FileKind::Directory);
    assert_eq!(scan.entries[1].kind, FileKind::File);
}

#[tokio::test]
async fn scan_directory_with_progress_reports_batches_and_final_sorted_scan() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("b.txt"), b"hello").unwrap();
    fs::write(dir.path().join("a.txt"), b"hello").unwrap();

    let mut batch_names = Vec::new();
    let scan = scan_directory_with_progress(
        dir.path(),
        ScanOptions::default(),
        tokio_util::sync::CancellationToken::new(),
        |batch| batch_names.extend(names(&batch.entries)),
    )
    .await
    .unwrap();

    batch_names.sort();
    assert_eq!(batch_names, vec!["a.txt", "b.txt"]);
    assert_eq!(names(&scan.entries), vec!["a.txt", "b.txt"]);
}

#[tokio::test]
async fn scan_directory_with_progress_emits_multiple_batches_matching_final_scan() {
    let dir = tempdir().unwrap();
    for index in 0..260 {
        fs::write(dir.path().join(format!("file-{index:03}.txt")), b"hello").unwrap();
    }

    let mut batch_names = Vec::new();
    let mut batch_count = 0usize;
    let scan = scan_directory_with_progress(
        dir.path(),
        ScanOptions::default(),
        tokio_util::sync::CancellationToken::new(),
        |batch| {
            batch_count += 1;
            batch_names.extend(names(&batch.entries));
        },
    )
    .await
    .unwrap();
    let baseline = scan_directory(dir.path(), ScanOptions::default())
        .await
        .unwrap();

    batch_names.sort();
    assert!(batch_count > 1);
    assert_eq!(batch_names, names(&baseline.entries));
    assert_eq!(names(&scan.entries), names(&baseline.entries));
}

#[tokio::test]
async fn scan_directory_with_progress_respects_cancelled_token() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), b"hello").unwrap();
    let cancellation = tokio_util::sync::CancellationToken::new();
    cancellation.cancel();

    let error =
        scan_directory_with_progress(dir.path(), ScanOptions::default(), cancellation, |_| {
            panic!("cancelled scan must not emit batches")
        })
        .await
        .unwrap_err();

    assert!(matches!(error, FileError::Cancelled));
}

#[cfg(unix)]
#[tokio::test]
async fn scan_directory_preserves_non_utf8_names() {
    let dir = tempdir().unwrap();
    let name = std::ffi::OsString::from_vec(vec![b'n', b'o', b'n', 0xff]);
    fs::write(dir.path().join(&name), b"bytes").unwrap();

    let scan = scan_directory(
        dir.path(),
        ScanOptions {
            include_hidden: true,
            ..ScanOptions::default()
        },
    )
    .await
    .unwrap();

    assert!(scan
        .entries
        .iter()
        .any(|entry| entry.name() == OsStr::new(&name)));
}

#[cfg(unix)]
#[tokio::test]
async fn scan_directory_marks_symlinks_and_broken_symlinks() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("target"), b"target").unwrap();
    std::os::unix::fs::symlink(dir.path().join("target"), dir.path().join("link")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("missing"), dir.path().join("broken")).unwrap();

    let scan = scan_directory(
        dir.path(),
        ScanOptions {
            include_hidden: true,
            ..ScanOptions::default()
        },
    )
    .await
    .unwrap();

    let link = scan
        .entries
        .iter()
        .find(|entry| entry.name() == "link")
        .unwrap();
    let broken = scan
        .entries
        .iter()
        .find(|entry| entry.name() == "broken")
        .unwrap();

    assert_eq!(link.kind, FileKind::Symlink);
    assert!(link.is_symlink);
    assert!(!link.is_broken_symlink);
    assert_eq!(broken.kind, FileKind::Symlink);
    assert!(broken.is_broken_symlink);
}

#[tokio::test]
async fn scan_missing_directory_returns_structured_error() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing");

    let error = scan_directory(&missing, ScanOptions::default())
        .await
        .unwrap_err();

    match error {
        FileError::ReadDirectory { path, source } => {
            assert_eq!(path, missing);
            assert_eq!(source.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn scan_unreadable_directory_reports_error_when_os_denies_access() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let locked = dir.path().join("locked");
    fs::create_dir(&locked).unwrap();
    let original_permissions = fs::metadata(&locked).unwrap().permissions();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let result = scan_directory(&locked, ScanOptions::default()).await;

    fs::set_permissions(&locked, original_permissions).unwrap();

    if let Err(FileError::ReadDirectory { source, .. }) = result {
        assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
    }
}
