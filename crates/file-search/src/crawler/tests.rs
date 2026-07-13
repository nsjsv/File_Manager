use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

use crate::config::SearchIndexConfig;
use crate::database::{EntryStageProgress, IndexedEntryStageState, SearchDatabase};
use crate::error::SearchError;
use crate::extractor::{CommandSpec, ExtractionExecutionMode, ExtractionStatus};
use crate::model::SearchQuery;
use crate::writer::IndexWriter;

use super::{observed_file_signature, SearchIndexer};

fn config_for(root: &std::path::Path) -> SearchIndexConfig {
    SearchIndexConfig {
        roots: vec![root.to_path_buf()],
        excluded_paths: Vec::new(),
        content_indexing_enabled: true,
        max_extract_bytes: 1024,
    }
}

/// Opens a writer over a fresh on-disk database in its own directory, kept
/// separate from the content root so the database file is never itself
/// crawled.
fn writer_in(db_dir: &tempfile::TempDir) -> Arc<IndexWriter> {
    let db_path = db_dir.path().join("search.sqlite");
    Arc::new(IndexWriter::spawn(SearchDatabase::open(&db_path).unwrap()))
}

#[test]
fn changed_text_file_pipeline_plan_exposes_stage_boundaries() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let path = content.path().join("note.txt");
    fs::write(&path, "hello").unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    let metadata = fs::metadata(&path).unwrap();

    let changed_file_pipeline =
        indexer.plan_changed_file_pipeline(&path, &metadata, observed_file_signature(&metadata));

    assert_eq!(
        changed_file_pipeline.stage1_visible_fields.display_name,
        "note.txt"
    );
    assert_eq!(
        changed_file_pipeline.stage1_visible_fields.parent_path,
        content.path().to_path_buf()
    );
    assert_eq!(
        changed_file_pipeline.stage1_visible_fields.signature.size,
        5
    );
    assert_eq!(
        changed_file_pipeline
            .stage2_metadata_shape
            .mime_type
            .as_deref(),
        Some("text/plain")
    );
    assert_eq!(
        changed_file_pipeline
            .stage3_content_plan
            .extraction_plan
            .execution_mode,
        ExtractionExecutionMode::PlainTextInProcess
    );
}

#[test]
fn document_pipeline_plan_uses_isolated_subprocess_execution() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let path = content.path().join("report.pdf");
    fs::write(&path, b"placeholder").unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    let metadata = fs::metadata(&path).unwrap();

    let changed_file_pipeline =
        indexer.plan_changed_file_pipeline(&path, &metadata, observed_file_signature(&metadata));

    assert_eq!(
        changed_file_pipeline
            .stage3_content_plan
            .extraction_plan
            .execution_mode,
        ExtractionExecutionMode::IsolatedSubprocess {
            command: CommandSpec {
                program: "pdftotext".to_owned(),
                args: vec!["{}".to_owned(), "-".to_owned()],
            },
            timeout: Duration::from_secs(10),
        }
    );
}

#[test]
fn oversized_file_pipeline_plan_skips_stage3_before_extraction() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let path = content.path().join("large.txt");
    fs::write(&path, "needle").unwrap();

    let writer = writer_in(&db_dir);
    let mut config = config_for(content.path());
    config.max_extract_bytes = 2;
    let indexer = SearchIndexer::new(Arc::clone(&writer), config);
    let metadata = fs::metadata(&path).unwrap();

    let changed_file_pipeline =
        indexer.plan_changed_file_pipeline(&path, &metadata, observed_file_signature(&metadata));

    assert_eq!(
        changed_file_pipeline
            .stage3_content_plan
            .extraction_plan
            .execution_mode,
        ExtractionExecutionMode::SkipNow {
            skip_reason: ExtractionStatus::TooLarge,
        }
    );
}

#[tokio::test]
async fn indexes_text_and_skips_hidden_directories() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    fs::write(content.path().join("visible.txt"), "needle visible").unwrap();
    fs::create_dir(content.path().join(".hidden")).unwrap();
    fs::write(
        content.path().join(".hidden").join("hidden.txt"),
        "needle hidden",
    )
    .unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    indexer.rebuild().await.unwrap();

    let reader = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    let batch = reader.search(&SearchQuery::global(1, "needle")).unwrap();
    assert_eq!(batch.hits.len(), 1);
    assert_eq!(batch.hits[0].display_name, "visible.txt");
}

#[tokio::test]
async fn rebuild_tombstones_deleted_files() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let path = content.path().join("gone.txt");
    fs::write(&path, "needle").unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    indexer.rebuild().await.unwrap();
    fs::remove_file(&path).unwrap();
    indexer.rebuild().await.unwrap();

    let reader = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    let batch = reader.search(&SearchQuery::global(1, "needle")).unwrap();
    assert!(batch.hits.is_empty());
}

