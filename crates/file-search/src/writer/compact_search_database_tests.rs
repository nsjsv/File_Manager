use rusqlite::Connection;
use tempfile::tempdir;

use crate::database::SearchDatabase;

use super::IndexWriter;

#[test]
fn search_database_compaction_returns_sqlite_errors_without_stopping_writer() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("search.sqlite");
    let database = SearchDatabase::open(&database_path).unwrap();
    Connection::open(&database_path)
        .unwrap()
        .execute_batch("DROP TABLE file_search_fts")
        .unwrap();
    let writer = IndexWriter::spawn(database);

    assert!(writer.compact_search_database().is_err());
    assert_eq!(writer.count().unwrap(), 0);
}
