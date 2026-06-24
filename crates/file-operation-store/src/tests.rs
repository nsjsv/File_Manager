use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

fn test_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "file-operation-store-test-{}-{}-{}",
        std::process::id(),
        current_time_ms(),
        NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

fn test_store() -> (TaskQueueStore, PathBuf) {
    let root = test_root();
    let store = TaskQueueStore::new(root.join("state.sqlite")).unwrap();
    (store, root)
}

#[test]
fn insert_read_update_and_delete_task() {
    let (store, root) = test_store();
    let operation = StoredOperation::Copy {
        transfers: vec![StoredTransfer {
            source: StoredPath::from_path(Path::new("/tmp/source")),
            target: StoredPath::from_path(Path::new("/tmp/target")),
        }],
    };

    let id = store.insert_task(&operation).unwrap();
    let task = store.read_task(id).unwrap().unwrap();
    assert_eq!(task.id, id);
    assert_eq!(task.operation, operation);
    assert_eq!(task.status, StoredTaskStatus::Pending);
    assert_eq!(task.progress, StoredProgress::pending());
    assert_eq!(task.error, None);

    store.update_status(id, StoredTaskStatus::Running).unwrap();
    store
        .update_progress(id, StoredProgress::with_fraction(0.5))
        .unwrap();
    store.update_error(id, Some("boom")).unwrap();

    let updated = store.read_task(id).unwrap().unwrap();
    assert_eq!(updated.status, StoredTaskStatus::Running);
    assert_eq!(updated.progress, StoredProgress::with_fraction(0.5));
    assert_eq!(updated.error.as_deref(), Some("boom"));

    store
        .update_task_state(
            id,
            StoredTaskStatus::Completed,
            StoredProgress::with_fraction(1.0),
            None,
        )
        .unwrap();
    let completed = store.read_task(id).unwrap().unwrap();
    assert_eq!(completed.status, StoredTaskStatus::Completed);
    assert_eq!(completed.progress, StoredProgress::with_fraction(1.0));
    assert_eq!(completed.error, None);

    store.delete_task(id).unwrap();
    assert!(store.read_task(id).unwrap().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn marks_unfinished_tasks_failed_without_touching_failed_rows() {
    let (store, root) = test_store();
    let operation = StoredOperation::CreateDirectory {
        parent: StoredPath::from_path(Path::new("/tmp")),
    };
    let pending_id = store.insert_task(&operation).unwrap();
    let failed_id = store.insert_task(&operation).unwrap();
    store
        .update_status(failed_id, StoredTaskStatus::Failed)
        .unwrap();
    store.update_error(failed_id, Some("old error")).unwrap();
    let completed_id = store.insert_task(&operation).unwrap();
    store
        .update_status(completed_id, StoredTaskStatus::Completed)
        .unwrap();
    let canceled_id = store.insert_task(&operation).unwrap();
    store
        .update_status(canceled_id, StoredTaskStatus::Canceled)
        .unwrap();

    store
        .mark_unfinished_tasks_failed("recovered after restart")
        .unwrap();

    let recovered = store.read_task(pending_id).unwrap().unwrap();
    assert_eq!(recovered.status, StoredTaskStatus::Failed);
    assert_eq!(recovered.error.as_deref(), Some("recovered after restart"));

    let already_failed = store.read_task(failed_id).unwrap().unwrap();
    assert_eq!(already_failed.status, StoredTaskStatus::Failed);
    assert_eq!(already_failed.error.as_deref(), Some("old error"));
    let completed = store.read_task(completed_id).unwrap().unwrap();
    assert_eq!(completed.status, StoredTaskStatus::Completed);
    let canceled = store.read_task(canceled_id).unwrap().unwrap();
    assert_eq!(canceled.status, StoredTaskStatus::Canceled);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn clear_tasks_removes_all_rows() {
    let (store, root) = test_store();
    let operation = StoredOperation::CreateDirectory {
        parent: StoredPath::from_path(Path::new("/tmp")),
    };
    store.insert_task(&operation).unwrap();
    store.insert_task(&operation).unwrap();

    store.clear_tasks().unwrap();

    assert!(store.read_tasks().unwrap().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn delete_permanently_operation_roundtrips_through_json_and_database() {
    let (store, root) = test_store();
    let operation = StoredOperation::DeletePermanently {
        paths: vec![
            StoredPath::from_path(Path::new("/tmp/network/file.txt")),
            StoredPath::from_path(Path::new("/tmp/network/folder")),
        ],
    };
    let json = serde_json::to_string(&operation).unwrap();
    let decoded: StoredOperation = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, operation);
    assert_eq!(operation.kind(), "delete_permanently");

    let id = store.insert_task(&operation).unwrap();
    let task = store.read_task(id).unwrap().unwrap();

    assert_eq!(task.operation, operation);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn column_widths_roundtrip_replace_and_clear() {
    let (store, root) = test_store();
    store
        .replace_column_widths(HashMap::from([(0, 240.5), (2, 360.0)]))
        .unwrap();

    assert_eq!(
        store.read_column_widths().unwrap(),
        HashMap::from([(0, 240.5), (2, 360.0)])
    );

    store
        .replace_column_widths(HashMap::from([(1, 128.0)]))
        .unwrap();

    assert_eq!(
        store.read_column_widths().unwrap(),
        HashMap::from([(1, 128.0)])
    );

    store.replace_column_widths(HashMap::new()).unwrap();

    assert!(store.read_column_widths().unwrap().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn clear_tasks_keeps_column_widths() {
    let (store, root) = test_store();
    let operation = StoredOperation::CreateDirectory {
        parent: StoredPath::from_path(Path::new("/tmp")),
    };
    store.insert_task(&operation).unwrap();
    store
        .replace_column_widths(HashMap::from([(0, 240.0), (2, 360.0)]))
        .unwrap();

    store.clear_tasks().unwrap();

    assert!(store.read_tasks().unwrap().is_empty());
    assert_eq!(
        store.read_column_widths().unwrap(),
        HashMap::from([(0, 240.0), (2, 360.0)])
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn browser_session_roundtrip_replace_and_clear() {
    let (store, root) = test_store();
    let first = StoredBrowserSession {
        panes: vec![StoredBrowserPane {
            id: 0,
            tabs: vec![StoredBrowserTab {
                id: 0,
                directory: StoredPath::from_path(Path::new("/home/user")),
                is_trash_view: false,
                selected: Some(StoredPath::from_path(Path::new("/home/user/file.txt"))),
                selected_paths: vec![StoredPath::from_path(Path::new("/home/user/file.txt"))],
                deepest_open_column_directory: Some(StoredPath::from_path(Path::new(
                    "/home/user/Documents",
                ))),
                expanded_directories: vec![StoredPath::from_path(Path::new("/home/user/Projects"))],
                view_mode: StoredBrowserViewMode::List,
                back_stack: vec![StoredPath::from_path(Path::new("/home/user/Downloads"))],
                forward_stack: Vec::new(),
            }],
            active_tab_id: 0,
            column_viewports: vec![StoredColumnViewport {
                directory: StoredPath::from_path(Path::new("/home/user")),
                offset_y: 2.0,
                height: 400.0,
            }],
        }],
        layout: StoredBrowserPaneLayout::Single { active: 0 },
        search: Some(StoredSearchSession {
            scope: StoredSearchScope::CurrentDirectory,
            mode: StoredSearchMode::Files,
            root: StoredPath::from_path(Path::new("/home/user")),
            query: "notes".to_owned(),
        }),
        preview_path: Some(StoredPath::from_path(Path::new("/home/user/file.txt"))),
        properties: Some(StoredPropertiesSession {
            path: StoredPath::from_path(Path::new("/home/user/file.txt")),
            category: StoredFilePropertiesCategory::Permissions,
        }),
        settings_category: Some(StoredSettingsCategory::General),
    };

    store.replace_browser_session(&first).unwrap();
    assert_eq!(store.read_browser_session().unwrap(), Some(first.clone()));

    let second = StoredBrowserSession {
        panes: Vec::new(),
        layout: StoredBrowserPaneLayout::Single { active: 0 },
        search: None,
        preview_path: None,
        properties: None,
        settings_category: None,
    };
    store.replace_browser_session(&second).unwrap();
    assert_eq!(store.read_browser_session().unwrap(), Some(second));

    store.clear_browser_session().unwrap();
    assert_eq!(store.read_browser_session().unwrap(), None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unreadable_old_task_payload_rows_are_deleted_on_read() {
    let (store, root) = test_store();
    let connection = Connection::open(store.db_path()).unwrap();
    connection
        .execute(
            "INSERT INTO task_queue (
                operation_kind, payload_json, status, progress_fraction, error,
                created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, NULL, NULL, 1, 1)",
            params![
                "search_index",
                r#"{"kind":"search_index","root":{"encoding":"utf8","value":"/workspace"},"index_dir":{"encoding":"utf8","value":"/cache/old"},"selected_paths":[],"include_hidden":false}"#,
                StoredTaskStatus::Pending.as_str(),
            ],
        )
        .unwrap();
    drop(connection);

    assert!(store.read_tasks().unwrap().is_empty());
    let connection = Connection::open(store.db_path()).unwrap();
    let task_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM task_queue", [], |row| row.get(0))
        .unwrap();
    assert_eq!(task_count, 0);
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn non_utf8_unix_path_roundtrips_through_json_and_database() {
    let (store, root) = test_store();
    let path = PathBuf::from(OsString::from_vec(b"/tmp/non-utf8-\xFF".to_vec()));
    let operation = StoredOperation::Trash {
        paths: vec![StoredPath::from_path(&path)],
    };
    let json = serde_json::to_string(&operation).unwrap();
    let decoded: StoredOperation = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, operation);

    let id = store.insert_task(&operation).unwrap();
    let task = store.read_task(id).unwrap().unwrap();
    let StoredOperation::Trash { paths } = task.operation else {
        panic!("expected trash operation");
    };
    assert_eq!(
        paths[0].to_path_buf().as_os_str().as_bytes(),
        path.as_os_str().as_bytes()
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn browser_session_preserves_non_utf8_unix_paths() {
    let (store, root) = test_store();
    let path = PathBuf::from(OsString::from_vec(b"/tmp/session-\xFF".to_vec()));
    let session = StoredBrowserSession {
        panes: vec![StoredBrowserPane {
            id: 0,
            tabs: vec![StoredBrowserTab {
                id: 0,
                directory: StoredPath::from_path(&path),
                is_trash_view: false,
                selected: None,
                selected_paths: Vec::new(),
                deepest_open_column_directory: None,
                expanded_directories: Vec::new(),
                view_mode: StoredBrowserViewMode::Columns,
                back_stack: Vec::new(),
                forward_stack: Vec::new(),
            }],
            active_tab_id: 0,
            column_viewports: Vec::new(),
        }],
        layout: StoredBrowserPaneLayout::Single { active: 0 },
        search: None,
        preview_path: Some(StoredPath::from_path(&path)),
        properties: None,
        settings_category: None,
    };

    store.replace_browser_session(&session).unwrap();

    let restored = store.read_browser_session().unwrap().unwrap();
    let restored_path = restored.panes[0].tabs[0].directory.to_path_buf();
    assert_eq!(
        restored_path.as_os_str().as_bytes(),
        path.as_os_str().as_bytes()
    );
    let preview_path = restored.preview_path.unwrap().to_path_buf();
    assert_eq!(
        preview_path.as_os_str().as_bytes(),
        path.as_os_str().as_bytes()
    );
    let _ = fs::remove_dir_all(root);
}
