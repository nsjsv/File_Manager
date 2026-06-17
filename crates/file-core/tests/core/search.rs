use super::*;

#[tokio::test]
async fn search_file_tree_finds_nested_fuzzy_match() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("projects/reporting")).unwrap();
    fs::write(
        dir.path().join("projects/reporting/quarterly-summary.md"),
        b"notes",
    )
    .unwrap();

    let search = search_file_tree(dir.path(), "qsm", search_options(true, 10))
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

    let search = search_file_tree(dir.path(), "QSM", search_options(true, 10))
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

    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "qsm",
        search_options(true, 10),
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

    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "QSM",
        search_options(true, 10),
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

    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();

    let ignored = search_file_index(
        index_dir.path(),
        dir.path(),
        "cache",
        search_options(true, 10),
    )
    .await
    .unwrap();
    let shown = search_file_index(
        index_dir.path(),
        dir.path(),
        "app",
        search_options(true, 10),
    )
    .await
    .unwrap();

    assert!(ignored.matches.is_empty());
    assert_eq!(shown.matches.len(), 1);
    assert_eq!(shown.matches[0].path, dir.path().join("src/app.rs"));
}

#[tokio::test]
async fn search_file_tree_respects_gitignore_rules() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "target/\n*.log\n").unwrap();
    fs::create_dir_all(dir.path().join("target")).unwrap();
    fs::write(dir.path().join("target/cache.log"), b"ignored").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/app.rs"), b"shown").unwrap();

    let ignored = search_file_tree(dir.path(), "cache", search_options(true, 10))
        .await
        .unwrap();
    let shown = search_file_tree(dir.path(), "app", search_options(true, 10))
        .await
        .unwrap();

    assert!(ignored.matches.is_empty());
    assert_eq!(shown.matches.len(), 1);
    assert_eq!(shown.matches[0].path, dir.path().join("src/app.rs"));
}

#[tokio::test]
async fn file_search_index_uses_project_manifest_for_readiness() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("meta.json"), b"{}").unwrap();

    assert!(!file_search_index_exists(dir.path()));
}

#[tokio::test]
async fn file_search_index_rebuild_invalidates_cached_catalog() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("first-note.txt"), b"one").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();
    let first = search_file_index(
        index_dir.path(),
        dir.path(),
        "first",
        search_options(true, 10),
    )
    .await
    .unwrap();
    assert_eq!(first.matches.len(), 1);

    fs::remove_file(dir.path().join("first-note.txt")).unwrap();
    fs::write(dir.path().join("second-note.txt"), b"two").unwrap();
    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();

    let stale = search_file_index(
        index_dir.path(),
        dir.path(),
        "first",
        search_options(true, 10),
    )
    .await
    .unwrap();
    let refreshed = search_file_index(
        index_dir.path(),
        dir.path(),
        "second",
        search_options(true, 10),
    )
    .await
    .unwrap();

    assert!(stale.matches.is_empty());
    assert_eq!(refreshed.matches.len(), 1);
    assert_eq!(
        refreshed.matches[0].path,
        dir.path().join("second-note.txt")
    );
}

#[tokio::test]
async fn file_search_index_respects_result_limit() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    for index in 0..20 {
        fs::write(dir.path().join(format!("note-{index:02}.txt")), b"note").unwrap();
    }

    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "note",
        search_options(true, 5),
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 5);
}

#[tokio::test]
async fn search_file_tree_respects_hidden_option() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".secret-note"), b"hidden").unwrap();

    let hidden_excluded = search_file_tree(dir.path(), "secret", FileSearchOptions::default())
        .await
        .unwrap();
    let hidden_included = search_file_tree(dir.path(), "secret", search_options(true, 10))
        .await
        .unwrap();

    assert!(hidden_excluded.matches.is_empty());
    assert_eq!(hidden_included.matches.len(), 1);
}

#[tokio::test]
async fn search_file_tree_with_cancel_returns_cancelled() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), b"note").unwrap();
    let cancellation = tokio_util::sync::CancellationToken::new();
    cancellation.cancel();

    let error =
        search_file_tree_with_cancel(dir.path(), "note", search_options(true, 10), cancellation)
            .await
            .unwrap_err();

    assert!(matches!(error, FileError::Cancelled));
}

#[cfg(unix)]
#[tokio::test]
async fn search_file_tree_preserves_non_utf8_match_path() {
    let dir = tempdir().unwrap();
    let name = std::ffi::OsString::from_vec(vec![b'n', b'o', b'n', 0xff]);
    let path = dir.path().join(&name);
    fs::write(&path, b"bytes").unwrap();

    let search = search_file_tree(dir.path(), "non", search_options(true, 10))
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

    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "non",
        search_options(true, 10),
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].path, path);
    assert_eq!(search.matches[0].name(), OsStr::new(&name));
}

fn search_options(include_hidden: bool, limit: usize) -> FileSearchOptions {
    FileSearchOptions {
        include_hidden,
        limit,
        ..FileSearchOptions::default()
    }
}

fn index_options(include_hidden: bool) -> FileSearchIndexOptions {
    FileSearchIndexOptions {
        include_hidden,
        ..FileSearchIndexOptions::default()
    }
}
