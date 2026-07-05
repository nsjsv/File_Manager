#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use file_core::FileKind;
use file_index::{
    build_file_search_index, default_search_index_exclude_patterns, search_index_dir_for_root,
    BuildSelectedPathsRequest, DirectoryErrorPolicy, FileSearchIndexFailure,
    FileSearchIndexOptions, IndexProfile, IndexService, IndexServiceCommand, IndexServiceEvent,
    IndexTaskPhase, MediaMetadataPolicy, MediaMetadataScope, ProfileStore, SearchIndexFileRecord,
    SearchMode, SearchQuery,
};
use tempfile::tempdir;

#[test]
fn profile_store_preserves_explicit_roots_and_policies() {
    let dir = tempdir().unwrap();
    let store = ProfileStore::open(dir.path().join("control.sqlite")).unwrap();
    let profile = IndexProfile {
        id: "main".to_owned(),
        roots: vec![
            PathBuf::from("/tmp/projects"),
            PathBuf::from("/tmp/archive"),
        ],
        include_hidden: true,
        exclude_patterns: vec!["target/".to_owned(), "*.log".to_owned()],
        directory_error_policy: DirectoryErrorPolicy::Abort,
        media: MediaMetadataPolicy {
            scope: MediaMetadataScope::All,
        },
    };

    store.save_profile(&profile).unwrap();

    assert_eq!(store.load_profiles().unwrap(), vec![profile]);
}

#[test]
fn default_search_index_excludes_are_public_policy_patterns() {
    assert_eq!(
        default_search_index_exclude_patterns(),
        &[
            ".cargo/",
            ".npm/",
            ".pnpm/",
            ".pnpm-store/",
            "node_modules/",
            "target/",
        ]
    );
}

#[test]
fn index_service_configure_profile_preserves_explicit_nested_roots() {
    let dir = tempdir().unwrap();
    let service =
        IndexService::open(dir.path().join("control.sqlite"), dir.path().join("index")).unwrap();

    service
        .configure_profile(IndexProfile {
            id: "main".to_owned(),
            roots: vec![
                PathBuf::from("/workspace/project/src"),
                PathBuf::from("/workspace/project"),
                PathBuf::from("/workspace/archive"),
            ],
            include_hidden: false,
            exclude_patterns: Vec::new(),
            directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
            media: MediaMetadataPolicy::default(),
        })
        .unwrap();

    let IndexServiceEvent::ProfileLoaded(Some(profile)) = service.load_profile("main").unwrap()
    else {
        panic!("expected profile");
    };

    assert_eq!(
        profile.roots,
        vec![
            PathBuf::from("/workspace/project/src"),
            PathBuf::from("/workspace/project"),
            PathBuf::from("/workspace/archive"),
        ]
    );
}

#[test]
fn profile_store_persists_task_status_and_extractor_version() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("root");
    let store = ProfileStore::open(dir.path().join("control.sqlite")).unwrap();
    store
        .save_profile(&IndexProfile::new("main", vec![root.clone()]))
        .unwrap();

    store
        .save_task_status(
            "main",
            Some(&root),
            IndexTaskPhase::Running,
            Some("building"),
        )
        .unwrap();

    let statuses = store.load_task_statuses().unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].profile_id, "main");
    assert_eq!(statuses[0].root.as_ref(), Some(&root));
    assert_eq!(statuses[0].phase, IndexTaskPhase::Running);
    assert_eq!(statuses[0].message.as_deref(), Some("building"));
    assert!(statuses[0].extractor_version > 0);
}

#[test]
fn profile_store_drops_old_control_schema_without_migration() {
    let dir = tempdir().unwrap();
    let control_db = dir.path().join("control.sqlite");
    {
        let connection = rusqlite::Connection::open(&control_db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO schema_meta (key, value) VALUES ('schema_version', '4');
                 CREATE TABLE profiles (
                    id TEXT PRIMARY KEY,
                    include_hidden INTEGER NOT NULL,
                    content_enabled INTEGER NOT NULL,
                    content_max_file_bytes INTEGER NOT NULL,
                    media_enabled INTEGER NOT NULL,
                    directory_error_policy TEXT NOT NULL
                 );
                 INSERT INTO profiles (
                    id, include_hidden, content_enabled, content_max_file_bytes,
                    media_enabled, directory_error_policy
                 ) VALUES ('default', 0, 0, 16777216, 0, 'skip_unreadable');",
            )
            .unwrap();
    }

    let store = ProfileStore::open(&control_db).unwrap();

    assert!(store.load_profiles().unwrap().is_empty());
}

