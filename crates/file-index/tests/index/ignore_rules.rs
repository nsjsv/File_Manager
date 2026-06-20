use super::*;

#[tokio::test]
async fn file_search_index_status_marks_exclude_rule_removal_stale() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), b"needle").unwrap();

    build_file_search_index(
        dir.path(),
        index_dir.path(),
        index_options_with_excludes(true, default_dependency_exclude_patterns()),
    )
    .await
    .unwrap();
    let status = file_index::file_search_index_status(
        index_dir.path(),
        dir.path(),
        FileSearchIndexOptions {
            include_hidden: true,
            ..FileSearchIndexOptions::default()
        },
    )
    .await
    .unwrap();

    assert!(status.exists);
    assert!(status.stale);
    assert_eq!(
        status.reason.as_deref(),
        Some("search index exclude rules are outdated")
    );
}

#[tokio::test]
async fn file_search_index_respects_configured_dependency_exclude_rules() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    write_default_ignored_dependency_tree(dir.path());
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/app.rs"), b"shown").unwrap();

    build_file_search_index(
        dir.path(),
        index_dir.path(),
        index_options_with_excludes(true, default_dependency_exclude_patterns()),
    )
    .await
    .unwrap();

    for query in [
        "node_modules",
        "left-pad",
        "pnpm-cache",
        "npm-cache",
        "cargo-registry",
        "cargo-target",
    ] {
        let ignored = search_file_index(
            index_dir.path(),
            dir.path(),
            query,
            search_options_with_excludes(true, 10, default_dependency_exclude_patterns()),
        )
        .await
        .unwrap();
        assert!(ignored.matches.is_empty(), "{query} should be ignored");
    }
    let shown = search_file_index(
        index_dir.path(),
        dir.path(),
        "app",
        search_options_with_excludes(true, 10, default_dependency_exclude_patterns()),
    )
    .await
    .unwrap();

    assert_eq!(shown.matches.len(), 1);
    assert_eq!(shown.matches[0].path, dir.path().join("src/app.rs"));
}

#[tokio::test]
async fn file_search_index_does_not_ignore_dependency_directories_without_configured_rules() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    write_default_ignored_dependency_tree(dir.path());

    build_file_search_index(
        dir.path(),
        index_dir.path(),
        FileSearchIndexOptions {
            include_hidden: true,
            ..FileSearchIndexOptions::default()
        },
    )
    .await
    .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "left-pad",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
            ..FileSearchOptions::default()
        },
    )
    .await
    .unwrap();

    assert!(search.matches.iter().any(|item| {
        item.path
            == dir
                .path()
                .join("project/node_modules/left-pad/package.json")
    }));
}

#[tokio::test]
async fn search_file_tree_respects_configured_dependency_exclude_rules() {
    let dir = tempdir().unwrap();
    write_default_ignored_dependency_tree(dir.path());
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/app.rs"), b"shown").unwrap();

    let ignored = search_file_tree(
        dir.path(),
        "pnpm-cache",
        search_options_with_excludes(true, 10, default_dependency_exclude_patterns()),
    )
    .await
    .unwrap();
    let shown = search_file_tree(
        dir.path(),
        "app",
        FileSearchOptions {
            include_hidden: true,
            limit: 10,
            ..FileSearchOptions::default()
        },
    )
    .await
    .unwrap();

    assert!(ignored.matches.is_empty());
    assert_eq!(shown.matches.len(), 1);
    assert_eq!(shown.matches[0].path, dir.path().join("src/app.rs"));
}

#[tokio::test]
async fn selected_path_indexing_respects_configured_dependency_exclude_rules() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let selected = dir.path().join("project/node_modules");
    fs::create_dir_all(&selected).unwrap();
    fs::write(selected.join("left-pad.js"), b"ignored").unwrap();

    let outcome = build_file_search_index_for_paths(
        dir.path(),
        index_dir.path(),
        vec![selected],
        index_options_with_excludes(true, default_dependency_exclude_patterns()),
    )
    .await
    .unwrap();
    let ignored = search_file_index(
        index_dir.path(),
        dir.path(),
        "left-pad",
        search_options_with_excludes(true, 10, default_dependency_exclude_patterns()),
    )
    .await
    .unwrap();

    assert_eq!(outcome.indexed_count, 0);
    assert!(ignored.matches.is_empty());
}

fn search_options_with_excludes(
    include_hidden: bool,
    limit: usize,
    exclude_patterns: Vec<String>,
) -> FileSearchOptions {
    FileSearchOptions {
        include_hidden,
        limit,
        exclude_patterns,
        ..FileSearchOptions::default()
    }
}

fn index_options_with_excludes(
    include_hidden: bool,
    exclude_patterns: Vec<String>,
) -> FileSearchIndexOptions {
    FileSearchIndexOptions {
        include_hidden,
        exclude_patterns,
        ..FileSearchIndexOptions::default()
    }
}

fn default_dependency_exclude_patterns() -> Vec<String> {
    file_index::default_search_index_exclude_patterns()
        .iter()
        .map(|pattern| (*pattern).to_owned())
        .collect()
}

fn write_default_ignored_dependency_tree(root: &std::path::Path) {
    fs::create_dir_all(root.join("project/node_modules/left-pad")).unwrap();
    fs::write(
        root.join("project/node_modules/left-pad/package.json"),
        b"ignored",
    )
    .unwrap();
    fs::create_dir_all(root.join("project/.pnpm-store/v3/files")).unwrap();
    fs::write(
        root.join("project/.pnpm-store/v3/files/pnpm-cache"),
        b"ignored",
    )
    .unwrap();
    fs::create_dir_all(root.join("project/.npm/_cacache")).unwrap();
    fs::write(root.join("project/.npm/_cacache/npm-cache"), b"ignored").unwrap();
    fs::create_dir_all(root.join("project/.cargo/registry/src")).unwrap();
    fs::write(
        root.join("project/.cargo/registry/src/cargo-registry"),
        b"ignored",
    )
    .unwrap();
    fs::create_dir_all(root.join("project/crate/target/debug")).unwrap();
    fs::write(
        root.join("project/crate/target/debug/cargo-target"),
        b"ignored",
    )
    .unwrap();
}
