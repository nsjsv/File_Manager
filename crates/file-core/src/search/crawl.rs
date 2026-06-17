use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs as std_fs, thread};

use ignore::{gitignore::Gitignore, gitignore::GitignoreBuilder, WalkBuilder};
use tokio_util::sync::CancellationToken;

use super::catalog::SearchCatalogRecord;
use crate::scan::is_hidden_name;
use crate::{FileError, FileKind, ScanWarning};

const INDEX_THROTTLE_EVERY: usize = 128;
const INDEX_THROTTLE_SLEEP: Duration = Duration::from_millis(2);

pub(crate) struct SearchCrawlOptions {
    pub(crate) include_hidden: bool,
    pub(crate) exclude_patterns: Vec<String>,
    pub(crate) excluded_index_dir: Option<PathBuf>,
    pub(crate) throttle: bool,
    pub(crate) cancel: Option<CancellationToken>,
}

impl SearchCrawlOptions {
    fn ensure_not_cancelled(&self) -> Result<(), FileError> {
        if self
            .cancel
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            Err(FileError::Cancelled)
        } else {
            Ok(())
        }
    }
}

pub(crate) fn crawl_search_records(
    root: &Path,
    options: &SearchCrawlOptions,
) -> Result<(Vec<SearchCatalogRecord>, Vec<ScanWarning>), FileError> {
    std_fs::read_dir(root).map_err(|source| FileError::ReadDirectory {
        path: root.to_path_buf(),
        source,
    })?;

    let mut skipped = Vec::new();
    let mut records = Vec::new();

    for result in search_walk_builder(root, root, options).build() {
        options.ensure_not_cancelled()?;
        let dir_entry = match result {
            Ok(dir_entry) => dir_entry,
            Err(error) => {
                skipped.push(ignore_error_warning(root, error));
                continue;
            }
        };
        if dir_entry.depth() == 0 {
            continue;
        }

        let path = dir_entry.into_path();
        let metadata = match std_fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(source) => {
                skipped.push(ScanWarning {
                    path,
                    message: source.to_string(),
                });
                continue;
            }
        };

        records.push(SearchCatalogRecord::from_path(
            root,
            path,
            file_kind_from_metadata(&metadata),
        ));

        if options.throttle && records.len() % INDEX_THROTTLE_EVERY == 0 {
            thread::sleep(INDEX_THROTTLE_SLEEP);
        }
    }

    Ok((records, skipped))
}

pub(crate) fn crawl_selected_search_records_with_progress(
    catalog_root: &Path,
    selected_paths: &[PathBuf],
    options: &SearchCrawlOptions,
    mut progress: impl FnMut(usize, usize),
) -> Result<(Vec<SearchCatalogRecord>, Vec<ScanWarning>), FileError> {
    std_fs::read_dir(catalog_root).map_err(|source| FileError::ReadDirectory {
        path: catalog_root.to_path_buf(),
        source,
    })?;

    let mut skipped = Vec::new();
    let mut records = Vec::new();
    let mut seen_keys = HashSet::new();
    let selected_paths = non_nested_selected_paths(selected_paths);
    let custom_excludes = custom_exclude_matcher(catalog_root, &options.exclude_patterns);

    for (index, selected_path) in selected_paths.into_iter().enumerate() {
        options.ensure_not_cancelled()?;
        if selected_path_is_hidden(&selected_path) && !options.include_hidden {
            progress(index + 1, records.len());
            continue;
        }

        let metadata = match std_fs::symlink_metadata(&selected_path) {
            Ok(metadata) => metadata,
            Err(source) => {
                skipped.push(ScanWarning {
                    path: selected_path,
                    message: source.to_string(),
                });
                progress(index + 1, records.len());
                continue;
            }
        };

        let kind = file_kind_from_metadata(&metadata);
        if path_matches_custom_excludes(
            custom_excludes.as_ref(),
            catalog_root,
            &selected_path,
            kind == FileKind::Directory,
        ) {
            progress(index + 1, records.len());
            continue;
        }
        if selected_path != catalog_root {
            push_unique_record(
                SearchCatalogRecord::from_path(catalog_root, selected_path.clone(), kind),
                &mut records,
                &mut seen_keys,
            );
        }

        if kind == FileKind::Directory {
            crawl_selected_directory_children(
                catalog_root,
                &selected_path,
                options,
                &mut records,
                &mut skipped,
                &mut seen_keys,
            )?;
        }
        progress(index + 1, records.len());
    }

    Ok((records, skipped))
}

