use super::*;

#[tokio::test]
async fn directory_watcher_coalesces_refresh_events() {
    let dir = tempdir().unwrap();
    let mut watcher = watch_directory(dir.path(), std::time::Duration::from_millis(40)).unwrap();

    fs::write(dir.path().join("one"), b"1").unwrap();
    fs::write(dir.path().join("two"), b"2").unwrap();

    let change = tokio::time::timeout(std::time::Duration::from_secs(2), watcher.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(change.path, dir.path());
}
