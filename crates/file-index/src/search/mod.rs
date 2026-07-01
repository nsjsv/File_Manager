mod build;
mod cache;
mod catalog;
mod crawl;
mod engine;
mod extractor;
mod full_text;
mod ignore_policy;
mod manifest;
pub(crate) mod path_encoding;
mod query;
mod store;
mod types;

use std::path::{Path, PathBuf};

use cache::query_runtime_for_index;
use catalog::{SearchCatalog, SearchCatalogRecord};
use crawl::crawl_search_records;
use engine::search_index_catalog_and_tantivy;
use store::{clear_failures, read_index_status, remove_catalog_dir};
use tokio_util::sync::CancellationToken;

use crate::IndexError;

pub(crate) use crawl::{watchable_search_directories, SearchCrawlOptions};
pub use ignore_policy::default_search_index_exclude_patterns;
pub(crate) use types::EXTRACTOR_VERSION;
pub use types::{
    DirectoryErrorPolicy, FileSearchIndexFailure, FileSearchIndexMode, FileSearchIndexOptions,
    FileSearchIndexOutcome, FileSearchIndexProgress, FileSearchIndexStatus, FileSearchMatch,
    FileSearchOptions, FileSearchOutcome, MediaExifField, MediaSearchKind, MediaSearchMetadata,
    SearchIndexFileRecord, SearchResultSource,
};

pub async fn search_file_tree(
    root: impl AsRef<Path>,
    query: impl AsRef<str>,
    options: FileSearchOptions,
) -> Result<FileSearchOutcome, IndexError> {
    search_file_tree_with_cancel(root, query, options, CancellationToken::new()).await
}

pub async fn search_file_tree_with_cancel(
    root: impl AsRef<Path>,
    query: impl AsRef<str>,
    options: FileSearchOptions,
    cancel: CancellationToken,
) -> Result<FileSearchOutcome, IndexError> {
    let root = root.as_ref().to_path_buf();
    let query = query.as_ref().trim().to_owned();
    if query.is_empty() {
        return Ok(empty_search_outcome(root));
    }

    let join_root = root.clone();
    let search_root = root.clone();
    let include_hidden = options.include_hidden;
    let exclude_patterns = options.exclude_patterns.clone();
    let (catalog, skipped) = tokio::task::spawn_blocking(move || {
        let (records, skipped) = crawl_search_records(
            &search_root,
            &SearchCrawlOptions {
                include_hidden,
                exclude_patterns,
                directory_error_policy: options.directory_error_policy,
                excluded_index_dir: None,
                throttle: false,
                cancel: Some(cancel),
            },
        )?;
        let catalog = SearchCatalog::from_records(search_root, records, None);
        Ok::<_, IndexError>((catalog, skipped))
    })
    .await
    .map_err(|error| search_index_error(&join_root, error))??;
    let matches = query::search_catalog(&catalog, &query, options.limit.max(1));

    Ok(FileSearchOutcome {
        root,
        matches,
        skipped,
    })
}

pub async fn build_file_search_index(
    root: impl AsRef<Path>,
    index_dir: impl AsRef<Path>,
    options: FileSearchIndexOptions,
) -> Result<FileSearchIndexOutcome, IndexError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        build::build_file_search_index_blocking(&root, &index_dir, options)
    })
    .await
    .map_err(|error| search_index_error(&join_root, error))?
}

pub async fn build_file_search_index_for_paths(
    root: impl AsRef<Path>,
    index_dir: impl AsRef<Path>,
    selected_paths: Vec<PathBuf>,
    options: FileSearchIndexOptions,
) -> Result<FileSearchIndexOutcome, IndexError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        build::build_file_search_index_for_paths_blocking(
            &root,
            &index_dir,
            selected_paths,
            options,
        )
    })
    .await
    .map_err(|error| search_index_error(&join_root, error))?
}

