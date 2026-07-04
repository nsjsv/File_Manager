use std::collections::HashMap;
use std::path::{Path, PathBuf};

use file_core::ScanWarning;
use tokio_util::sync::CancellationToken;

use super::cache::clear_query_cache;
use super::catalog::SearchCatalogRecord;
use super::crawl::{
    crawl_search_records_with_callback, crawl_selected_search_records_with_callback_and_progress,
    SearchCrawlOptions,
};
use super::extractor::{extract_media_document, extract_text_document};
use super::full_text::TantivyIndexWriter;
use super::manifest::{current_time_ms, SearchIndexManifest};
use super::store::{
    self, catalog_dir_size, prepare_catalog_dir, replace_catalog_dir, CatalogWriteSession,
};
use super::types::{
    DirectoryErrorPolicy, FileSearchIndexFailure, FileSearchIndexMode, FileSearchIndexOptions,
    FileSearchIndexOutcome, FileSearchIndexProgress,
};
use crate::profile::MediaMetadataScope;
use crate::IndexError;

const FULL_REBUILD_SELECTED_ROOT_THRESHOLD: usize = 64;

struct PreviousSearchIndex {
    manifest: SearchIndexManifest,
    failures: Vec<FileSearchIndexFailure>,
}

struct PendingIndexWriter {
    catalog: CatalogWriteSession,
    tantivy: Option<TantivyIndexWriter>,
    skipped: Vec<ScanWarning>,
    content_index_enabled: bool,
    content_max_file_bytes: u64,
    media_metadata_scope: MediaMetadataScope,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RecordFingerprint {
    mtime_ms: Option<i64>,
    size_bytes: Option<u64>,
}

impl PendingIndexWriter {
    fn create(
        index_dir: &Path,
        content_index_enabled: bool,
        content_max_file_bytes: u64,
        media_metadata_scope: MediaMetadataScope,
    ) -> Result<Self, IndexError> {
        Ok(Self {
            catalog: CatalogWriteSession::create(index_dir)?,
            tantivy: tantivy_writer_for_indexing(
                index_dir,
                content_index_enabled,
                media_metadata_scope,
            )?,
            skipped: Vec::new(),
            content_index_enabled,
            content_max_file_bytes,
            media_metadata_scope,
        })
    }

    fn add_record(&mut self, record: &SearchCatalogRecord) -> Result<(), IndexError> {
        if let Some(writer) = self.tantivy.as_mut() {
            write_full_text_record(
                writer,
                record,
                self.content_index_enabled,
                self.content_max_file_bytes,
                self.media_metadata_scope,
                &mut self.skipped,
            )?;
        }
        self.catalog.add_record(record)
    }

    fn extend_skipped(&mut self, skipped: Vec<ScanWarning>) {
        self.skipped.extend(skipped);
    }

    fn skipped(&self) -> &[ScanWarning] {
        &self.skipped
    }

    fn record_count(&self) -> usize {
        self.catalog.record_count()
    }

    fn finish(
        self,
        manifest: &mut SearchIndexManifest,
        failures: &[FileSearchIndexFailure],
    ) -> Result<Vec<ScanWarning>, IndexError> {
        let Self {
            catalog,
            tantivy,
            skipped,
            ..
        } = self;
        if let Some(writer) = tantivy {
            writer.finish()?;
        }
        catalog.finish(manifest, failures)?;
        Ok(skipped)
    }
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
            vec![root.to_path_buf()],
            options,
            None,
            &mut |_| {},
        );
    }

    build_file_search_index_full_rebuild_blocking(root, index_dir, options, None)
}

fn build_root_full_rebuild_with_progress(
    root: &Path,
    index_dir: &Path,
    options: FileSearchIndexOptions,
    cancel: Option<CancellationToken>,
    progress: &mut impl FnMut(FileSearchIndexProgress),
) -> Result<FileSearchIndexOutcome, IndexError> {
    let outcome = build_file_search_index_full_rebuild_blocking(root, index_dir, options, cancel)?;
    progress(FileSearchIndexProgress::IndexedPaths {
        completed_paths: 1,
        total_paths: 1,
        indexed_count: outcome.indexed_count,
    });
    Ok(outcome)
}

fn build_file_search_index_full_rebuild_blocking(
    root: &Path,
    index_dir: &Path,
    options: FileSearchIndexOptions,
    cancel: Option<CancellationToken>,
) -> Result<FileSearchIndexOutcome, IndexError> {
    let pending_index_dir = index_dir.with_extension("building");
    prepare_catalog_dir(root, &pending_index_dir)?;
    let mut pending_index = PendingIndexWriter::create(
        &pending_index_dir,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
    )?;
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
        |record| pending_index.add_record(&record),
    )?;
    pending_index.extend_skipped(crawl_skipped);
    let indexed_count = pending_index.record_count();
    let failures = warnings_to_failures(pending_index.skipped());
    finish_pending_index_build(
        root,
        index_dir,
        &options,
        None,
        indexed_count,
        pending_index,
        failures,
        None,
    )
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
            selected_paths,
            options,
            cancel,
            progress,
        );
    }

    if selected_paths.iter().any(|path| path == root) {
        return build_root_full_rebuild_with_progress(root, index_dir, options, cancel, progress);
    }

    build_selected_paths_full_rebuild_blocking(
        root,
        index_dir,
        selected_paths,
        options,
        cancel,
        progress,
    )
}