#[tokio::test]
async fn unchanged_files_are_skipped_on_second_pass() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    for index in 0..5 {
        fs::write(content.path().join(format!("file-{index}.txt")), "needle").unwrap();
    }

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));

    let first = indexer.rebuild().await.unwrap();
    assert_eq!(first.scanned, 5);
    assert_eq!(first.reindexed, 5);
    assert_eq!(first.skipped, 0);

    // Nothing changed on disk, so the second pass re-extracts nothing.
    let second = indexer.rebuild().await.unwrap();
    assert_eq!(second.scanned, 0);
    assert_eq!(second.checked, 6);
    assert_eq!(second.changed, 0);
    assert_eq!(second.reindexed, 0);
    assert_eq!(second.skipped, 5);
    assert_eq!(second.directories_enumerated, 0);
    assert_eq!(second.database_mutations, 0);
    assert_eq!(second.content_reads, 0);
    assert_eq!(second.directory_snapshots_changed, 0);
}

#[tokio::test]
async fn warm_start_enumerates_only_the_parent_of_an_offline_new_file() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    fs::write(content.path().join("existing.txt"), "existing").unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    indexer.rebuild().await.unwrap();
    fs::write(content.path().join("new.txt"), "new needle").unwrap();

    let warm = indexer.rebuild().await.unwrap();

    assert_eq!(warm.directories_enumerated, 1);
    assert_eq!(warm.reindexed, 1);
    assert_eq!(warm.content_reads, 1);
    let reader = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    assert_eq!(
        reader
            .search(&SearchQuery::global(1, "needle"))
            .unwrap()
            .hits
            .len(),
        1
    );
}

#[tokio::test]
async fn warm_start_replaces_a_known_directory_with_a_file() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let replaced_path = content.path().join("replaced");
    fs::create_dir(&replaced_path).unwrap();
    fs::write(replaced_path.join("old.txt"), "old content").unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    indexer.rebuild().await.unwrap();

    fs::remove_dir_all(&replaced_path).unwrap();
    fs::write(&replaced_path, "replacement needle").unwrap();

    indexer.rebuild().await.unwrap();

    let reader = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    let hits = reader
        .search(&SearchQuery::global(1, "replaced"))
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, replaced_path);
    assert!(reader
        .search(&SearchQuery::global(2, "old"))
        .unwrap()
        .hits
        .is_empty());
}

#[tokio::test]
async fn unchanged_oversized_files_keep_durable_skip_state() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let path = content.path().join("large.txt");
    fs::write(&path, "needle").unwrap();

    let writer = writer_in(&db_dir);
    let mut config = config_for(content.path());
    config.max_extract_bytes = 2;
    let indexer = SearchIndexer::new(Arc::clone(&writer), config);

    let first = indexer.rebuild().await.unwrap();
    assert_eq!(first.reindexed, 1);

    let database = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    assert_eq!(
        database.content_status(&path).unwrap(),
        Some(ExtractionStatus::TooLarge)
    );
    assert_eq!(
        database.entry_stage_state(&path).unwrap(),
        Some(IndexedEntryStageState {
            metadata: EntryStageProgress::Complete,
            content: EntryStageProgress::Skipped,
        })
    );

    let second = indexer.rebuild().await.unwrap();
    assert_eq!(second.scanned, 0);
    assert_eq!(second.checked, 2);
    assert_eq!(second.reindexed, 0);
    assert_eq!(second.skipped, 1);
}

#[tokio::test]
async fn rebuild_paths_only_scans_the_requested_file_and_keeps_skip_semantics() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let first_path = content.path().join("first.txt");
    let second_path = content.path().join("second.txt");
    fs::write(&first_path, "alpha").unwrap();
    fs::write(&second_path, "beta").unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    indexer.rebuild().await.unwrap();

    let scoped = indexer
        .rebuild_paths(vec![first_path.clone()])
        .await
        .unwrap();
    assert_eq!(scoped.scanned, 1);
    assert_eq!(scoped.reindexed, 0);
    assert_eq!(scoped.skipped, 1);

    let reader = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    let batch = reader.search(&SearchQuery::global(1, "beta")).unwrap();
    assert_eq!(batch.hits.len(), 1);
    assert_eq!(batch.hits[0].display_name, "second.txt");
}

#[tokio::test]
async fn coverage_repair_enumerates_only_the_scope_direct_children() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let nested = content.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(content.path().join("direct.txt"), "direct").unwrap();
    fs::write(nested.join("deep.txt"), "deep").unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    indexer.rebuild().await.unwrap();

    let repair = indexer
        .repair_scopes_with_progress_cancelled(
            vec![content.path().to_path_buf()],
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();

    assert_eq!(repair.checked, 0);
    assert_eq!(repair.directories_enumerated, 1);
    assert_eq!(repair.scanned, 1);
    assert_eq!(repair.reindexed, 0);
}

