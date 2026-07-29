use std::fs;

use rusqlite::Connection;
use tempfile::tempdir;

use super::{SearchDatabase, SearchError, SCHEMA_VERSION};

#[test]
fn future_schema_is_rejected_before_writer_setup_or_migration() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("search.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE sentinel (value TEXT NOT NULL);
             INSERT INTO sentinel VALUES ('preserved');",
        )
        .unwrap();
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    drop(connection);

    let original_bytes = fs::read(&database_path).unwrap();
    let error = match SearchDatabase::open(&database_path) {
        Ok(_) => panic!("future schema opened successfully"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SearchError::UnsupportedDatabaseSchema {
            found,
            supported,
        } if found == SCHEMA_VERSION + 1 && supported == SCHEMA_VERSION
    ));
    assert_eq!(fs::read(&database_path).unwrap(), original_bytes);
    assert!(!database_path.with_file_name("search.sqlite-wal").exists());
    assert!(!database_path.with_file_name("search.sqlite-shm").exists());

    let connection = Connection::open(&database_path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        SCHEMA_VERSION + 1
    );
    assert_eq!(
        connection
            .query_row("SELECT value FROM sentinel", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "preserved"
    );
}
