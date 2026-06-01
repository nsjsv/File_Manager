use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::{fs, io};

use file_core::{
    build_file_search_index, copy_path, copy_path_with_conflict_strategy, copy_path_with_controls,
    create_directory, create_empty_file, create_file_with_contents, filter_hidden, move_path,
    move_path_with_conflict_strategy, rename_path, scan_directory, search_file_index,
    search_file_tree, sort_entries, watch_directory, DirectoryEntry, EntryMetadata, FileError,
    FileKind, FileOperationControls, FileOperationRunState, FileSearchIndexOptions,
    FileSearchOptions, ScanOptions, SortDirection, SortField, TransferConflictStrategy,
};
use tempfile::tempdir;

fn entry(path: PathBuf, kind: FileKind, len: u64, is_hidden: bool) -> DirectoryEntry {
    DirectoryEntry::new(
        path,
        kind,
        EntryMetadata {
            len,
            modified: None,
            readonly: false,
        },
        is_hidden,
        false,
        false,
    )
}

fn names(entries: &[DirectoryEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| entry.name().to_string_lossy().into_owned())
        .collect()
}

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

#[test]
fn hidden_filter_removes_dot_entries_when_disabled() {
    let root = Path::new("/tmp");
    let mut entries = vec![
        entry(root.join(".hidden"), FileKind::File, 0, true),
        entry(root.join("shown"), FileKind::File, 0, false),
    ];

    filter_hidden(&mut entries, false);

    assert_eq!(names(&entries), vec!["shown"]);
}

#[test]
fn sort_entries_keeps_directories_first_then_sorts_by_name() {
    let root = Path::new("/tmp");
    let mut entries = vec![
        entry(root.join("z.txt"), FileKind::File, 2, false),
        entry(root.join("a-dir"), FileKind::Directory, 0, false),
        entry(root.join("a.txt"), FileKind::File, 1, false),
    ];
    let options = ScanOptions {
        include_hidden: true,
        sort_field: SortField::Name,
        sort_direction: SortDirection::Ascending,
        directories_first: true,
    };

    sort_entries(&mut entries, &options);

    assert_eq!(names(&entries), vec!["a-dir", "a.txt", "z.txt"]);
}

#[test]
fn sort_entries_can_sort_by_size_descending() {
    let root = Path::new("/tmp");
    let mut entries = vec![
        entry(root.join("small"), FileKind::File, 1, false),
        entry(root.join("large"), FileKind::File, 10, false),
    ];
    let options = ScanOptions {
        include_hidden: true,
        sort_field: SortField::Size,
        sort_direction: SortDirection::Descending,
        directories_first: false,
    };

    sort_entries(&mut entries, &options);

    assert_eq!(names(&entries), vec!["large", "small"]);
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

#[tokio::test]
async fn search_file_tree_finds_nested_fuzzy_match() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("projects/reporting")).unwrap();
    fs::write(
        dir.path().join("projects/reporting/quarterly-summary.md"),
        b"notes",
    )
    .unwrap();

    let search = search_file_tree(
        dir.path(),
        "qsm",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(
        search.matches[0].path,
        dir.path().join("projects/reporting/quarterly-summary.md")
    );
}

#[tokio::test]
async fn search_file_tree_fuzzy_match_ignores_case() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("projects/reporting")).unwrap();
    fs::write(
        dir.path().join("projects/reporting/quarterly-summary.md"),
        b"notes",
    )
    .unwrap();

    let search = search_file_tree(
        dir.path(),
        "QSM",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(
        search.matches[0].path,
        dir.path().join("projects/reporting/quarterly-summary.md")
    );
}

#[tokio::test]
async fn file_search_index_finds_nested_fuzzy_match_from_disk() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("projects/reporting")).unwrap();
    fs::write(
        dir.path().join("projects/reporting/quarterly-summary.md"),
        b"notes",
    )
    .unwrap();

    build_file_search_index(
        dir.path(),
        index_dir.path(),
        FileSearchIndexOptions {
            include_hidden: true,
        },
    )
    .await
    .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "qsm",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(
        search.matches[0].path,
        dir.path().join("projects/reporting/quarterly-summary.md")
    );
}

