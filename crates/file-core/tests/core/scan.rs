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
