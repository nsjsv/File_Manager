use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use file_core::ScanWarning;
use tokio_util::sync::CancellationToken;

use super::cache::clear_query_cache;
use super::catalog::SearchCatalogRecord;
use super::crawl::{
    crawl_search_records, crawl_search_records_with_callback,
    crawl_selected_search_records_with_progress, SearchCrawlOptions,
};
use super::extractor::{extract_media_document, extract_text_document};
use super::full_text::TantivyIndexWriter;
use super::manifest::{current_time_ms, SearchIndexManifest};
use super::store::{
    self, catalog_dir_size, prepare_catalog_dir, replace_catalog_dir, CatalogWriteSession,
};
use super::types::{
    FileSearchIndexFailure, FileSearchIndexMode, FileSearchIndexOptions, FileSearchIndexOutcome,
    FileSearchIndexProgress,
};
use crate::profile::MediaMetadataScope;
use crate::IndexError;

enum SearchIndexBuildScope {
    Root,
    Selected(Vec<PathBuf>),
}

struct PreviousSearchIndex {
    manifest: SearchIndexManifest,
    records: Vec<SearchCatalogRecord>,
    failures: Vec<FileSearchIndexFailure>,
}

fn crawl_excluded_index_dir(options: &FileSearchIndexOptions, index_dir: &Path) -> PathBuf {
    options
        .excluded_index_dir
        .clone()
        .unwrap_or_else(|| index_dir.to_path_buf())
}

pub(super) fn build_file_search_index_blocking(
    root: &Path,
    index_dir: &Path,
    options: FileSearchIndexOptions,
) -> Result<FileSearchIndexOutcome, IndexError> {
    if options.mode == FileSearchIndexMode::Incremental {
        return build_file_search_index_incremental_blocking(
            root,
            index_dir,
            SearchIndexBuildScope::Root,
            options,
            None,
            &mut |_| {},
        );
    }

    build_file_search_index_full_rebuild_blocking(root, index_dir, options, None)
}

fn build_file_search_index_full_rebuild_blocking(
    root: &Path,
    index_dir: &Path,
    options: FileSearchIndexOptions,
    cancel: Option<CancellationToken>,
) -> Result<FileSearchIndexOutcome, IndexError> {
    let pending_index_dir = index_dir.with_extension("building");
    prepare_catalog_dir(root, &pending_index_dir)?;
    let mut catalog = CatalogWriteSession::create(&pending_index_dir)?;
    let mut tantivy = tantivy_writer_for_indexing(
        &pending_index_dir,
        options.content_index_enabled,
        options.media_metadata_scope,
    )?;
    let mut skipped = Vec::new();
    let crawl_skipped = crawl_search_records_with_callback(
        root,
        &SearchCrawlOptions {
            include_hidden: options.include_hidden,
            exclude_patterns: options.exclude_patterns.clone(),
            directory_error_policy: options.directory_error_policy,
            excluded_index_dir: Some(crawl_excluded_index_dir(&options, index_dir)),
            throttle: true,
            cancel,
        },
        |record| {
            if let Some(writer) = tantivy.as_mut() {
                write_full_text_record(
                    writer,
                    &record,
                    options.content_index_enabled,
                    options.content_max_file_bytes,
                    options.media_metadata_scope,
                    &mut skipped,
                )?;
            }
            catalog.add_record(&record)
        },
    )?;
    skipped.extend(crawl_skipped);
    if let Some(writer) = tantivy {
        writer.finish()?;
    }
    let failures = warnings_to_failures(&skipped);
    let indexed_count = catalog.record_count();
    let mut manifest = SearchIndexManifest::new(
        root,
        options.include_hidden,
        &options.exclude_patterns,
        options.directory_error_policy,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
        indexed_count,
        failures.len(),
        None,
    );
    catalog.finish(&mut manifest, &failures)?;
    replace_catalog_dir(index_dir, &pending_index_dir)?;
    manifest.index_size_bytes = catalog_dir_size(index_dir)?;
    clear_query_cache();

    Ok(FileSearchIndexOutcome {
        root: root.to_path_buf(),
        index_dir: index_dir.to_path_buf(),
        indexed_count,
        index_size_bytes: manifest.index_size_bytes,
        updated_at_ms: manifest.updated_at_ms,
        failed_count: failures.len(),
        skipped,
    })
}