#[tokio::test]
async fn file_search_index_fuzzy_match_ignores_case() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("projects/reporting")).unwrap();
    fs::write(
        dir.path().join("projects/reporting/quarterly-summary.md"),
        b"notes",
    )
    .unwrap();

    build_file_search_index(
        dir.path(),
        index_dir.path(),
        FileSearchIndexOptions {
            include_hidden: true,
        },
    )
    .await
    .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "QSM",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(
        search.matches[0].path,
        dir.path().join("projects/reporting/quarterly-summary.md")
    );
}

#[tokio::test]
async fn file_search_index_respects_gitignore_rules() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "target/\n*.log\n").unwrap();
    fs::create_dir_all(dir.path().join("target")).unwrap();
    fs::write(dir.path().join("target/cache.log"), b"ignored").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/app.rs"), b"shown").unwrap();

    build_file_search_index(
        dir.path(),
        index_dir.path(),
        FileSearchIndexOptions {
            include_hidden: true,
        },
    )
    .await
    .unwrap();

    let ignored = search_file_index(
        index_dir.path(),
        dir.path(),
        "cache",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
        },
    )
    .await
    .unwrap();
    let shown = search_file_index(
        index_dir.path(),
        dir.path(),
        "app",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert!(ignored.matches.is_empty());
    assert_eq!(shown.matches.len(), 1);
    assert_eq!(shown.matches[0].path, dir.path().join("src/app.rs"));
}

#[tokio::test]
async fn search_file_tree_respects_hidden_option() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".secret-note"), b"hidden").unwrap();

    let hidden_excluded = search_file_tree(dir.path(), "secret", FileSearchOptions::default())
        .await
        .unwrap();
    let hidden_included = search_file_tree(
        dir.path(),
        "secret",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert!(hidden_excluded.matches.is_empty());
    assert_eq!(hidden_included.matches.len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn search_file_tree_preserves_non_utf8_match_path() {
    let dir = tempdir().unwrap();
    let name = std::ffi::OsString::from_vec(vec![b'n', b'o', b'n', 0xff]);
    let path = dir.path().join(&name);
    fs::write(&path, b"bytes").unwrap();

    let search = search_file_tree(
        dir.path(),
        "non",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].path, path);
    assert_eq!(search.matches[0].name(), OsStr::new(&name));
}

#[cfg(unix)]
#[tokio::test]
async fn file_search_index_preserves_non_utf8_match_path() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let name = std::ffi::OsString::from_vec(vec![b'n', b'o', b'n', 0xff]);
    let path = dir.path().join(&name);
    fs::write(&path, b"bytes").unwrap();

    build_file_search_index(
        dir.path(),
        index_dir.path(),
        FileSearchIndexOptions {
            include_hidden: true,
        },
    )
    .await
    .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "non",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].path, path);
    assert_eq!(search.matches[0].name(), OsStr::new(&name));
}

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

    copy_path_with_conflict_strategy(
        &source,
        &target,
        tokio_util::sync::CancellationToken::new(),
        None,
        TransferConflictStrategy::Replace,
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

    copy_path_with_conflict_strategy(
        &source,
        &target,
        tokio_util::sync::CancellationToken::new(),
        None,
        TransferConflictStrategy::Skip,
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

    copy_path_with_conflict_strategy(
        &source,
        &target,
        tokio_util::sync::CancellationToken::new(),
        None,
        TransferConflictStrategy::KeepBoth,
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

    copy_path_with_conflict_strategy(
        &source,
        &target,
        tokio_util::sync::CancellationToken::new(),
        None,
        TransferConflictStrategy::Merge,
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

    move_path_with_conflict_strategy(
        &source,
        &target,
        tokio_util::sync::CancellationToken::new(),
        None,
        TransferConflictStrategy::KeepBoth,
    )
    .await
    .unwrap();

    assert!(!source.exists());
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(fs::read(&alternate).unwrap(), b"new");
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

    move_path_with_conflict_strategy(
        &source,
        &target,
        tokio_util::sync::CancellationToken::new(),
        None,
        TransferConflictStrategy::Merge,
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

    let copy = tokio::spawn(copy_path_with_controls(
        source.clone(),
        copied.clone(),
        controls,
        None,
    ));
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    assert!(!copied.exists());

    run_state_sender
        .send(FileOperationRunState::Running)
        .unwrap();
    copy.await.unwrap().unwrap();

    assert_eq!(fs::read(copied).unwrap(), b"pause me");
}

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