fn build_selected_paths_full_rebuild_blocking(
    root: &Path,
    index_dir: &Path,
    selected_paths: Vec<PathBuf>,
    options: FileSearchIndexOptions,
    cancel: Option<CancellationToken>,
    progress: &mut impl FnMut(FileSearchIndexProgress),
) -> Result<FileSearchIndexOutcome, IndexError> {
    let pending_index_dir = index_dir.with_extension("building");
    prepare_catalog_dir(root, &pending_index_dir)?;
    let total_paths = selected_paths.len().max(1);
    let mut pending_index = PendingIndexWriter::create(
        &pending_index_dir,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
    )?;
    let crawl_skipped = crawl_selected_search_records_with_callback_and_progress(
        root,
        &selected_paths,
        &SearchCrawlOptions {
            include_hidden: options.include_hidden,
            exclude_patterns: options.exclude_patterns.clone(),
            directory_error_policy: options.directory_error_policy,
            excluded_index_dir: Some(crawl_excluded_index_dir(&options, index_dir)),
            throttle: true,
            cancel,
        },
        |completed_paths, indexed_count| {
            progress(FileSearchIndexProgress::IndexedPaths {
                completed_paths,
                total_paths,
                indexed_count,
            });
        },
        |record| pending_index.add_record(&record),
    )?;
    pending_index.extend_skipped(crawl_skipped);
    let indexed_count = pending_index.record_count();
    let failures = warnings_to_failures(pending_index.skipped());
    finish_pending_index_build(
        root,
        index_dir,
        &options,
        None,
        indexed_count,
        pending_index,
        failures,
        None,
    )
}

fn build_file_search_index_incremental_blocking(
    root: &Path,
    index_dir: &Path,
    selected_paths: Vec<PathBuf>,
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
    let selected_roots = non_nested_index_roots(&selected_paths);

    if selected_roots.is_empty() {
        return previous.map_or_else(
            || {
                build_selected_paths_full_rebuild_blocking(
                    root,
                    index_dir,
                    selected_paths,
                    options,
                    cancel,
                    progress,
                )
            },
            |previous| Ok(incremental_noop_outcome(root, index_dir, previous)),
        );
    }

    let Some(previous) = previous else {
        return build_root_full_rebuild_with_progress(root, index_dir, options, cancel, progress);
    };

    if should_promote_incremental_selected_roots_to_full_rebuild(root, &selected_roots) {
        return build_root_full_rebuild_with_progress(root, index_dir, options, cancel, progress);
    }

    build_incremental_subtree_rebuild_blocking(
        root,
        index_dir,
        &selected_roots,
        options,
        previous,
        cancel,
        progress,
    )
}

fn build_incremental_subtree_rebuild_blocking(
    root: &Path,
    index_dir: &Path,
    selected_roots: &[PathBuf],
    options: FileSearchIndexOptions,
    previous: PreviousSearchIndex,
    cancel: Option<CancellationToken>,
    progress: &mut impl FnMut(FileSearchIndexProgress),
) -> Result<FileSearchIndexOutcome, IndexError> {
    let pending_index_dir = index_dir.with_extension("building");
    prepare_catalog_dir(root, &pending_index_dir)?;
    let mut pending_index = PendingIndexWriter::create(
        &pending_index_dir,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
    )?;
    let scan_cancel = cancel.clone().unwrap_or_else(CancellationToken::new);
    let mut previous_selected_metadata = HashMap::new();

    store::scan_catalog_records_with_cancel(
        index_dir,
        root,
        options.include_hidden,
        &options.exclude_patterns,
        options.directory_error_policy,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
        &scan_cancel,
        |record| {
            if selected_roots_cover_path(selected_roots, &record.path) {
                previous_selected_metadata
                    .insert(record.storage_key.clone(), record_fingerprint(&record));
            } else {
                pending_index.add_record(&record)?;
            }
            Ok(())
        },
    )?;

    let total_paths = selected_roots.len().max(1);
    let mut changed_count = 0usize;
    let crawl_skipped = crawl_selected_search_records_with_callback_and_progress(
        root,
        selected_roots,
        &SearchCrawlOptions {
            include_hidden: options.include_hidden,
            exclude_patterns: options.exclude_patterns.clone(),
            directory_error_policy: options.directory_error_policy,
            excluded_index_dir: Some(crawl_excluded_index_dir(&options, index_dir)),
            throttle: true,
            cancel,
        },
        |completed_paths, indexed_count| {
            progress(FileSearchIndexProgress::IndexedPaths {
                completed_paths,
                total_paths,
                indexed_count,
            });
        },
        |record| {
            if previous_selected_metadata
                .remove(record.storage_key.as_str())
                .is_none_or(|previous| previous != record_fingerprint(&record))
            {
                changed_count += 1;
            }
            pending_index.add_record(&record)
        },
    )?;

    changed_count += previous_selected_metadata.len();
    pending_index.extend_skipped(crawl_skipped);
    let selected_skipped = warnings_within_selected_roots(pending_index.skipped(), selected_roots);
    let failures = merge_scan_failures(&selected_skipped, &previous.failures, Some(selected_roots));
    finish_pending_index_build(
        root,
        index_dir,
        &options,
        Some(previous.manifest.built_at_ms),
        changed_count,
        pending_index,
        failures,
        Some(selected_skipped),
    )
}

