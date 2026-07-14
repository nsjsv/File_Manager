use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{Event, EventKind};
use rusqlite::Connection;
use tempfile::tempdir;

use crate::model::{IndexHealth, IndexPhase, IndexStatus};
use crate::SearchIndexConfig;

use super::*;

fn snapshot_for(phase: DaemonLifecyclePhase) -> DaemonLifecycleSnapshot {
    DaemonLifecycleSnapshot {
        phase,
        maintenance_backend_failure: None,
        watch_coverage_health: WatchCoverageHealth::Healthy,
        visible_indexed_files: 41,
        capabilities: Vec::new(),
    }
}

#[test]
fn public_status_keeps_visible_count_independent_of_phase() {
    let cases = [
        (DaemonLifecyclePhase::Starting, IndexPhase::Starting),
        (
            DaemonLifecyclePhase::Checking {
                checked_entries: 128,
                changed_entries: 3,
            },
            IndexPhase::Checking {
                checked_entries: 128,
                changed_entries: 3,
            },
        ),
        (
            DaemonLifecyclePhase::Crawling {
                scanned_entries: 9,
                current_scope: PathBuf::from("/tmp/root"),
            },
            IndexPhase::Crawling {
                scanned_entries: 9,
                current_scope: PathBuf::from("/tmp/root"),
            },
        ),
        (
            DaemonLifecyclePhase::Applying {
                pending_mutations: 4,
            },
            IndexPhase::Applying {
                pending_mutations: 4,
            },
        ),
        (DaemonLifecyclePhase::Complete, IndexPhase::Complete),
        (
            DaemonLifecyclePhase::Failed {
                message: "database unavailable".to_owned(),
            },
            IndexPhase::Failed {
                message: "database unavailable".to_owned(),
            },
        ),
    ];

    for (phase, expected) in cases {
        let status = snapshot_for(phase).to_index_status();
        assert_eq!(status.phase, expected);
        assert_eq!(status.visible_indexed_files, 41);
    }
}

#[test]
fn watch_degradation_does_not_replace_active_phase() {
    let mut snapshot = snapshot_for(DaemonLifecyclePhase::Checking {
        checked_entries: 7,
        changed_entries: 1,
    });
    snapshot.update_watch_coverage(WatchCoverageHealth::Incomplete {
        gap_count: 1,
        message: "one watch unavailable".to_owned(),
    });

    assert_eq!(
        snapshot.to_index_status(),
        IndexStatus {
            phase: IndexPhase::Checking {
                checked_entries: 7,
                changed_entries: 1,
            },
            visible_indexed_files: 41,
            health: IndexHealth::Degraded {
                message: "one watch unavailable".to_owned(),
            },
            capabilities: Vec::new(),
        }
    );
}

#[test]
fn watch_event_paths_are_deduplicated_and_access_events_are_ignored() {
    let duplicate = PathBuf::from("/tmp/repeated");
    let changed = changed_paths_from_watch_event(
        Event::new(EventKind::Any)
            .add_path(duplicate.clone())
            .add_path(duplicate.clone()),
    )
    .unwrap();
    assert_eq!(changed, vec![duplicate]);

    let access = changed_paths_from_watch_event(
        Event::new(EventKind::Access(notify::event::AccessKind::Any))
            .add_path(PathBuf::from("/tmp/ignored")),
    )
    .unwrap();
    assert!(access.is_empty());
}

#[test]
fn ordinary_paths_arriving_during_startup_stay_local() {
    let root = PathBuf::from("/root");
    let changed = root.join("changed.txt");
    let now = Instant::now();
    let mut pending = PendingDaemonWork::new(vec![root]);
    pending.absorb_request(DaemonWorkRequest::StartupCheck, now);
    pending.absorb_request(
        DaemonWorkRequest::ChangedPaths {
            changed_paths: vec![changed.clone()],
        },
        now,
    );

    assert_eq!(
        pending.take_next_work(now),
        Some(DaemonWorkRequest::StartupCheck)
    );
    assert_eq!(
        pending.take_next_work(now),
        Some(DaemonWorkRequest::ChangedPaths {
            changed_paths: vec![changed],
        })
    );
    assert!(pending.dirty_roots.is_empty());
}

#[test]
fn pending_path_overflow_marks_one_root_dirty_after_the_quiet_window() {
    let first_root = PathBuf::from("/first");
    let second_root = PathBuf::from("/second");
    let now = Instant::now();
    let mut pending = PendingDaemonWork::new(vec![first_root.clone(), second_root]);
    pending.changed_watch_paths = BoundedPathSet::new(1, usize::MAX);
    pending.absorb_request(
        DaemonWorkRequest::ChangedPaths {
            changed_paths: vec![first_root.join("one"), first_root.join("two")],
        },
        now,
    );

    assert!(pending.take_next_work(now).is_none());
    assert_eq!(
        pending.take_next_work(now + DIRTY_ROOT_QUIET_WINDOW),
        Some(DaemonWorkRequest::DirtyRootRecovery {
            roots: vec![first_root],
        })
    );
}