#[tokio::test]
async fn rebuild_paths_tombstones_deleted_subtrees_without_a_full_rebuild() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let removed_directory = content.path().join("removed");
    fs::create_dir(&removed_directory).unwrap();
    fs::write(removed_directory.join("gone.txt"), "vanish me").unwrap();
    fs::write(content.path().join("kept.txt"), "keep me").unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    indexer.rebuild().await.unwrap();

    fs::remove_dir_all(&removed_directory).unwrap();
    let scoped = indexer
        .rebuild_paths(vec![removed_directory.clone()])
        .await
        .unwrap();
    assert_eq!(scoped.scanned, 0);
    assert_eq!(scoped.reindexed, 0);
    assert_eq!(scoped.skipped, 0);

    let reader = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    assert!(reader
        .search(&SearchQuery::global(1, "vanish"))
        .unwrap()
        .hits
        .is_empty());

    let kept_batch = reader.search(&SearchQuery::global(2, "keep")).unwrap();
    assert_eq!(kept_batch.hits.len(), 1);
    assert_eq!(kept_batch.hits[0].display_name, "kept.txt");
}

#[tokio::test]
async fn modified_file_is_reindexed() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let path = content.path().join("note.txt");
    fs::write(&path, "needle one").unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    indexer.rebuild().await.unwrap();

    // Rewriting with different-length content changes the recorded size, so
    // the signature no longer matches and the file must be re-indexed —
    // independent of mtime resolution, which avoids same-nanosecond flakiness.
    fs::write(&path, "needle two words").unwrap();

    let second = indexer.rebuild().await.unwrap();
    assert_eq!(second.checked, 2);
    assert_eq!(second.changed, 1);
    assert_eq!(second.scanned, 1);
    assert_eq!(second.reindexed, 1);
    assert_eq!(second.skipped, 0);
    assert_eq!(second.directories_enumerated, 0);
    assert_eq!(second.database_mutations, 1);
    assert_eq!(second.content_reads, 1);

    let reader = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    let batch = reader.search(&SearchQuery::global(1, "words")).unwrap();
    assert_eq!(batch.hits.len(), 1);
}

#[tokio::test]
async fn permission_ctime_change_hides_and_recovers_an_unchanged_file() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let path = content.path().join("permission.txt");
    fs::write(&path, "permission needle").unwrap();

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    indexer.rebuild().await.unwrap();

    fs::set_permissions(&path, fs::Permissions::from_mode(0)).unwrap();
    let hidden = indexer.rebuild_paths(vec![path.clone()]).await.unwrap();
    assert_eq!(hidden.reindexed, 1);
    let reader = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    assert!(reader
        .search(&SearchQuery::global(1, "needle"))
        .unwrap()
        .hits
        .is_empty());

    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let recovered = indexer.rebuild_paths(vec![path.clone()]).await.unwrap();
    assert_eq!(recovered.reindexed, 1);
    let hits = reader
        .search(&SearchQuery::global(2, "needle"))
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, path);
}

#[tokio::test]
async fn records_oversized_content_status() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let path = content.path().join("large.txt");
    fs::write(&path, "needle").unwrap();

    let writer = writer_in(&db_dir);
    let mut config = config_for(content.path());
    config.max_extract_bytes = 2;
    let indexer = SearchIndexer::new(Arc::clone(&writer), config);
    indexer.rebuild().await.unwrap();

    let reader = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    let status = reader.content_status(&path).unwrap();
    assert_eq!(status, Some(ExtractionStatus::TooLarge));
    let stage_state = reader.entry_stage_state(&path).unwrap();
    assert_eq!(
        stage_state,
        Some(crate::database::IndexedEntryStageState {
            metadata: crate::database::EntryStageProgress::Complete,
            content: crate::database::EntryStageProgress::Skipped,
        })
    );
}

#[tokio::test]
async fn cancelled_full_and_local_crawls_do_not_tombstone_missing_files() {
    let content = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    let subtree = content.path().join("subtree");
    fs::create_dir(&subtree).unwrap();
    let missing_path = subtree.join("victim.bin");
    fs::write(&missing_path, "victim").unwrap();
    for index in 0..128 {
        fs::write(subtree.join(format!("unchanged-{index}.bin")), "unchanged").unwrap();
    }

    let writer = writer_in(&db_dir);
    let indexer = SearchIndexer::new(Arc::clone(&writer), config_for(content.path()));
    indexer.rebuild().await.unwrap();
    fs::remove_file(&missing_path).unwrap();

    let full_cancellation = CancellationToken::new();
    let cancel_from_full_progress = full_cancellation.clone();
    let full_error = indexer
        .rebuild_with_progress_cancelled(&full_cancellation, move |_| {
            cancel_from_full_progress.cancel();
        })
        .await
        .unwrap_err();
    assert!(matches!(full_error, SearchError::Cancelled));

    let local_cancellation = CancellationToken::new();
    let cancel_from_local_progress = local_cancellation.clone();
    let local_error = indexer
        .rebuild_paths_with_progress_cancelled(vec![subtree], &local_cancellation, move |_| {
            cancel_from_local_progress.cancel()
        })
        .await
        .unwrap_err();
    assert!(matches!(local_error, SearchError::Cancelled));

    let reader = SearchDatabase::open_read_only(&db_dir.path().join("search.sqlite")).unwrap();
    let hits = reader
        .search(&SearchQuery::global(1, "victim"))
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, missing_path);
}