pub async fn build_file_search_index_for_paths_with_progress(
    root: impl AsRef<Path>,
    index_dir: impl AsRef<Path>,
    selected_paths: Vec<PathBuf>,
    options: FileSearchIndexOptions,
    cancel: CancellationToken,
    mut progress: impl FnMut(FileSearchIndexProgress) + Send + 'static,
) -> Result<FileSearchIndexOutcome, IndexError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        build::build_file_search_index_for_paths_blocking_with_progress(
            &root,
            &index_dir,
            selected_paths,
            options,
            Some(cancel),
            &mut progress,
        )
    })
    .await
    .map_err(|error| search_index_error(&join_root, error))?
}

pub async fn search_file_index(
    index_dir: impl AsRef<Path>,
    root: impl AsRef<Path>,
    query: impl AsRef<str>,
    options: FileSearchOptions,
) -> Result<FileSearchOutcome, IndexError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let query = query.as_ref().trim().to_owned();
    if query.is_empty() {
        return Ok(empty_search_outcome(root));
    }
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        let runtime = query_runtime_for_index(
            &index_dir,
            &root,
            options.include_hidden,
            &options.exclude_patterns,
            options.directory_error_policy,
            options.content_index_enabled,
            options.content_max_file_bytes,
            options.media_metadata_scope,
        )?;
        let matches = search_index_catalog_and_tantivy(&runtime, &query, &options)?;
        Ok(FileSearchOutcome {
            root,
            matches,
            skipped: Vec::new(),
        })
    })
    .await
    .map_err(|error| search_index_error(&join_root, error))?
}

pub fn file_search_index_exists(index_dir: impl AsRef<Path>) -> bool {
    store::read_manifest(index_dir.as_ref()).is_ok()
}

pub async fn file_search_index_status(
    index_dir: impl AsRef<Path>,
    root: impl AsRef<Path>,
    options: FileSearchIndexOptions,
) -> Result<FileSearchIndexStatus, IndexError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        read_index_status(
            &index_dir,
            &root,
            options.include_hidden,
            &options.exclude_patterns,
            options.directory_error_policy,
            options.content_index_enabled,
            options.content_max_file_bytes,
            options.media_metadata_scope,
        )
    })
    .await
    .map_err(|error| search_index_error(&join_root, error))?
}

pub async fn file_search_index_snapshot(
    index_dir: impl AsRef<Path>,
    root: impl AsRef<Path>,
    options: FileSearchIndexOptions,
) -> Result<(Vec<SearchIndexFileRecord>, Vec<FileSearchIndexFailure>), IndexError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        let (_, records) = store::load_catalog(
            &index_dir,
            &root,
            options.include_hidden,
            &options.exclude_patterns,
            options.directory_error_policy,
            options.content_index_enabled,
            options.content_max_file_bytes,
            options.media_metadata_scope,
        )?;
        let records = records
            .iter()
            .map(SearchCatalogRecord::to_file_record)
            .collect();
        let failures = store::read_failures(&index_dir)?;
        Ok((records, failures))
    })
    .await
    .map_err(|error| search_index_error(&join_root, error))?
}

pub async fn clear_file_search_index_failures(
    index_dir: impl AsRef<Path>,
) -> Result<(), IndexError> {
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_path = index_dir.clone();

    tokio::task::spawn_blocking(move || clear_failures(&index_dir))
        .await
        .map_err(|error| search_index_error(&join_path, error))?
}

pub async fn remove_file_search_index(index_dir: impl AsRef<Path>) -> Result<(), IndexError> {
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_path = index_dir.clone();

    tokio::task::spawn_blocking(move || remove_catalog_dir(&index_dir))
        .await
        .map_err(|error| search_index_error(&join_path, error))?
}

#[doc(hidden)]
pub fn clear_search_query_cache_for_tests() {
    cache::clear_query_cache();
}

fn empty_search_outcome(root: PathBuf) -> FileSearchOutcome {
    FileSearchOutcome {
        root,
        matches: Vec::new(),
        skipped: Vec::new(),
    }
}

pub(crate) fn search_index_error(path: &Path, error: impl ToString) -> IndexError {
    IndexError::store(path, error)
}
