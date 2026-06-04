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
