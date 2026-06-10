mod cache;
mod catalog;
mod crawl;
mod path_encoding;
mod query;
mod store;
mod types;

use std::path::{Path, PathBuf};

use cache::{cache_built_catalog, catalog_for_index};
use catalog::SearchCatalog;
use crawl::{
    crawl_search_records, crawl_selected_search_records_with_progress, SearchCrawlOptions,
};
use store::{prepare_catalog_dir, replace_catalog_dir, write_catalog, SearchIndexManifest};
use tokio_util::sync::CancellationToken;

use crate::FileError;

pub use types::{
    FileSearchIndexOptions, FileSearchIndexOutcome, FileSearchIndexProgress, FileSearchMatch,
    FileSearchOptions, FileSearchOutcome,
};

pub async fn search_file_tree(
    root: impl AsRef<Path>,
    query: impl AsRef<str>,
    options: FileSearchOptions,
) -> Result<FileSearchOutcome, FileError> {
    let root = root.as_ref().to_path_buf();
    let query = query.as_ref().trim().to_owned();
    if query.is_empty() {
        return Ok(empty_search_outcome(root));
    }

    let join_root = root.clone();
    let search_root = root.clone();
    let include_hidden = options.include_hidden;
    let (catalog, skipped) = tokio::task::spawn_blocking(move || {
        let (records, skipped) = crawl_search_records(
            &search_root,
            &SearchCrawlOptions {
                include_hidden,
                excluded_index_dir: None,
                throttle: false,
                cancel: None,
            },
        )?;
        let catalog = SearchCatalog::from_records(search_root, records, None);
        Ok::<_, FileError>((catalog, skipped))
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
) -> Result<FileSearchIndexOutcome, FileError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        build_file_search_index_blocking(&root, &index_dir, options)
    })
    .await
    .map_err(|error| search_index_error(&join_root, error))?
}

pub async fn build_file_search_index_for_paths(
    root: impl AsRef<Path>,
    index_dir: impl AsRef<Path>,
    selected_paths: Vec<PathBuf>,
    options: FileSearchIndexOptions,
) -> Result<FileSearchIndexOutcome, FileError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        build_file_search_index_for_paths_blocking(&root, &index_dir, selected_paths, options)
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
) -> Result<FileSearchIndexOutcome, FileError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        build_file_search_index_for_paths_blocking_with_progress(
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
) -> Result<FileSearchOutcome, FileError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let query = query.as_ref().trim().to_owned();
    if query.is_empty() {
        return Ok(empty_search_outcome(root));
    }
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        let catalog = catalog_for_index(&index_dir, &root, options.include_hidden)?;
        let matches = query::search_catalog(&catalog, &query, options.limit.max(1));
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

fn build_file_search_index_blocking(
    root: &Path,
    index_dir: &Path,
    options: FileSearchIndexOptions,
) -> Result<FileSearchIndexOutcome, FileError> {
    let pending_index_dir = index_dir.with_extension("building");
    prepare_catalog_dir(root, &pending_index_dir)?;
    let (records, skipped) = crawl_search_records(
        root,
        &SearchCrawlOptions {
            include_hidden: options.include_hidden,
            excluded_index_dir: Some(index_dir.to_path_buf()),
            throttle: true,
            cancel: None,
        },
    )?;
    let manifest = SearchIndexManifest::new(root, options.include_hidden, records.len());
    write_catalog(&pending_index_dir, &manifest, &records)?;
    replace_catalog_dir(index_dir, &pending_index_dir)?;

    let indexed_count = records.len();
    let catalog = SearchCatalog::from_records(root.to_path_buf(), records, Some(&manifest));
    cache_built_catalog(index_dir, root, options.include_hidden, &manifest, catalog);

    Ok(FileSearchIndexOutcome {
        root: root.to_path_buf(),
        index_dir: index_dir.to_path_buf(),
        indexed_count,
        skipped,
    })
}

fn build_file_search_index_for_paths_blocking(
    root: &Path,
    index_dir: &Path,
    selected_paths: Vec<PathBuf>,
    options: FileSearchIndexOptions,
) -> Result<FileSearchIndexOutcome, FileError> {
    build_file_search_index_for_paths_blocking_with_progress(
        root,
        index_dir,
        selected_paths,
        options,
        None,
        &mut |_| {},
    )
}

fn build_file_search_index_for_paths_blocking_with_progress(
    root: &Path,
    index_dir: &Path,
    selected_paths: Vec<PathBuf>,
    options: FileSearchIndexOptions,
    cancel: Option<CancellationToken>,
    progress: &mut impl FnMut(FileSearchIndexProgress),
) -> Result<FileSearchIndexOutcome, FileError> {
    let pending_index_dir = index_dir.with_extension("building");
    prepare_catalog_dir(root, &pending_index_dir)?;
    let total_paths = selected_paths.len().max(1);
    let (records, skipped) = crawl_selected_search_records_with_progress(
        root,
        &selected_paths,
        &SearchCrawlOptions {
            include_hidden: options.include_hidden,
            excluded_index_dir: Some(index_dir.to_path_buf()),
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
    )?;
    let manifest = SearchIndexManifest::new(root, options.include_hidden, records.len());
    write_catalog(&pending_index_dir, &manifest, &records)?;
    replace_catalog_dir(index_dir, &pending_index_dir)?;

    let indexed_count = records.len();
    let catalog = SearchCatalog::from_records(root.to_path_buf(), records, Some(&manifest));
    cache_built_catalog(index_dir, root, options.include_hidden, &manifest, catalog);

    Ok(FileSearchIndexOutcome {
        root: root.to_path_buf(),
        index_dir: index_dir.to_path_buf(),
        indexed_count,
        skipped,
    })
}

fn empty_search_outcome(root: PathBuf) -> FileSearchOutcome {
    FileSearchOutcome {
        root,
        matches: Vec::new(),
        skipped: Vec::new(),
    }
}

pub(crate) fn search_index_error(path: &Path, error: impl ToString) -> FileError {
    FileError::SearchIndex {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}
