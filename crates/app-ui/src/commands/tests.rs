use std::path::PathBuf;

use file_index::FileSearchIndexOutcome;

use super::ensure_search_index_outcome_matches_root;

#[test]
fn search_index_outcome_must_use_current_root_layout() {
    let index_base_dir = PathBuf::from("/tmp/index-base");
    let root = PathBuf::from("/home/user/Documents");
    let outcome = FileSearchIndexOutcome {
        root: root.clone(),
        index_dir: file_index::search_index_dir_for_root(&index_base_dir, &root),
        indexed_count: 1,
        index_size_bytes: 256,
        updated_at_ms: 1,
        failed_count: 0,
        skipped: Vec::new(),
    };

    assert!(ensure_search_index_outcome_matches_root(&index_base_dir, outcome).is_ok());
}

#[test]
fn search_index_outcome_rejects_stale_daemon_layout() {
    let index_base_dir = PathBuf::from("/tmp/index-base");
    let outcome = FileSearchIndexOutcome {
        root: PathBuf::from("/home/user/Documents"),
        index_dir: PathBuf::from("/tmp/index-base/old-hash-layout"),
        indexed_count: 1,
        index_size_bytes: 256,
        updated_at_ms: 1,
        failed_count: 0,
        skipped: Vec::new(),
    };

    let error = ensure_search_index_outcome_matches_root(&index_base_dir, outcome).unwrap_err();

    assert!(error.contains("expected"));
}