fn tantivy_writer_for_indexing(
    index_dir: &Path,
    content_index_enabled: bool,
    media_metadata_scope: MediaMetadataScope,
) -> Result<Option<TantivyIndexWriter>, IndexError> {
    if content_index_enabled || media_metadata_scope.includes_media() {
        Ok(Some(TantivyIndexWriter::create(index_dir)?))
    } else {
        Ok(None)
    }
}

fn write_collected_records_to_pending_index(
    pending_index_dir: &Path,
    records: &[SearchCatalogRecord],
    content_index_enabled: bool,
    content_max_file_bytes: u64,
    media_metadata_scope: MediaMetadataScope,
    skipped: &mut Vec<ScanWarning>,
    cancel: Option<&CancellationToken>,
) -> Result<CatalogWriteSession, IndexError> {
    let mut catalog = CatalogWriteSession::create(pending_index_dir)?;
    let mut tantivy = tantivy_writer_for_indexing(
        pending_index_dir,
        content_index_enabled,
        media_metadata_scope,
    )?;
    for record in records {
        ensure_not_cancelled(cancel)?;
        if let Some(writer) = tantivy.as_mut() {
            write_full_text_record(
                writer,
                record,
                content_index_enabled,
                content_max_file_bytes,
                media_metadata_scope,
                skipped,
            )?;
        }
        catalog.add_record(record)?;
    }
    if let Some(writer) = tantivy {
        writer.finish()?;
    }
    Ok(catalog)
}

fn ensure_not_cancelled(cancel: Option<&CancellationToken>) -> Result<(), IndexError> {
    if cancel.is_some_and(CancellationToken::is_cancelled) {
        Err(IndexError::Cancelled)
    } else {
        Ok(())
    }
}

fn write_full_text_record(
    writer: &mut TantivyIndexWriter,
    record: &SearchCatalogRecord,
    content_index_enabled: bool,
    content_max_file_bytes: u64,
    media_metadata_scope: MediaMetadataScope,
    skipped: &mut Vec<ScanWarning>,
) -> Result<(), IndexError> {
    if content_index_enabled {
        match extract_text_document(record, content_max_file_bytes) {
            Ok(Some(document)) => writer.add_text_document(&document)?,
            Ok(None) => {}
            Err(warning) => skipped.push(warning),
        }
    }
    if media_metadata_scope.includes_media() {
        match extract_media_document(record, media_metadata_scope) {
            Ok(Some(document)) => writer.add_media_document(&document)?,
            Ok(None) => {}
            Err(warning) => skipped.push(warning),
        }
    }
    Ok(())
}

pub(super) fn build_file_search_index_for_paths_blocking(
    root: &Path,
    index_dir: &Path,
    selected_paths: Vec<PathBuf>,
    options: FileSearchIndexOptions,
) -> Result<FileSearchIndexOutcome, IndexError> {
    build_file_search_index_for_paths_blocking_with_progress(
        root,
        index_dir,
        selected_paths,
        options,
        None,
        &mut |_| {},
    )
}

