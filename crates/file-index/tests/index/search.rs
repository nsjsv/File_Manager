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
async fn file_search_index_status_marks_content_policy_change_stale() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), b"needle").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), content_index_options(true))
        .await
        .unwrap();
    let status = file_index::file_search_index_status(
        index_dir.path(),
        dir.path(),
        FileSearchIndexOptions {
            content_index_enabled: true,
            content_max_file_bytes: 8,
            ..index_options(true)
        },
    )
    .await
    .unwrap();

    assert!(status.exists);
    assert!(status.stale);
    assert_eq!(
        status.reason.as_deref(),
        Some("search index content size policy is outdated")
    );
}

#[tokio::test]
async fn file_search_index_status_marks_media_policy_change_stale() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("photo.jpg"), b"not really a jpeg").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();
    let status = file_index::file_search_index_status(
        index_dir.path(),
        dir.path(),
        media_index_options(true),
    )
    .await
    .unwrap();

    assert!(status.exists);
    assert!(status.stale);
    assert_eq!(
        status.reason.as_deref(),
        Some("search index media policy is outdated")
    );
}

#[tokio::test]
async fn file_search_index_status_marks_directory_error_policy_change_stale() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), b"needle").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();
    let status = file_index::file_search_index_status(
        index_dir.path(),
        dir.path(),
        FileSearchIndexOptions {
            directory_error_policy: DirectoryErrorPolicy::Abort,
            ..index_options(true)
        },
    )
    .await
    .unwrap();

    assert!(status.exists);
    assert!(status.stale);
    assert_eq!(
        status.reason.as_deref(),
        Some("search index directory error policy is outdated")
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
async fn file_search_index_fallback_candidates_respect_result_limit() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    for index in 0..20 {
        fs::write(
            dir.path()
                .join(format!("quarterly-search-memo-{index:02}.md")),
            b"note",
        )
        .unwrap();
    }

    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();
    let search = search_file_index(index_dir.path(), dir.path(), "qsm", search_options(true, 5))
        .await
        .unwrap();

    assert_eq!(search.matches.len(), 5);
    assert!(search.matches.iter().all(|search_match| search_match
        .name()
        .to_string_lossy()
        .contains("quarterly-search-memo")));
}

#[cfg(unix)]
#[tokio::test]
async fn file_search_index_records_unreadable_directory_and_continues() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("visible-note.txt"), b"shown").unwrap();
    let blocked = dir.path().join("blocked");
    fs::create_dir_all(&blocked).unwrap();
    fs::write(blocked.join("hidden-note.txt"), b"hidden").unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();

    let outcome = build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();

    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(outcome.indexed_count, 1);
    assert_eq!(outcome.failed_count, 1);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].path, blocked);

    let status =
        file_index::file_search_index_status(index_dir.path(), dir.path(), index_options(true))
            .await
            .unwrap();
    assert_eq!(status.failed_count, 1);
    assert_eq!(status.failures.len(), 1);
    assert_eq!(status.failures[0].path, blocked);

    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "visible",
        search_options(true, 10),
    )
    .await
    .unwrap();
    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].path, dir.path().join("visible-note.txt"));
}

#[cfg(unix)]
#[tokio::test]
async fn file_search_index_abort_policy_fails_on_unreadable_directory() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let blocked = dir.path().join("blocked");
    fs::create_dir_all(&blocked).unwrap();
    fs::write(blocked.join("hidden-note.txt"), b"hidden").unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    let mut options = index_options(true);
    options.directory_error_policy = DirectoryErrorPolicy::Abort;

    let error = build_file_search_index(dir.path(), index_dir.path(), options)
        .await
        .unwrap_err();

    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(error, IndexError::ReadDirectory { path, .. } if path == blocked));
}

#[tokio::test]
async fn file_search_index_queries_text_contents_when_enabled() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("meeting.md"), "quarterly launch runway").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), content_index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "runway",
        content_search_options(true, 10),
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].path, dir.path().join("meeting.md"));
    assert_eq!(
        search.matches[0].source,
        file_index::SearchResultSource::Contents
    );
    assert!(search.matches[0].snippet.is_none());
}