#[test]
fn profile_store_persists_root_file_metadata_and_failures() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("root");
    let store = ProfileStore::open(dir.path().join("control.sqlite")).unwrap();
    store
        .save_profile(&IndexProfile::new("main", vec![root.clone()]))
        .unwrap();
    let record = SearchIndexFileRecord {
        path: root.join("note.txt"),
        relative_path: PathBuf::from("note.txt"),
        kind: FileKind::File,
        mtime_ms: Some(123),
        size_bytes: Some(456),
    };
    let failure = FileSearchIndexFailure {
        path: root.join("blocked"),
        message: "permission denied".to_owned(),
        first_failed_at_ms: 1,
        last_failed_at_ms: 2,
        retry_count: 3,
    };

    store
        .save_root_snapshot(
            "main",
            &root,
            std::slice::from_ref(&record),
            &[failure.clone()],
        )
        .unwrap();

    let snapshot = store.load_root_snapshot("main", &root).unwrap();
    assert_eq!(snapshot.records, vec![record]);
    assert_eq!(snapshot.failures, vec![failure]);
}

#[test]
fn profile_store_waits_for_short_write_lock_overlap() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("root");
    let control_db = dir.path().join("control.sqlite");
    let store = ProfileStore::open(&control_db).unwrap();
    store
        .save_profile(&IndexProfile::new("main", vec![root.clone()]))
        .unwrap();
    let (lock_ready_tx, lock_ready_rx) = mpsc::channel();
    let lock_db = control_db.clone();

    let lock_holder = thread::spawn(move || {
        let connection = rusqlite::Connection::open(lock_db).unwrap();
        connection.execute_batch("BEGIN IMMEDIATE;").unwrap();
        lock_ready_tx.send(()).unwrap();
        thread::sleep(Duration::from_millis(100));
        connection.execute_batch("COMMIT;").unwrap();
    });

    lock_ready_rx.recv().unwrap();
    store
        .save_task_status(
            "main",
            Some(&root),
            IndexTaskPhase::Running,
            Some("briefly blocked"),
        )
        .unwrap();
    lock_holder.join().unwrap();
}

#[cfg(unix)]
#[test]
fn profile_store_preserves_non_utf8_paths_in_profile_snapshot_and_failures() {
    let dir = tempdir().unwrap();
    let root = dir.path().join(non_utf8_name(b"root-\xff"));
    let file = root.join(non_utf8_name(b"file-\xfe.txt"));
    let relative = PathBuf::from(non_utf8_name(b"file-\xfe.txt"));
    let failure_path = root.join(non_utf8_name(b"blocked-\xfd"));
    let store = ProfileStore::open(dir.path().join("control.sqlite")).unwrap();
    let profile = IndexProfile::new("main", vec![root.clone()]);
    store.save_profile(&profile).unwrap();
    let record = SearchIndexFileRecord {
        path: file.clone(),
        relative_path: relative.clone(),
        kind: FileKind::File,
        mtime_ms: Some(123),
        size_bytes: Some(456),
    };
    let failure = FileSearchIndexFailure {
        path: failure_path.clone(),
        message: "permission denied".to_owned(),
        first_failed_at_ms: 1,
        last_failed_at_ms: 2,
        retry_count: 3,
    };

    store
        .save_task_status(
            "main",
            Some(&root),
            IndexTaskPhase::Running,
            Some("building"),
        )
        .unwrap();
    store
        .save_root_snapshot("main", &root, &[record.clone()], &[failure.clone()])
        .unwrap();

    assert_eq!(store.load_profiles().unwrap()[0].roots, vec![root.clone()]);
    assert_eq!(
        store.load_task_statuses().unwrap()[0].root.as_ref(),
        Some(&root)
    );
    let snapshot = store.load_root_snapshot("main", &root).unwrap();
    assert_eq!(snapshot.records, vec![record]);
    assert_eq!(snapshot.failures, vec![failure]);
}

#[tokio::test]
async fn index_service_queries_files_mode_from_configured_profile() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("daily-note.txt"), b"note").unwrap();
    let index_base = dir.path().join("indexes");
    let index_dir = search_index_dir_for_root(&index_base, &root);
    build_file_search_index(&root, &index_dir, FileSearchIndexOptions::default())
        .await
        .unwrap();
    let service = IndexService::open(dir.path().join("control.sqlite"), &index_base).unwrap();
    service
        .configure_profile(IndexProfile::new("main", vec![root.clone()]))
        .unwrap();

    let event = service
        .query(SearchQuery {
            profile_id: "main".to_owned(),
            root,
            text: "daily".to_owned(),
            mode: SearchMode::Files,
            limit: 10,
        })
        .await
        .unwrap();

    let IndexServiceEvent::QueryFinished(outcome) = event else {
        panic!("expected query event");
    };
    assert_eq!(outcome.matches.len(), 1);
    assert_eq!(
        outcome.matches[0].name().to_string_lossy(),
        "daily-note.txt"
    );
}