pub(super) fn build_file_search_index_for_paths_blocking_with_progress(
    root: &Path,
    index_dir: &Path,
    selected_paths: Vec<PathBuf>,
    options: FileSearchIndexOptions,
    cancel: Option<CancellationToken>,
    progress: &mut impl FnMut(FileSearchIndexProgress),
) -> Result<FileSearchIndexOutcome, IndexError> {
    if options.mode == FileSearchIndexMode::Incremental {
        return build_file_search_index_incremental_blocking(
            root,
            index_dir,
            SearchIndexBuildScope::Selected(selected_paths),
            options,
            cancel,
            progress,
        );
    }

    let selected_paths_include_root = selected_paths.iter().any(|path| path == root);

    if selected_paths_include_root {
        let outcome =
            build_file_search_index_full_rebuild_blocking(root, index_dir, options, cancel)?;
        progress(FileSearchIndexProgress::IndexedPaths {
            completed_paths: 1,
            total_paths: 1,
            indexed_count: outcome.indexed_count,
        });
        return Ok(outcome);
    }

    let pending_index_dir = index_dir.with_extension("building");
    prepare_catalog_dir(root, &pending_index_dir)?;
    let total_paths = selected_paths.len().max(1);
    let crawl_cancel = cancel.clone();
    let (records, skipped) = crawl_selected_search_records_with_progress(
        root,
        &selected_paths,
        &SearchCrawlOptions {
            include_hidden: options.include_hidden,
            exclude_patterns: options.exclude_patterns.clone(),
            directory_error_policy: options.directory_error_policy,
            excluded_index_dir: Some(crawl_excluded_index_dir(&options, index_dir)),
            throttle: true,
            cancel: crawl_cancel,
        },
        |completed_paths, indexed_count| {
            progress(FileSearchIndexProgress::IndexedPaths {
                completed_paths,
                total_paths,
                indexed_count,
            });
        },
    )?;
    let mut skipped = skipped;
    let catalog = write_collected_records_to_pending_index(
        &pending_index_dir,
        &records,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
        &mut skipped,
        cancel.as_ref(),
    )?;
    let failures = warnings_to_failures(&skipped);
    let indexed_count = catalog.record_count();
    let mut manifest = SearchIndexManifest::new(
        root,
        options.include_hidden,
        &options.exclude_patterns,
        options.directory_error_policy,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
        indexed_count,
        failures.len(),
        None,
    );
    catalog.finish(&mut manifest, &failures)?;
    replace_catalog_dir(index_dir, &pending_index_dir)?;
    manifest.index_size_bytes = catalog_dir_size(index_dir)?;
    clear_query_cache();

    Ok(FileSearchIndexOutcome {
        root: root.to_path_buf(),
        index_dir: index_dir.to_path_buf(),
        indexed_count,
        index_size_bytes: manifest.index_size_bytes,
        updated_at_ms: manifest.updated_at_ms,
        failed_count: failures.len(),
        skipped,
    })
}

fn build_file_search_index_incremental_blocking(
    root: &Path,
    index_dir: &Path,
    scope: SearchIndexBuildScope,
    options: FileSearchIndexOptions,
    cancel: Option<CancellationToken>,
    progress: &mut impl FnMut(FileSearchIndexProgress),
) -> Result<FileSearchIndexOutcome, IndexError> {
    let previous = previous_search_index(
        index_dir,
        root,
        options.include_hidden,
        &options.exclude_patterns,
        options.directory_error_policy,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
    );
    let built_at_ms = previous.as_ref().map(|index| index.manifest.built_at_ms);
    let previous_records = previous
        .as_ref()
        .map(|index| index.records.as_slice())
        .unwrap_or(&[]);
    let previous_failures = previous
        .as_ref()
        .map(|index| index.failures.as_slice())
        .unwrap_or(&[]);

    let mut rescanned_roots = None;
    let (records, skipped) = match scope {
        SearchIndexBuildScope::Root => {
            let crawl_cancel = cancel.clone();
            let (scanned_records, skipped) = crawl_search_records(
                root,
                &SearchCrawlOptions {
                    include_hidden: options.include_hidden,
                    exclude_patterns: options.exclude_patterns.clone(),
                    directory_error_policy: options.directory_error_policy,
                    excluded_index_dir: Some(crawl_excluded_index_dir(&options, index_dir)),
                    throttle: true,
                    cancel: crawl_cancel,
                },
            )?;
            progress(FileSearchIndexProgress::IndexedPaths {
                completed_paths: 1,
                total_paths: 1,
                indexed_count: scanned_records.len(),
            });
            (
                merge_root_records(previous_records, scanned_records),
                skipped,
            )
        }
        SearchIndexBuildScope::Selected(selected_paths) => {
            let selected_roots = non_nested_index_roots(&selected_paths);
            let total_paths = selected_roots.len().max(1);
            let crawl_cancel = cancel.clone();
            let (scanned_records, skipped) = crawl_selected_search_records_with_progress(
                root,
                &selected_roots,
                &SearchCrawlOptions {
                    include_hidden: options.include_hidden,
                    exclude_patterns: options.exclude_patterns.clone(),
                    directory_error_policy: options.directory_error_policy,
                    excluded_index_dir: Some(crawl_excluded_index_dir(&options, index_dir)),
                    throttle: true,
                    cancel: crawl_cancel,
                },
                |completed_paths, indexed_count| {
                    progress(FileSearchIndexProgress::IndexedPaths {
                        completed_paths,
                        total_paths,
                        indexed_count,
                    });
                },
            )?;
            if selected_roots.is_empty() {
                progress(FileSearchIndexProgress::IndexedPaths {
                    completed_paths: 1,
                    total_paths,
                    indexed_count: previous_records.len(),
                });
            }
            let merged_records =
                merge_selected_records(previous_records, &selected_roots, scanned_records);
            rescanned_roots = Some(selected_roots);
            (merged_records, skipped)
        }
    };

    let indexed_count = changed_record_count(previous_records, &records);
    write_records_to_index(
        root,
        index_dir,
        options.include_hidden,
        &options.exclude_patterns,
        records,
        skipped,
        previous_failures,
        rescanned_roots.as_deref(),
        built_at_ms,
        indexed_count,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
        options.directory_error_policy,
        cancel.as_ref(),
    )
}

