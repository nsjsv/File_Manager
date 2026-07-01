use super::*;
use rusqlite::Connection;

#[tokio::test]
async fn file_search_index_files_mode_keeps_catalog_only_old_extractor_queryable() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("alpha-note.txt"), "alpha runway").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();
    set_manifest_extractor_version(index_dir.path(), 2);

    let status =
        file_index::file_search_index_status(index_dir.path(), dir.path(), index_options(true))
            .await
            .unwrap();
    assert!(status.exists);
    assert!(!status.stale);

    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "alpha",
        search_options(true, 10),
    )
    .await
    .unwrap();
    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].path, dir.path().join("alpha-note.txt"));
}

#[tokio::test]
async fn file_search_index_full_text_modes_require_current_extractor_version() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("alpha-note.md"), "alpha runway").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), content_index_options(true))
        .await
        .unwrap();
    set_manifest_extractor_version(index_dir.path(), 2);

    let status = file_index::file_search_index_status(
        index_dir.path(),
        dir.path(),
        content_index_options(true),
    )
    .await
    .unwrap();
    assert!(status.exists);
    assert!(status.stale);
    assert_eq!(
        status.reason.as_deref(),
        Some("search index extractor version is outdated")
    );

    let error = search_file_index(
        index_dir.path(),
        dir.path(),
        "alpha",
        content_search_options(true, 10),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        IndexError::Store { message, .. } if message.contains("extractor version is outdated")
    ));
}

#[tokio::test]
async fn file_search_index_rebuild_invalidates_cached_full_text_runtime() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("first-note.md"), "alpha runway").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), content_index_options(true))
        .await
        .unwrap();
    let first = search_file_index(
        index_dir.path(),
        dir.path(),
        "alpha",
        content_search_options(true, 10),
    )
    .await
    .unwrap();
    assert_eq!(first.matches.len(), 1);

    fs::remove_file(dir.path().join("first-note.md")).unwrap();
    fs::write(dir.path().join("second-note.md"), "beta runway").unwrap();
    build_file_search_index(dir.path(), index_dir.path(), content_index_options(true))
        .await
        .unwrap();

    let stale = search_file_index(
        index_dir.path(),
        dir.path(),
        "alpha",
        content_search_options(true, 10),
    )
    .await
    .unwrap();
    let refreshed = search_file_index(
        index_dir.path(),
        dir.path(),
        "beta",
        content_search_options(true, 10),
    )
    .await
    .unwrap();

    assert!(stale.matches.is_empty());
    assert_eq!(refreshed.matches.len(), 1);
    assert_eq!(refreshed.matches[0].path, dir.path().join("second-note.md"));
}

#[tokio::test]
async fn file_search_index_all_mode_merges_duplicate_sources() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("roadmap.md"), "roadmap body").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), all_index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "roadmap",
        all_search_options(true, 10),
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].path, dir.path().join("roadmap.md"));
    assert_eq!(
        search.matches[0].source,
        file_index::SearchResultSource::Contents
    );
    assert!(search.matches[0]
        .snippet
        .as_deref()
        .is_some_and(|snippet| snippet.contains("roadmap")));
}

#[tokio::test]
async fn file_search_index_all_mode_preserves_media_source() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let image_path = dir.path().join("holiday-photo.png");
    image::RgbImage::new(24, 12).save(&image_path).unwrap();

    build_file_search_index(dir.path(), index_dir.path(), all_index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "holiday",
        all_search_options(true, 10),
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].path, image_path);
    assert_eq!(
        search.matches[0].source,
        file_index::SearchResultSource::Media
    );
    let media = search.matches[0].media.as_ref().expect("media metadata");
    assert_eq!(media.media_kind, file_index::MediaSearchKind::Image);
    assert_eq!(media.width, Some(24));
    assert_eq!(media.height, Some(12));
}

fn content_search_options(include_hidden: bool, limit: usize) -> FileSearchOptions {
    FileSearchOptions {
        mode: file_index::SearchMode::Contents,
        content_index_enabled: true,
        content_max_file_bytes: 16 * 1024 * 1024,
        include_hidden,
        limit,
        ..FileSearchOptions::default()
    }
}

fn search_options(include_hidden: bool, limit: usize) -> FileSearchOptions {
    FileSearchOptions {
        include_hidden,
        limit,
        ..FileSearchOptions::default()
    }
}

fn all_search_options(include_hidden: bool, limit: usize) -> FileSearchOptions {
    FileSearchOptions {
        mode: file_index::SearchMode::All,
        content_index_enabled: true,
        content_max_file_bytes: 16 * 1024 * 1024,
        media_metadata_scope: file_index::MediaMetadataScope::All,
        include_hidden,
        limit,
        ..FileSearchOptions::default()
    }
}

fn content_index_options(include_hidden: bool) -> FileSearchIndexOptions {
    FileSearchIndexOptions {
        content_index_enabled: true,
        content_max_file_bytes: 16 * 1024 * 1024,
        include_hidden,
        ..FileSearchIndexOptions::default()
    }
}

fn index_options(include_hidden: bool) -> FileSearchIndexOptions {
    FileSearchIndexOptions {
        include_hidden,
        ..FileSearchIndexOptions::default()
    }
}

fn all_index_options(include_hidden: bool) -> FileSearchIndexOptions {
    FileSearchIndexOptions {
        content_index_enabled: true,
        content_max_file_bytes: 16 * 1024 * 1024,
        media_metadata_scope: file_index::MediaMetadataScope::All,
        include_hidden,
        ..FileSearchIndexOptions::default()
    }
}

fn set_manifest_extractor_version(index_dir: &std::path::Path, extractor_version: u32) {
    let connection = Connection::open(index_dir.join("catalog.sqlite")).unwrap();
    connection
        .execute(
            "UPDATE manifest SET value = ?1 WHERE key = 'extractor_version'",
            [extractor_version.to_string()],
        )
        .unwrap();
}