fn crawl_selected_directory_children(
    catalog_root: &Path,
    selected_path: &Path,
    options: &SearchCrawlOptions,
    records: &mut Vec<SearchCatalogRecord>,
    skipped: &mut Vec<ScanWarning>,
    seen_keys: &mut HashSet<String>,
) -> Result<(), FileError> {
    for result in search_walk_builder(selected_path, catalog_root, options).build() {
        options.ensure_not_cancelled()?;
        let dir_entry = match result {
            Ok(dir_entry) => dir_entry,
            Err(error) => {
                skipped.push(ignore_error_warning(selected_path, error));
                continue;
            }
        };
        if dir_entry.depth() == 0 {
            continue;
        }

        let path = dir_entry.into_path();
        let metadata = match std_fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(source) => {
                skipped.push(ScanWarning {
                    path,
                    message: source.to_string(),
                });
                continue;
            }
        };
        push_unique_record(
            SearchCatalogRecord::from_path(catalog_root, path, file_kind_from_metadata(&metadata)),
            records,
            seen_keys,
        );

        if options.throttle && records.len() % INDEX_THROTTLE_EVERY == 0 {
            thread::sleep(INDEX_THROTTLE_SLEEP);
        }
    }
    Ok(())
}

fn search_walk_builder(
    walk_root: &Path,
    catalog_root: &Path,
    options: &SearchCrawlOptions,
) -> WalkBuilder {
    let mut builder = WalkBuilder::new(walk_root);
    let excluded_index_dir = options.excluded_index_dir.clone();
    let excluded_pending_index_dir = excluded_index_dir
        .as_ref()
        .map(|index_dir| index_dir.with_extension("building"));
    let custom_excludes = custom_exclude_matcher(catalog_root, &options.exclude_patterns);
    let catalog_root = catalog_root.to_path_buf();

    builder
        .hidden(!options.include_hidden)
        .parents(true)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            let path = entry.path();
            let is_dir = entry
                .file_type()
                .is_some_and(|file_type| file_type.is_dir());
            !excluded_index_dir
                .as_ref()
                .is_some_and(|index_dir| path.starts_with(index_dir))
                && !excluded_pending_index_dir
                    .as_ref()
                    .is_some_and(|index_dir| path.starts_with(index_dir))
                && !path_matches_custom_excludes(
                    custom_excludes.as_ref(),
                    &catalog_root,
                    path,
                    is_dir,
                )
        });
    builder
}

fn custom_exclude_matcher(root: &Path, patterns: &[String]) -> Option<Gitignore> {
    let mut builder = GitignoreBuilder::new(root);
    let mut added = false;
    for pattern in patterns
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
    {
        if builder.add_line(None, pattern).is_ok() {
            added = true;
        }
    }
    added.then(|| builder.build().ok()).flatten()
}

fn path_matches_custom_excludes(
    matcher: Option<&Gitignore>,
    root: &Path,
    path: &Path,
    is_dir: bool,
) -> bool {
    let Some(matcher) = matcher else {
        return false;
    };
    if !path.starts_with(root) {
        return false;
    }
    matcher.matched(path, is_dir).is_ignore()
}

fn file_kind_from_metadata(metadata: &std_fs::Metadata) -> FileKind {
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_file() {
        FileKind::File
    } else if file_type.is_symlink() {
        FileKind::Symlink
    } else {
        FileKind::Other
    }
}

fn push_unique_record(
    record: SearchCatalogRecord,
    records: &mut Vec<SearchCatalogRecord>,
    seen_keys: &mut HashSet<String>,
) {
    if seen_keys.insert(record.storage_key.clone()) {
        records.push(record);
    }
}

fn selected_path_is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(is_hidden_name)
}

fn non_nested_selected_paths(selected_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = selected_paths.to_vec();
    paths.sort_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });

    let mut reduced = Vec::new();
    for path in paths {
        if reduced
            .iter()
            .any(|parent: &PathBuf| path.starts_with(parent))
        {
            continue;
        }
        reduced.push(path);
    }
    reduced
}

fn ignore_error_warning(root: &Path, error: ignore::Error) -> ScanWarning {
    let path = ignore_error_path(&error)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());
    ScanWarning {
        path,
        message: error.to_string(),
    }
}

fn ignore_error_path(error: &ignore::Error) -> Option<&Path> {
    match error {
        ignore::Error::Partial(errors) => errors.iter().find_map(ignore_error_path),
        ignore::Error::WithLineNumber { err, .. } | ignore::Error::WithDepth { err, .. } => {
            ignore_error_path(err)
        }
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::Loop { child, .. } => Some(child),
        _ => None,
    }
}