fn previous_search_index(
    index_dir: &Path,
    root: &Path,
    include_hidden: bool,
    exclude_patterns: &[String],
    directory_error_policy: super::types::DirectoryErrorPolicy,
    content_index_enabled: bool,
    content_max_file_bytes: u64,
    media_metadata_scope: MediaMetadataScope,
) -> Option<PreviousSearchIndex> {
    let (manifest, records) = store::load_catalog(
        index_dir,
        root,
        include_hidden,
        exclude_patterns,
        directory_error_policy,
        content_index_enabled,
        content_max_file_bytes,
        media_metadata_scope,
    )
    .ok()?;
    let failures = store::read_failures(index_dir).unwrap_or_default();
    Some(PreviousSearchIndex {
        manifest,
        records,
        failures,
    })
}

fn write_records_to_index(
    root: &Path,
    index_dir: &Path,
    include_hidden: bool,
    exclude_patterns: &[String],
    records: Vec<SearchCatalogRecord>,
    skipped: Vec<ScanWarning>,
    previous_failures: &[FileSearchIndexFailure],
    rescanned_roots: Option<&[PathBuf]>,
    built_at_ms: Option<i64>,
    indexed_count: usize,
    content_index_enabled: bool,
    content_max_file_bytes: u64,
    media_metadata_scope: MediaMetadataScope,
    directory_error_policy: super::types::DirectoryErrorPolicy,
    cancel: Option<&CancellationToken>,
) -> Result<FileSearchIndexOutcome, IndexError> {
    let pending_index_dir = index_dir.with_extension("building");
    prepare_catalog_dir(root, &pending_index_dir)?;
    let mut skipped = skipped;
    let catalog = write_collected_records_to_pending_index(
        &pending_index_dir,
        &records,
        content_index_enabled,
        content_max_file_bytes,
        media_metadata_scope,
        &mut skipped,
        cancel,
    )?;
    let failures = merge_scan_failures(&skipped, previous_failures, rescanned_roots);
    let mut manifest = SearchIndexManifest::new(
        root,
        include_hidden,
        exclude_patterns,
        directory_error_policy,
        content_index_enabled,
        content_max_file_bytes,
        media_metadata_scope,
        catalog.record_count(),
        failures.len(),
        built_at_ms,
    );
    catalog.finish(&mut manifest, &failures)?;
    replace_catalog_dir(index_dir, &pending_index_dir)?;
    manifest.index_size_bytes = catalog_dir_size(index_dir)?;
    clear_query_cache();

    Ok(FileSearchIndexOutcome {
        root: root.to_path_buf(),
        index_dir: index_dir.to_path_buf(),
        indexed_count,
        index_size_bytes: manifest.index_size_bytes,
        updated_at_ms: manifest.updated_at_ms,
        failed_count: failures.len(),
        skipped,
    })
}

fn merge_root_records(
    previous_records: &[SearchCatalogRecord],
    scanned_records: Vec<SearchCatalogRecord>,
) -> Vec<SearchCatalogRecord> {
    let previous_by_key = previous_records_by_key(previous_records);
    scanned_records
        .into_iter()
        .map(|record| reuse_unchanged_record(&previous_by_key, record))
        .collect()
}

fn merge_selected_records(
    previous_records: &[SearchCatalogRecord],
    selected_roots: &[PathBuf],
    scanned_records: Vec<SearchCatalogRecord>,
) -> Vec<SearchCatalogRecord> {
    let previous_by_key = previous_records_by_key(previous_records);
    let mut seen_keys = HashSet::new();
    let mut merged_records = previous_records
        .iter()
        .filter(|record| !selected_roots_cover_path(selected_roots, &record.path))
        .filter_map(|record| {
            seen_keys
                .insert(record.storage_key.clone())
                .then(|| record.clone())
        })
        .collect::<Vec<_>>();

    for scanned_record in scanned_records {
        let record = reuse_unchanged_record(&previous_by_key, scanned_record);
        if seen_keys.insert(record.storage_key.clone()) {
            merged_records.push(record);
        }
    }
    merged_records
}

