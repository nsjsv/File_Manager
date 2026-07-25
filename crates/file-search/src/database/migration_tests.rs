use rusqlite::{params, Connection};
use tempfile::tempdir;

use crate::extractor::ExtractionStatus;

use super::SearchDatabase;

#[test]
fn path_migration_failure_rolls_back_every_rebuilt_table() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("search.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE files (
                path TEXT PRIMARY KEY,
                parent_path TEXT NOT NULL,
                display_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                size INTEGER NOT NULL,
                modified_ms INTEGER,
                accessed_ms INTEGER,
                created_ms INTEGER,
                mime_type TEXT,
                content_status TEXT NOT NULL,
                tombstoned INTEGER NOT NULL DEFAULT 0,
                device INTEGER,
                inode INTEGER,
                mtime_ns INTEGER,
                ctime_ns INTEGER,
                observation_state TEXT NOT NULL DEFAULT 'observable'
                    CHECK (observation_state IN ('observable', 'inaccessible')),
                scan_generation INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE file_stage_state (
                path TEXT PRIMARY KEY,
                metadata_stage_state TEXT NOT NULL,
                content_stage_state TEXT NOT NULL
             );
             CREATE TABLE directory_snapshots (
                path TEXT PRIMARY KEY,
                parent_path TEXT NOT NULL,
                root_path TEXT NOT NULL,
                device INTEGER NOT NULL,
                inode INTEGER NOT NULL,
                mtime_ns INTEGER NOT NULL,
                ctime_ns INTEGER NOT NULL,
                observation_state TEXT NOT NULL DEFAULT 'observable'
                    CHECK (observation_state IN ('observable', 'inaccessible'))
             );
             CREATE VIRTUAL TABLE file_search_fts
                USING fts5(path UNINDEXED, name, content);
             CREATE TABLE directory_snapshots_path_migration (sentinel INTEGER);
             PRAGMA user_version = 7;",
        )
        .unwrap();
    let content_status = serde_json::to_string(&ExtractionStatus::Indexed).unwrap();
    connection
        .execute(
            "INSERT INTO files (
                path, parent_path, display_name, kind, size, content_status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "/tmp/note.txt",
                "/tmp",
                "note.txt",
                "file",
                4_i64,
                content_status,
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO file_stage_state
                (path, metadata_stage_state, content_stage_state)
             VALUES (?1, 'complete', 'complete')",
            ["/tmp/note.txt"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO directory_snapshots (
                path, parent_path, root_path, device, inode, mtime_ns, ctime_ns
             ) VALUES (?1, ?2, ?3, 1, 2, 3, 4)",
            ["/tmp", "/", "/tmp"],
        )
        .unwrap();
    drop(connection);

    assert!(SearchDatabase::open(&database_path).is_err());

    let connection = Connection::open(&database_path).unwrap();
    let schema_version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(schema_version, 7);
    for (table, expected_path) in [
        ("files", "/tmp/note.txt"),
        ("file_stage_state", "/tmp/note.txt"),
        ("directory_snapshots", "/tmp"),
    ] {
        let (path_type, path): (String, String) = connection
            .query_row(
                &format!("SELECT typeof(path), path FROM {table} LIMIT 1"),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(path_type, "text", "table {table}");
        assert_eq!(path, expected_path, "table {table}");
    }
    let partial_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table'
               AND name IN ('files_path_migration', 'file_stage_state_path_migration')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(partial_tables, 0);
}