#[tokio::test]
async fn index_service_queries_contents_mode_via_rg_without_catalog() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("meeting.md"), "quarterly roadmap signal").unwrap();
    let index_base = dir.path().join("indexes");
    let service = IndexService::open(dir.path().join("control.sqlite"), &index_base).unwrap();
    service
        .configure_profile(IndexProfile::new("main", vec![root.clone()]))
        .unwrap();

    let event = service
        .query(SearchQuery {
            profile_id: "main".to_owned(),
            root,
            text: "roadmap".to_owned(),
            mode: SearchMode::Contents,
            limit: 10,
        })
        .await
        .unwrap();

    let IndexServiceEvent::QueryFinished(outcome) = event else {
        panic!("expected query event");
    };
    assert_eq!(outcome.matches.len(), 1);
    assert_eq!(outcome.matches[0].name().to_string_lossy(), "meeting.md");
    assert_eq!(
        outcome.matches[0].source,
        file_index::SearchResultSource::Contents
    );
    assert_eq!(
        outcome.matches[0].snippet.as_deref(),
        Some("quarterly roadmap signal")
    );
}

#[tokio::test]
async fn index_service_command_covers_build_status_clear_failures_remove_root_and_delete_profile() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("selected-note.txt"), "body").unwrap();
    let control_db = dir.path().join("control.sqlite");
    let service = IndexService::open(&control_db, dir.path().join("indexes")).unwrap();
    service
        .execute(IndexServiceCommand::ConfigureProfile(IndexProfile::new(
            "main",
            vec![root.clone()],
        )))
        .await
        .unwrap();

    let event = service
        .execute(IndexServiceCommand::BuildSelectedPaths(
            BuildSelectedPathsRequest {
                profile_id: "main".to_owned(),
                root: root.clone(),
                selected_paths: vec![root.clone()],
            },
        ))
        .await
        .unwrap();
    assert!(matches!(event, IndexServiceEvent::RebuildFinished(_)));

    let event = service
        .execute(IndexServiceCommand::Status {
            profile_id: "main".to_owned(),
            root: root.clone(),
        })
        .await
        .unwrap();
    let IndexServiceEvent::StatusLoaded(status) = event else {
        panic!("expected status event");
    };
    assert!(status.exists);
    assert!(!status.stale);

    let event = service
        .execute(IndexServiceCommand::ClearFailures {
            profile_id: "main".to_owned(),
            root: root.clone(),
        })
        .await
        .unwrap();
    assert!(matches!(event, IndexServiceEvent::FailuresCleared(_)));

    let event = service
        .execute(IndexServiceCommand::RemoveRoot {
            profile_id: "main".to_owned(),
            root: root.clone(),
        })
        .await
        .unwrap();
    let IndexServiceEvent::RootRemoved(status) = event else {
        panic!("expected root removed event");
    };
    assert!(!status.exists);

    let event = service
        .execute(IndexServiceCommand::DeleteProfile("main".to_owned()))
        .await
        .unwrap();
    assert_eq!(event, IndexServiceEvent::ProfileDeleted("main".to_owned()));
}

#[tokio::test]
async fn index_service_rebuild_updates_task_status() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("status-note.txt"), "body").unwrap();
    let control_db = dir.path().join("control.sqlite");
    let service = IndexService::open(&control_db, dir.path().join("indexes")).unwrap();
    service
        .configure_profile(IndexProfile::new("main", vec![root.clone()]))
        .unwrap();

    service.rebuild("main", root.clone()).await.unwrap();

    let statuses = ProfileStore::open(control_db)
        .unwrap()
        .load_task_statuses()
        .unwrap();
    let status = statuses
        .iter()
        .find(|status| status.root.as_ref() == Some(&root))
        .expect("root task status");
    assert_eq!(status.phase, IndexTaskPhase::Finished);
    assert!(status.extractor_version > 0);
}

#[tokio::test]
async fn index_service_rebuild_excludes_index_base_inside_hidden_root() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("note.txt"), "ordinary file").unwrap();
    let index_base = root.join(".cache/file-manager/search-index");
    std::fs::create_dir_all(&index_base).unwrap();
    let service = IndexService::open(index_base.join("control.sqlite"), &index_base).unwrap();
    let mut profile = IndexProfile::new("main", vec![root.clone()]);
    profile.include_hidden = true;
    service.configure_profile(profile).unwrap();

    service.rebuild("main", root.clone()).await.unwrap();
    let event = service
        .query(SearchQuery {
            profile_id: "main".to_owned(),
            root,
            text: "control".to_owned(),
            mode: SearchMode::Files,
            limit: 10,
        })
        .await
        .unwrap();
    let IndexServiceEvent::QueryFinished(outcome) = event else {
        panic!("expected query result");
    };

    assert!(outcome.matches.is_empty());
}

#[cfg(unix)]
fn non_utf8_name(bytes: &[u8]) -> OsString {
    OsString::from_vec(bytes.to_vec())
}