#[test]
fn repeated_overflow_while_recovering_keeps_one_follow_up() {
    let root = PathBuf::from("/root");
    let now = Instant::now();
    let mut pending = PendingDaemonWork::new(vec![root.clone()]);
    pending.request_dirty_roots(vec![root.clone()], now);
    assert!(matches!(
        pending.take_next_work(now + DIRTY_ROOT_QUIET_WINDOW),
        Some(DaemonWorkRequest::DirtyRootRecovery { .. })
    ));

    pending.request_dirty_roots(vec![root.clone()], now + DIRTY_ROOT_QUIET_WINDOW);
    let first_finished_at = now + DIRTY_ROOT_QUIET_WINDOW;
    pending.finish_dirty_root_recovery(&[root.clone()], true, first_finished_at);

    assert!(pending
        .take_next_work(first_finished_at + DIRTY_ROOT_RETRY_BASE - Duration::from_millis(1))
        .is_none());
    assert_eq!(
        pending.take_next_work(first_finished_at + DIRTY_ROOT_RETRY_BASE),
        Some(DaemonWorkRequest::DirtyRootRecovery { roots: vec![root] })
    );
}

#[test]
fn new_overflow_does_not_shorten_an_existing_recovery_backoff() {
    let root = PathBuf::from("/root");
    let now = Instant::now();
    let mut pending = PendingDaemonWork::new(vec![root.clone()]);
    pending.request_dirty_roots(vec![root.clone()], now);
    let first_started_at = now + DIRTY_ROOT_QUIET_WINDOW;
    assert!(matches!(
        pending.take_next_work(first_started_at),
        Some(DaemonWorkRequest::DirtyRootRecovery { .. })
    ));
    pending.request_dirty_roots(vec![root.clone()], first_started_at);
    pending.finish_dirty_root_recovery(&[root.clone()], true, first_started_at);

    pending.request_dirty_roots(
        vec![root.clone()],
        first_started_at + Duration::from_secs(1),
    );

    assert!(pending
        .take_next_work(first_started_at + DIRTY_ROOT_QUIET_WINDOW)
        .is_none());
    assert!(matches!(
        pending.take_next_work(first_started_at + DIRTY_ROOT_RETRY_BASE),
        Some(DaemonWorkRequest::DirtyRootRecovery { roots }) if roots == vec![root]
    ));
}

#[test]
fn failed_dirty_recovery_uses_capped_backoff() {
    assert_eq!(dirty_root_retry_delay(1), Duration::from_secs(30));
    assert_eq!(dirty_root_retry_delay(2), Duration::from_secs(60));
    assert_eq!(dirty_root_retry_delay(100), DIRTY_ROOT_RETRY_MAX);
}

#[test]
fn healthy_idle_queue_has_no_periodic_work_even_after_twenty_four_hours() {
    let now = Instant::now();
    let mut pending = PendingDaemonWork::new(vec![PathBuf::from("/root")]);

    assert!(pending
        .take_next_work(now + Duration::from_secs(24 * 60 * 60))
        .is_none());
    assert!(pending.next_deadline().is_none());
}

#[test]
fn concurrent_core_shutdown_joins_worker_once() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("search.sqlite");
    let core = Arc::new(
        SearchDaemonCore::new(
            database_path,
            SearchIndexConfig {
                roots: Vec::new(),
                ..SearchIndexConfig::default()
            },
        )
        .unwrap(),
    );

    let first = {
        let core = Arc::clone(&core);
        std::thread::spawn(move || core.shutdown())
    };
    let second = {
        let core = Arc::clone(&core);
        std::thread::spawn(move || core.shutdown())
    };

    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
}

#[test]
fn daemon_rejects_configuration_outside_the_service_budget_before_opening_database() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("search.sqlite");
    let invalid = SearchIndexConfig {
        roots: vec![PathBuf::from("/tmp"); crate::config::MAX_CONFIGURED_ROOTS + 1],
        ..SearchIndexConfig::default()
    };

    assert!(SearchDaemonCore::new(database_path.clone(), invalid).is_err());
    assert!(!database_path.exists());
}

#[test]
fn daemon_compacts_fragmented_full_text_storage_before_startup_returns() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("search.sqlite");
    drop(SearchDatabase::open(&database_path).unwrap());
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute(
            "INSERT INTO file_search_fts(file_search_fts, rank) VALUES('automerge', 0)",
            [],
        )
        .unwrap();
    for index in 0..4 {
        connection
            .execute(
                "INSERT INTO file_search_fts(rowid, path, name, content)
                 VALUES(?1, ?2, ?3, 'needle')",
                rusqlite::params![index + 1, format!("/tmp/{index}"), format!("file-{index}")],
            )
            .unwrap();
    }
    let segment_count = || {
        connection
            .query_row(
                "SELECT COUNT(DISTINCT segid) FROM file_search_fts_idx",
                [],
                |row| row.get::<_, u64>(0),
            )
            .unwrap()
    };
    assert!(segment_count() > 1);

    let core = SearchDaemonCore::new(
        database_path,
        SearchIndexConfig {
            roots: Vec::new(),
            ..SearchIndexConfig::default()
        },
    )
    .unwrap();

    assert_eq!(segment_count(), 1);
    core.shutdown().unwrap();
}