#[tokio::test]
async fn file_search_index_skips_binary_text_candidates() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("binary.txt"), b"needle\0hidden").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), content_index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "needle",
        content_search_options(true, 10),
    )
    .await
    .unwrap();

    assert!(search.matches.is_empty());
}

#[tokio::test]
async fn file_search_index_skips_log_and_csv_contents_by_default() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("events.log"), "roadmap").unwrap();
    fs::write(dir.path().join("report.csv"), "roadmap").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), content_index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "roadmap",
        content_search_options(true, 10),
    )
    .await
    .unwrap();

    assert!(search.matches.is_empty());
}

#[tokio::test]
async fn file_search_index_queries_image_media_metadata_when_enabled() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let path = dir.path().join("product-shot.png");
    image::RgbImage::new(37, 19).save(&path).unwrap();

    build_file_search_index(dir.path(), index_dir.path(), media_index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "product",
        media_search_options(true, 10),
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    let media = search.matches[0].media.as_ref().expect("media metadata");
    assert_eq!(
        search.matches[0].source,
        file_index::SearchResultSource::Media
    );
    assert_eq!(media.media_kind, file_index::MediaSearchKind::Image);
    assert_eq!(media.width, Some(37));
    assert_eq!(media.height, Some(19));
}

#[tokio::test]
async fn selected_path_full_rebuild_queries_content_and_media_metadata() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let selected = dir.path().join("selected");
    fs::create_dir_all(&selected).unwrap();
    fs::write(selected.join("briefing.md"), "quarterly runway").unwrap();
    let image_path = selected.join("product-shot.png");
    image::RgbImage::new(31, 17).save(&image_path).unwrap();
    let all_search_options = |mode| FileSearchOptions {
        mode,
        content_index_enabled: true,
        media_metadata_scope: file_index::MediaMetadataScope::All,
        ..search_options(true, 10)
    };

    build_file_search_index_for_paths(
        dir.path(),
        index_dir.path(),
        vec![selected],
        all_index_options(true),
    )
    .await
    .unwrap();

    let content = search_file_index(
        index_dir.path(),
        dir.path(),
        "runway",
        all_search_options(file_index::SearchMode::Contents),
    )
    .await
    .unwrap();
    let media = search_file_index(
        index_dir.path(),
        dir.path(),
        "product",
        all_search_options(file_index::SearchMode::Media),
    )
    .await
    .unwrap();

    assert_eq!(content.matches.len(), 1);
    assert_eq!(
        content.matches[0].path,
        dir.path().join("selected/briefing.md")
    );
    assert!(content.matches[0].snippet.is_none());
    assert_eq!(media.matches[0].path, image_path);
    let media_metadata = media.matches[0].media.as_ref().expect("media metadata");
    assert_eq!(media_metadata.width, Some(31));
}