fn previous_search_index(
    index_dir: &Path,
    root: &Path,
    include_hidden: bool,
    exclude_patterns: &[String],
    directory_error_policy: DirectoryErrorPolicy,
    content_index_enabled: bool,
    content_max_file_bytes: u64,
    media_metadata_scope: MediaMetadataScope,
) -> Option<PreviousSearchIndex> {
    let manifest = store::read_manifest(index_dir).ok()?;
    manifest
        .validate_for(
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
    Some(PreviousSearchIndex { manifest, failures })
}

fn finish_pending_index_build(
    root: &Path,
    index_dir: &Path,
    options: &FileSearchIndexOptions,
    built_at_ms: Option<i64>,
    indexed_count: usize,
    pending_index: PendingIndexWriter,
    failures: Vec<FileSearchIndexFailure>,
    reported_skipped: Option<Vec<ScanWarning>>,
) -> Result<FileSearchIndexOutcome, IndexError> {
    let failed_count = failures.len();
    let mut manifest = SearchIndexManifest::new(
        root,
        options.include_hidden,
        &options.exclude_patterns,
        options.directory_error_policy,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
        pending_index.record_count(),
        failed_count,
        built_at_ms,
    );
    let all_skipped = pending_index.finish(&mut manifest, &failures)?;
    let skipped = reported_skipped.unwrap_or(all_skipped);
    replace_catalog_dir(index_dir, &index_dir.with_extension("building"))?;
    manifest.index_size_bytes = catalog_dir_size(index_dir)?;
    clear_query_cache();

    Ok(FileSearchIndexOutcome {
        root: root.to_path_buf(),
        index_dir: index_dir.to_path_buf(),
        indexed_count,
        index_size_bytes: manifest.index_size_bytes,
        updated_at_ms: manifest.updated_at_ms,
        failed_count,
        skipped,
    })
}

fn incremental_noop_outcome(
    root: &Path,
    index_dir: &Path,
    previous: PreviousSearchIndex,
) -> FileSearchIndexOutcome {
    FileSearchIndexOutcome {
        root: root.to_path_buf(),
        index_dir: index_dir.to_path_buf(),
        indexed_count: 0,
        index_size_bytes: catalog_dir_size(index_dir).unwrap_or(previous.manifest.index_size_bytes),
        updated_at_ms: previous.manifest.updated_at_ms,
        failed_count: previous.failures.len(),
        skipped: Vec::new(),
    }
}

fn should_promote_incremental_selected_roots_to_full_rebuild(
    root: &Path,
    selected_roots: &[PathBuf],
) -> bool {
    selected_roots.iter().any(|path| path == root)
        || selected_roots.len() >= FULL_REBUILD_SELECTED_ROOT_THRESHOLD
}

fn record_fingerprint(record: &SearchCatalogRecord) -> RecordFingerprint {
    RecordFingerprint {
        mtime_ms: record.mtime_ms,
        size_bytes: record.size_bytes,
    }
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

fn warnings_within_selected_roots(
    warnings: &[ScanWarning],
    selected_roots: &[PathBuf],
) -> Vec<ScanWarning> {
    warnings
        .iter()
        .filter(|warning| selected_roots_cover_path(selected_roots, &warning.path))
        .cloned()
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_selected_roots_promote_full_rebuild_for_root() {
        let root = PathBuf::from("/tmp/root");

        assert!(should_promote_incremental_selected_roots_to_full_rebuild(
            &root,
            std::slice::from_ref(&root),
        ));
    }

    #[test]
    fn incremental_selected_roots_promote_full_rebuild_for_wide_batch() {
        let root = PathBuf::from("/tmp/root");
        let selected_roots = (0..FULL_REBUILD_SELECTED_ROOT_THRESHOLD)
            .map(|index| root.join(format!("dir-{index}")))
            .collect::<Vec<_>>();

        assert!(should_promote_incremental_selected_roots_to_full_rebuild(
            &root,
            &selected_roots,
        ));
    }

    #[test]
    fn incremental_selected_roots_keep_subtree_rebuild_for_small_batch() {
        let root = PathBuf::from("/tmp/root");
        let selected_roots = vec![root.join("project/src")];

        assert!(!should_promote_incremental_selected_roots_to_full_rebuild(
            &root,
            &selected_roots,
        ));
    }
}