fn previous_records_by_key(records: &[SearchCatalogRecord]) -> HashMap<&str, &SearchCatalogRecord> {
    records
        .iter()
        .map(|record| (record.storage_key.as_str(), record))
        .collect()
}

fn reuse_unchanged_record(
    previous_by_key: &HashMap<&str, &SearchCatalogRecord>,
    scanned_record: SearchCatalogRecord,
) -> SearchCatalogRecord {
    if let Some(&previous_record) = previous_by_key.get(scanned_record.storage_key.as_str()) {
        if record_metadata_matches(previous_record, &scanned_record) {
            return previous_record.clone();
        }
    }
    scanned_record
}

fn record_metadata_matches(left: &SearchCatalogRecord, right: &SearchCatalogRecord) -> bool {
    left.mtime_ms == right.mtime_ms && left.size_bytes == right.size_bytes
}

fn non_nested_index_roots(selected_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = selected_paths.to_vec();
    roots.sort_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });

    let mut reduced_roots = Vec::new();
    for root in roots {
        if reduced_roots
            .iter()
            .any(|parent: &PathBuf| root.starts_with(parent))
        {
            continue;
        }
        reduced_roots.push(root);
    }
    reduced_roots
}

fn selected_roots_cover_path(selected_roots: &[PathBuf], path: &Path) -> bool {
    selected_roots.iter().any(|root| path.starts_with(root))
}

fn changed_record_count(previous: &[SearchCatalogRecord], next: &[SearchCatalogRecord]) -> usize {
    let previous = previous
        .iter()
        .map(|record| {
            (
                record.storage_key.as_str(),
                (record.mtime_ms, record.size_bytes),
            )
        })
        .collect::<HashMap<_, _>>();
    let next_keys = next
        .iter()
        .map(|record| record.storage_key.as_str())
        .collect::<HashSet<_>>();
    let mut changed = next
        .iter()
        .filter(|record| {
            previous
                .get(record.storage_key.as_str())
                .is_none_or(|metadata| *metadata != (record.mtime_ms, record.size_bytes))
        })
        .count();
    changed += previous
        .keys()
        .filter(|key| !next_keys.contains(*key))
        .count();
    changed
}

fn merge_scan_failures(
    warnings: &[ScanWarning],
    previous_failures: &[FileSearchIndexFailure],
    rescanned_roots: Option<&[PathBuf]>,
) -> Vec<FileSearchIndexFailure> {
    let now = current_time_ms();
    let previous_by_path = previous_failures
        .iter()
        .map(|failure| (failure.path.clone(), failure))
        .collect::<HashMap<_, _>>();
    let mut merged_failures = HashMap::new();

    if let Some(roots) = rescanned_roots {
        for failure in previous_failures
            .iter()
            .filter(|failure| !selected_roots_cover_path(roots, &failure.path))
        {
            merged_failures.insert(failure.path.clone(), failure.clone());
        }
    }

    for warning in warnings {
        let failure = previous_by_path
            .get(&warning.path)
            .map(|previous| FileSearchIndexFailure {
                path: warning.path.clone(),
                message: warning.message.clone(),
                first_failed_at_ms: previous.first_failed_at_ms,
                last_failed_at_ms: now,
                retry_count: previous.retry_count.saturating_add(1),
            })
            .unwrap_or_else(|| FileSearchIndexFailure {
                path: warning.path.clone(),
                message: warning.message.clone(),
                first_failed_at_ms: now,
                last_failed_at_ms: now,
                retry_count: 1,
            });
        merged_failures.insert(warning.path.clone(), failure);
    }

    let mut failures = merged_failures.into_values().collect::<Vec<_>>();
    failures.sort_by(|left, right| left.path.cmp(&right.path));
    failures
}

fn warnings_to_failures(warnings: &[ScanWarning]) -> Vec<FileSearchIndexFailure> {
    merge_scan_failures(warnings, &[], None)
}