#[tokio::test]
async fn selected_path_incremental_rebuild_preserves_unaffected_content_and_media() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let selected = dir.path().join("selected");
    let notes = dir.path().join("notes");
    let photos = dir.path().join("photos");
    fs::create_dir_all(&selected).unwrap();
    fs::create_dir_all(&notes).unwrap();
    fs::create_dir_all(&photos).unwrap();
    fs::write(selected.join("briefing.md"), "quarterly runway").unwrap();
    fs::write(notes.join("summary.md"), "kept content").unwrap();
    let image_path = photos.join("product-shot.png");
    image::RgbImage::new(31, 17).save(&image_path).unwrap();
    let all_search_options = |mode| FileSearchOptions {
        mode,
        content_index_enabled: true,
        media_metadata_scope: file_index::MediaMetadataScope::All,
        ..search_options(true, 10)
    };

    build_file_search_index(dir.path(), index_dir.path(), all_index_options(true))
        .await
        .unwrap();

    fs::write(selected.join("briefing.md"), "launch signal").unwrap();
    build_file_search_index_for_paths(
        dir.path(),
        index_dir.path(),
        vec![selected.clone()],
        FileSearchIndexOptions {
            mode: file_index::FileSearchIndexMode::Incremental,
            ..all_index_options(true)
        },
    )
    .await
    .unwrap();

    let updated_content = search_file_index(
        index_dir.path(),
        dir.path(),
        "signal",
        all_search_options(file_index::SearchMode::Contents),
    )
    .await
    .unwrap();
    let retained_content = search_file_index(
        index_dir.path(),
        dir.path(),
        "kept",
        all_search_options(file_index::SearchMode::Contents),
    )
    .await
    .unwrap();
    let retained_media = search_file_index(
        index_dir.path(),
        dir.path(),
        "product",
        all_search_options(file_index::SearchMode::Media),
    )
    .await
    .unwrap();

    assert_eq!(updated_content.matches.len(), 1);
    assert_eq!(
        updated_content.matches[0].path,
        selected.join("briefing.md")
    );
    assert_eq!(retained_content.matches.len(), 1);
    assert_eq!(retained_content.matches[0].path, notes.join("summary.md"));
    assert_eq!(retained_media.matches.len(), 1);
    assert_eq!(retained_media.matches[0].path, image_path);
    assert_eq!(
        retained_media.matches[0]
            .media
            .as_ref()
            .and_then(|media| media.width),
        Some(31)
    );
}

#[tokio::test]
async fn selected_path_incremental_rebuild_does_not_report_unaffected_extractor_failures() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let selected = dir.path().join("selected");
    let notes = dir.path().join("notes");
    fs::create_dir_all(&selected).unwrap();
    fs::create_dir_all(&notes).unwrap();
    fs::write(selected.join("briefing.md"), "launch signal").unwrap();
    let outside = notes.join("stale.md");
    fs::write(&outside, "kept content").unwrap();

    build_file_search_index(dir.path(), index_dir.path(), content_index_options(true))
        .await
        .unwrap();

    fs::remove_file(&outside).unwrap();
    fs::write(selected.join("briefing.md"), "updated launch signal").unwrap();
    let outcome = build_file_search_index_for_paths(
        dir.path(),
        index_dir.path(),
        vec![selected],
        FileSearchIndexOptions {
            mode: file_index::FileSearchIndexMode::Incremental,
            ..content_index_options(true)
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.failed_count, 0);
    assert!(outcome.skipped.is_empty());

    let status = file_index::file_search_index_status(
        index_dir.path(),
        dir.path(),
        content_index_options(true),
    )
    .await
    .unwrap();
    assert_eq!(status.failed_count, 0);
    assert!(status.failures.is_empty());
}

#[tokio::test]
async fn file_search_index_queries_image_exif_when_enabled() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let path = dir.path().join("camera.png");
    fs::write(&path, png_with_exif_description("sunset ridge")).unwrap();

    build_file_search_index(dir.path(), index_dir.path(), media_index_options(true))
        .await
        .unwrap();
    let search = search_file_index(
        index_dir.path(),
        dir.path(),
        "sunset",
        media_search_options(true, 10),
    )
    .await
    .unwrap();

    assert_eq!(search.matches.len(), 1);
    let media = search.matches[0].media.as_ref().expect("media metadata");
    assert!(media
        .exif
        .iter()
        .any(|field| { field.tag == "Description" && field.value.contains("sunset ridge") }));
}

