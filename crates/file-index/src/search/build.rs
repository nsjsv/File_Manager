use std::path::{Path, PathBuf};

use file_core::ScanWarning;
use tokio_util::sync::CancellationToken;

use super::cache::clear_query_cache;
use super::catalog::SearchCatalogRecord;
use super::crawl::{
    crawl_search_records_with_callback, crawl_selected_search_records_with_callback_and_progress,
    SearchCrawlOptions,
};
use super::extractor::extract_media_document;
use super::full_text::TantivyIndexWriter;
use super::manifest::{current_time_ms, SearchIndexManifest};
use super::store::{
    catalog_dir_size, prepare_catalog_dir, replace_catalog_dir, CatalogWriteSession,
};
use super::types::{
    FileSearchIndexFailure, FileSearchIndexOptions, FileSearchIndexOutcome, FileSearchIndexProgress,
};
use crate::profile::MediaMetadataScope;
use crate::IndexError;

struct PendingIndexWriter {
    catalog: CatalogWriteSession,
    tantivy: Option<TantivyIndexWriter>,
    skipped: Vec<ScanWarning>,
    media_metadata_scope: MediaMetadataScope,
}

impl PendingIndexWriter {
    fn create(
        index_dir: &Path,
        media_metadata_scope: MediaMetadataScope,
    ) -> Result<Self, IndexError> {
        Ok(Self {
            catalog: CatalogWriteSession::create(index_dir)?,
            tantivy: media_metadata_scope
                .includes_media()
                .then(|| TantivyIndexWriter::create(index_dir))
                .transpose()?,
            skipped: Vec::new(),
            media_metadata_scope,
        })
    }

    fn add_record(&mut self, record: &SearchCatalogRecord) -> Result<(), IndexError> {
        if let Some(writer) = self.tantivy.as_mut() {
            match extract_media_document(record, self.media_metadata_scope) {
                Ok(Some(document)) => writer.add_media_document(&document)?,
                Ok(None) => {}
                Err(warning) => self.skipped.push(warning),
            }
        }
        self.catalog.add_record(record)
    }

    fn extend_skipped(&mut self, skipped: Vec<ScanWarning>) {
        self.skipped.extend(skipped);
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
    let mut pending_index =
        PendingIndexWriter::create(&pending_index_dir, options.media_metadata_scope)?;
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
    finish_pending_index_build(root, index_dir, &options, pending_index)
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
    let mut pending_index =
        PendingIndexWriter::create(&pending_index_dir, options.media_metadata_scope)?;
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
    finish_pending_index_build(root, index_dir, &options, pending_index)
}

fn finish_pending_index_build(
    root: &Path,
    index_dir: &Path,
    options: &FileSearchIndexOptions,
    pending_index: PendingIndexWriter,
) -> Result<FileSearchIndexOutcome, IndexError> {
    let indexed_count = pending_index.record_count();
    let failures = warnings_to_failures(&pending_index.skipped);
    let failed_count = failures.len();
    let mut manifest = SearchIndexManifest::new(
        root,
        options.include_hidden,
        &options.exclude_patterns,
        options.directory_error_policy,
        options.media_metadata_scope,
        indexed_count,
        failed_count,
        None,
    );
    let skipped = pending_index.finish(&mut manifest, &failures)?;
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

fn warnings_to_failures(warnings: &[ScanWarning]) -> Vec<FileSearchIndexFailure> {
    let now = current_time_ms();
    let mut failures = warnings
        .iter()
        .map(|warning| FileSearchIndexFailure {
            path: warning.path.clone(),
            message: warning.message.clone(),
            first_failed_at_ms: now,
            last_failed_at_ms: now,
            retry_count: 1,
        })
        .collect::<Vec<_>>();
    failures.sort_by(|left, right| left.path.cmp(&right.path));
    failures.dedup_by(|left, right| left.path == right.path);
    failures
}