#[tokio::test]
async fn file_search_index_media_scope_limits_audio_to_all() {
    let dir = tempdir().unwrap();
    let off_index_dir = tempdir().unwrap();
    let images_index_dir = tempdir().unwrap();
    let all_index_dir = tempdir().unwrap();
    let photo = dir.path().join("holiday-photo.png");
    let audio = dir.path().join("holiday-song.mp3");
    image::RgbImage::new(2, 2).save(&photo).unwrap();
    fs::write(&audio, b"audio placeholder").unwrap();

    build_file_search_index(dir.path(), off_index_dir.path(), index_options(true))
        .await
        .unwrap();
    let off_search = search_file_index(
        off_index_dir.path(),
        dir.path(),
        "holiday",
        media_search_options_with_scope(true, 10, file_index::MediaMetadataScope::Off),
    )
    .await
    .unwrap();
    assert!(off_search.matches.is_empty());

    build_file_search_index(
        dir.path(),
        images_index_dir.path(),
        media_index_options_with_scope(true, file_index::MediaMetadataScope::Images),
    )
    .await
    .unwrap();
    let images_search = search_file_index(
        images_index_dir.path(),
        dir.path(),
        "holiday",
        media_search_options_with_scope(true, 10, file_index::MediaMetadataScope::Images),
    )
    .await
    .unwrap();
    assert_eq!(images_search.matches.len(), 1);
    assert_eq!(images_search.matches[0].path, photo);

    build_file_search_index(
        dir.path(),
        all_index_dir.path(),
        media_index_options_with_scope(true, file_index::MediaMetadataScope::All),
    )
    .await
    .unwrap();
    let all_search = search_file_index(
        all_index_dir.path(),
        dir.path(),
        "holiday-song",
        media_search_options_with_scope(true, 10, file_index::MediaMetadataScope::All),
    )
    .await
    .unwrap();
    assert_eq!(all_search.matches.len(), 1);
    assert_eq!(all_search.matches[0].path, audio);
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

    assert!(matches!(error, IndexError::Cancelled));
}

#[tokio::test]
async fn file_search_index_with_cancel_returns_cancelled() {
    let dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), b"note").unwrap();
    build_file_search_index(dir.path(), index_dir.path(), index_options(true))
        .await
        .unwrap();
    let cancellation = tokio_util::sync::CancellationToken::new();
    cancellation.cancel();

    let error = search_file_index_with_cancel(
        index_dir.path(),
        dir.path(),
        "note",
        search_options(true, 10),
        cancellation,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, IndexError::Cancelled));
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

fn content_search_options(include_hidden: bool, limit: usize) -> FileSearchOptions {
    FileSearchOptions {
        mode: file_index::SearchMode::Contents,
        content_index_enabled: true,
        content_max_file_bytes: file_index::profile::DEFAULT_CONTENT_MAX_FILE_BYTES,
        ..search_options(include_hidden, limit)
    }
}

fn media_search_options(include_hidden: bool, limit: usize) -> FileSearchOptions {
    media_search_options_with_scope(include_hidden, limit, file_index::MediaMetadataScope::All)
}

fn media_search_options_with_scope(
    include_hidden: bool,
    limit: usize,
    media_metadata_scope: file_index::MediaMetadataScope,
) -> FileSearchOptions {
    FileSearchOptions {
        mode: file_index::SearchMode::Media,
        media_metadata_scope,
        ..search_options(include_hidden, limit)
    }
}

fn index_options(include_hidden: bool) -> FileSearchIndexOptions {
    FileSearchIndexOptions {
        include_hidden,
        ..FileSearchIndexOptions::default()
    }
}

fn content_index_options(include_hidden: bool) -> FileSearchIndexOptions {
    FileSearchIndexOptions {
        content_index_enabled: true,
        content_max_file_bytes: file_index::profile::DEFAULT_CONTENT_MAX_FILE_BYTES,
        ..index_options(include_hidden)
    }
}

fn media_index_options(include_hidden: bool) -> FileSearchIndexOptions {
    media_index_options_with_scope(include_hidden, file_index::MediaMetadataScope::All)
}

fn media_index_options_with_scope(
    include_hidden: bool,
    media_metadata_scope: file_index::MediaMetadataScope,
) -> FileSearchIndexOptions {
    FileSearchIndexOptions {
        media_metadata_scope,
        ..index_options(include_hidden)
    }
}

fn all_index_options(include_hidden: bool) -> FileSearchIndexOptions {
    FileSearchIndexOptions {
        content_index_enabled: true,
        media_metadata_scope: file_index::MediaMetadataScope::All,
        ..index_options(include_hidden)
    }
}
