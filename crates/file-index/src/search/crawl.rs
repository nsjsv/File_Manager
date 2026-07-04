use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs as std_fs, thread};

use ignore::{gitignore::Gitignore, gitignore::GitignoreBuilder, WalkBuilder};
use tokio_util::sync::CancellationToken;

use super::catalog::SearchCatalogRecord;
use super::types::DirectoryErrorPolicy;
use crate::IndexError;
use file_core::{FileKind, ScanWarning};

const INDEX_THROTTLE_EVERY: usize = 128;
const INDEX_THROTTLE_SLEEP: Duration = Duration::from_millis(2);

pub(crate) struct SearchCrawlOptions {
    pub(crate) include_hidden: bool,
    pub(crate) exclude_patterns: Vec<String>,
    pub(crate) directory_error_policy: DirectoryErrorPolicy,
    pub(crate) excluded_index_dir: Option<PathBuf>,
    pub(crate) throttle: bool,
    pub(crate) cancel: Option<CancellationToken>,
}

impl SearchCrawlOptions {
    fn ensure_not_cancelled(&self) -> Result<(), IndexError> {
        if self
            .cancel
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            Err(IndexError::Cancelled)
        } else {
            Ok(())
        }
    }
}

pub(crate) fn crawl_search_records(
    root: &Path,
    options: &SearchCrawlOptions,
) -> Result<(Vec<SearchCatalogRecord>, Vec<ScanWarning>), IndexError> {
    std_fs::read_dir(root).map_err(|source| IndexError::ReadDirectory {
        path: root.to_path_buf(),
        source,
    })?;

    let mut skipped = Vec::new();
    let mut records = Vec::new();
    let mut skipped_roots = Vec::new();

    for result in search_walk_builder(root, root, options).build() {
        options.ensure_not_cancelled()?;
        let dir_entry = match result {
            Ok(dir_entry) => dir_entry,
            Err(error) => {
                record_walk_error(
                    root,
                    error,
                    options,
                    &mut records,
                    &mut skipped,
                    &mut skipped_roots,
                )?;
                continue;
            }
        };
        if dir_entry.depth() == 0 {
            continue;
        }

        let path = dir_entry.into_path();
        if skipped_roots
            .iter()
            .any(|skipped| path.starts_with(skipped))
        {
            continue;
        }
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

        records.push(SearchCatalogRecord::from_path_with_metadata(
            root,
            path,
            file_kind_from_metadata(&metadata),
            &metadata,
        ));

        if options.throttle && records.len() % INDEX_THROTTLE_EVERY == 0 {
            thread::sleep(INDEX_THROTTLE_SLEEP);
        }
    }

    Ok((records, skipped))
}

pub(crate) fn crawl_search_records_with_callback(
    root: &Path,
    options: &SearchCrawlOptions,
    mut visit: impl FnMut(SearchCatalogRecord) -> Result<(), IndexError>,
) -> Result<Vec<ScanWarning>, IndexError> {
    std_fs::read_dir(root).map_err(|source| IndexError::ReadDirectory {
        path: root.to_path_buf(),
        source,
    })?;

    let mut skipped = Vec::new();
    let mut skipped_roots = Vec::new();
    let mut discarded_records = Vec::new();
    let mut indexed_count = 0usize;

    for result in search_walk_builder(root, root, options).build() {
        options.ensure_not_cancelled()?;
        let dir_entry = match result {
            Ok(dir_entry) => dir_entry,
            Err(error) => {
                record_walk_error(
                    root,
                    error,
                    options,
                    &mut discarded_records,
                    &mut skipped,
                    &mut skipped_roots,
                )?;
                continue;
            }
        };
        if dir_entry.depth() == 0 {
            continue;
        }

        let path = dir_entry.into_path();
        if skipped_roots
            .iter()
            .any(|skipped| path.starts_with(skipped))
        {
            continue;
        }
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

        let kind = file_kind_from_metadata(&metadata);
        if kind == FileKind::Directory {
            match std_fs::read_dir(&path) {
                Ok(_) => {}
                Err(source) => {
                    if options.directory_error_policy == DirectoryErrorPolicy::Abort {
                        return Err(IndexError::ReadDirectory { path, source });
                    }
                    if !skipped_roots
                        .iter()
                        .any(|skipped| path.starts_with(skipped))
                    {
                        skipped_roots.push(path.clone());
                    }
                    if !skipped.iter().any(|warning| warning.path == path) {
                        skipped.push(ScanWarning {
                            path,
                            message: source.to_string(),
                        });
                    }
                    continue;
                }
            }
        }

        visit(SearchCatalogRecord::from_path_with_metadata(
            root, path, kind, &metadata,
        ))?;
        indexed_count += 1;

        if options.throttle && indexed_count % INDEX_THROTTLE_EVERY == 0 {
            thread::sleep(INDEX_THROTTLE_SLEEP);
        }
    }

    Ok(skipped)
}

#[cfg(test)]
pub(crate) fn crawl_selected_search_records_with_progress(
    catalog_root: &Path,
    selected_paths: &[PathBuf],
    options: &SearchCrawlOptions,
    mut progress: impl FnMut(usize, usize),
) -> Result<(Vec<SearchCatalogRecord>, Vec<ScanWarning>), IndexError> {
    std_fs::read_dir(catalog_root).map_err(|source| IndexError::ReadDirectory {
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
        if selected_path != catalog_root
            && selected_path_is_hidden(&selected_path)
            && !options.include_hidden
        {
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
                SearchCatalogRecord::from_path_with_metadata(
                    catalog_root,
                    selected_path.clone(),
                    kind,
                    &metadata,
                ),
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

pub(crate) fn crawl_selected_search_records_with_callback_and_progress(
    catalog_root: &Path,
    selected_paths: &[PathBuf],
    options: &SearchCrawlOptions,
    mut progress: impl FnMut(usize, usize),
    mut visit: impl FnMut(SearchCatalogRecord) -> Result<(), IndexError>,
) -> Result<Vec<ScanWarning>, IndexError> {
    std_fs::read_dir(catalog_root).map_err(|source| IndexError::ReadDirectory {
        path: catalog_root.to_path_buf(),
        source,
    })?;

    let mut skipped = Vec::new();
    let mut seen_keys = HashSet::new();
    let mut indexed_count = 0usize;
    let selected_paths = non_nested_selected_paths(selected_paths);
    let custom_excludes = custom_exclude_matcher(catalog_root, &options.exclude_patterns);

    for (index, selected_path) in selected_paths.into_iter().enumerate() {
        options.ensure_not_cancelled()?;
        if selected_path != catalog_root
            && selected_path_is_hidden(&selected_path)
            && !options.include_hidden
        {
            progress(index + 1, indexed_count);
            continue;
        }

        let metadata = match std_fs::symlink_metadata(&selected_path) {
            Ok(metadata) => metadata,
            Err(source) => {
                skipped.push(ScanWarning {
                    path: selected_path,
                    message: source.to_string(),
                });
                progress(index + 1, indexed_count);
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
            progress(index + 1, indexed_count);
            continue;
        }
        if selected_path != catalog_root {
            visit_selected_record_if_new(
                SearchCatalogRecord::from_path_with_metadata(
                    catalog_root,
                    selected_path.clone(),
                    kind,
                    &metadata,
                ),
                &mut seen_keys,
                &mut indexed_count,
                &mut visit,
            )?;
        }

        if kind == FileKind::Directory {
            crawl_selected_directory_children_with_callback(
                catalog_root,
                &selected_path,
                options,
                &mut skipped,
                &mut seen_keys,
                &mut indexed_count,
                &mut visit,
            )?;
        }
        progress(index + 1, indexed_count);
    }

    Ok(skipped)
}

#[cfg(test)]
fn crawl_selected_directory_children(
    catalog_root: &Path,
    selected_path: &Path,
    options: &SearchCrawlOptions,
    records: &mut Vec<SearchCatalogRecord>,
    skipped: &mut Vec<ScanWarning>,
    seen_keys: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let mut skipped_roots = Vec::new();
    for result in search_walk_builder(selected_path, catalog_root, options).build() {
        options.ensure_not_cancelled()?;
        let dir_entry = match result {
            Ok(dir_entry) => dir_entry,
            Err(error) => {
                record_walk_error(
                    selected_path,
                    error,
                    options,
                    records,
                    skipped,
                    &mut skipped_roots,
                )?;
                continue;
            }
        };
        if dir_entry.depth() == 0 {
            continue;
        }

        let path = dir_entry.into_path();
        if skipped_roots
            .iter()
            .any(|skipped| path.starts_with(skipped))
        {
            continue;
        }
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
            SearchCatalogRecord::from_path_with_metadata(
                catalog_root,
                path,
                file_kind_from_metadata(&metadata),
                &metadata,
            ),
            records,
            seen_keys,
        );

        if options.throttle && records.len() % INDEX_THROTTLE_EVERY == 0 {
            thread::sleep(INDEX_THROTTLE_SLEEP);
        }
    }
    Ok(())
}

fn crawl_selected_directory_children_with_callback(
    catalog_root: &Path,
    selected_path: &Path,
    options: &SearchCrawlOptions,
    skipped: &mut Vec<ScanWarning>,
    seen_keys: &mut HashSet<String>,
    indexed_count: &mut usize,
    visit: &mut impl FnMut(SearchCatalogRecord) -> Result<(), IndexError>,
) -> Result<(), IndexError> {
    let mut skipped_roots = Vec::new();
    let mut discarded_records = Vec::new();
    for result in search_walk_builder(selected_path, catalog_root, options).build() {
        options.ensure_not_cancelled()?;
        let dir_entry = match result {
            Ok(dir_entry) => dir_entry,
            Err(error) => {
                record_walk_error(
                    selected_path,
                    error,
                    options,
                    &mut discarded_records,
                    skipped,
                    &mut skipped_roots,
                )?;
                continue;
            }
        };
        if dir_entry.depth() == 0 {
            continue;
        }

        let path = dir_entry.into_path();
        if skipped_roots
            .iter()
            .any(|skipped_root| path.starts_with(skipped_root))
        {
            continue;
        }
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
        visit_selected_record_if_new(
            SearchCatalogRecord::from_path_with_metadata(
                catalog_root,
                path,
                file_kind_from_metadata(&metadata),
                &metadata,
            ),
            seen_keys,
            indexed_count,
            visit,
        )?;

        if options.throttle && *indexed_count % INDEX_THROTTLE_EVERY == 0 {
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

pub(crate) fn watchable_search_directories(
    root: &Path,
    options: &SearchCrawlOptions,
) -> Result<(Vec<PathBuf>, Vec<ScanWarning>), IndexError> {
    std_fs::read_dir(root).map_err(|source| IndexError::ReadDirectory {
        path: root.to_path_buf(),
        source,
    })?;

    let mut directories = vec![root.to_path_buf()];
    let mut skipped = Vec::new();
    let mut skipped_roots = Vec::new();
    let mut discarded_records = Vec::new();

    for result in search_walk_builder(root, root, options).build() {
        options.ensure_not_cancelled()?;
        let dir_entry = match result {
            Ok(dir_entry) => dir_entry,
            Err(error) => {
                let skipped_root = record_walk_error(
                    root,
                    error,
                    options,
                    &mut discarded_records,
                    &mut skipped,
                    &mut skipped_roots,
                )?;
                directories.retain(|directory| !directory.starts_with(&skipped_root));
                continue;
            }
        };
        if dir_entry.depth() == 0 {
            continue;
        }

        let path = dir_entry.path();
        if skipped_roots
            .iter()
            .any(|skipped| path.starts_with(skipped))
        {
            continue;
        }
        if dir_entry
            .file_type()
            .is_some_and(|file_type| file_type.is_dir())
        {
            directories.push(path.to_path_buf());
        }
    }

    directories.sort_unstable();
    directories.dedup();
    Ok((directories, skipped))
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

#[cfg(test)]
fn push_unique_record(
    record: SearchCatalogRecord,
    records: &mut Vec<SearchCatalogRecord>,
    seen_keys: &mut HashSet<String>,
) {
    if seen_keys.insert(record.storage_key.clone()) {
        records.push(record);
    }
}

fn visit_selected_record_if_new(
    record: SearchCatalogRecord,
    seen_keys: &mut HashSet<String>,
    indexed_count: &mut usize,
    visit: &mut impl FnMut(SearchCatalogRecord) -> Result<(), IndexError>,
) -> Result<(), IndexError> {
    if seen_keys.insert(record.storage_key.clone()) {
        visit(record)?;
        *indexed_count += 1;
    }
    Ok(())
}

fn record_walk_error(
    fallback_root: &Path,
    error: ignore::Error,
    options: &SearchCrawlOptions,
    records: &mut Vec<SearchCatalogRecord>,
    skipped: &mut Vec<ScanWarning>,
    skipped_roots: &mut Vec<PathBuf>,
) -> Result<PathBuf, IndexError> {
    let error_path = ignore_error_path(&error)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| fallback_root.to_path_buf());
    if options.directory_error_policy == DirectoryErrorPolicy::Abort {
        return Err(IndexError::ReadDirectory {
            path: error_path,
            source: ignore_error_io(error),
        });
    }

    records.retain(|record| !record.path.starts_with(&error_path));
    if !skipped_roots
        .iter()
        .any(|skipped| error_path.starts_with(skipped))
    {
        skipped_roots.push(error_path.clone());
    }
    if !skipped.iter().any(|warning| warning.path == error_path) {
        skipped.push(ScanWarning {
            path: error_path.clone(),
            message: error.to_string(),
        });
    }
    Ok(error_path)
}

fn selected_path_is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(is_hidden_name)
}

fn is_hidden_name(name: &std::ffi::OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
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

fn ignore_error_io(error: ignore::Error) -> std::io::Error {
    error
        .into_io_error()
        .unwrap_or_else(|| std::io::Error::other("search index walk failed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn watchable_search_directories_respects_profile_boundaries() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let visible = root.join("src");
        let hidden = root.join(".cache");
        let excluded = root.join("node_modules/pkg");
        let index_dir = root.join(".file-index");
        let pending_index_dir = index_dir.with_extension("building");
        std_fs::create_dir_all(&visible).unwrap();
        std_fs::create_dir_all(&hidden).unwrap();
        std_fs::create_dir_all(&excluded).unwrap();
        std_fs::create_dir_all(&index_dir).unwrap();
        std_fs::create_dir_all(&pending_index_dir).unwrap();

        let (directories, skipped) = watchable_search_directories(
            root,
            &SearchCrawlOptions {
                include_hidden: false,
                exclude_patterns: vec!["node_modules/".to_owned()],
                directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
                excluded_index_dir: Some(index_dir.clone()),
                throttle: false,
                cancel: None,
            },
        )
        .unwrap();

        assert!(skipped.is_empty());
        assert!(directories.iter().any(|directory| directory == root));
        assert!(directories.contains(&visible));
        assert!(!directories.contains(&hidden));
        assert!(!directories
            .iter()
            .any(|directory| directory.starts_with(&excluded)));
        assert!(!directories
            .iter()
            .any(|directory| directory.starts_with(&index_dir)));
        assert!(!directories
            .iter()
            .any(|directory| directory.starts_with(&pending_index_dir)));
    }

    #[test]
    fn selected_hidden_catalog_root_is_scanned_without_including_hidden_children() {
        let dir = tempdir().unwrap();
        let hidden_root = dir.path().join(".config");
        let visible_file = hidden_root.join("settings.toml");
        let hidden_child = hidden_root.join(".secret");
        std_fs::create_dir_all(&hidden_root).unwrap();
        std_fs::write(&visible_file, b"visible").unwrap();
        std_fs::write(&hidden_child, b"hidden").unwrap();

        let (records, skipped) = crawl_selected_search_records_with_progress(
            &hidden_root,
            std::slice::from_ref(&hidden_root),
            &SearchCrawlOptions {
                include_hidden: false,
                exclude_patterns: Vec::new(),
                directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
                excluded_index_dir: None,
                throttle: false,
                cancel: None,
            },
            |_, _| {},
        )
        .unwrap();

        assert!(skipped.is_empty());
        assert!(records.iter().any(|record| record.path == visible_file));
        assert!(!records.iter().any(|record| record.path == hidden_child));
    }

    #[cfg(unix)]
    #[test]
    fn watchable_search_directories_records_unreadable_child_when_skipping() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let root = dir.path();
        let visible = root.join("visible");
        let blocked = root.join("blocked");
        std_fs::create_dir_all(&visible).unwrap();
        std_fs::create_dir_all(blocked.join("child")).unwrap();
        let original_permissions = std_fs::metadata(&blocked).unwrap().permissions();
        std_fs::set_permissions(&blocked, std_fs::Permissions::from_mode(0o000)).unwrap();
        if std_fs::read_dir(&blocked).is_ok() {
            std_fs::set_permissions(&blocked, original_permissions).unwrap();
            return;
        }

        let outcome = watchable_search_directories(
            root,
            &SearchCrawlOptions {
                include_hidden: false,
                exclude_patterns: Vec::new(),
                directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
                excluded_index_dir: None,
                throttle: false,
                cancel: None,
            },
        );
        std_fs::set_permissions(&blocked, original_permissions).unwrap();
        let (directories, skipped) = outcome.unwrap();

        assert!(directories.contains(&visible));
        assert!(!directories
            .iter()
            .any(|directory| directory.starts_with(&blocked)));
        assert!(skipped.iter().any(|warning| warning.path == blocked));
    }

    #[cfg(unix)]
    #[test]
    fn watchable_search_directories_fails_unreadable_child_when_abort_policy_is_used() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let root = dir.path();
        let blocked = root.join("blocked");
        std_fs::create_dir_all(blocked.join("child")).unwrap();
        let original_permissions = std_fs::metadata(&blocked).unwrap().permissions();
        std_fs::set_permissions(&blocked, std_fs::Permissions::from_mode(0o000)).unwrap();
        if std_fs::read_dir(&blocked).is_ok() {
            std_fs::set_permissions(&blocked, original_permissions).unwrap();
            return;
        }

        let outcome = watchable_search_directories(
            root,
            &SearchCrawlOptions {
                include_hidden: false,
                exclude_patterns: Vec::new(),
                directory_error_policy: DirectoryErrorPolicy::Abort,
                excluded_index_dir: None,
                throttle: false,
                cancel: None,
            },
        );
        std_fs::set_permissions(&blocked, original_permissions).unwrap();

        assert!(matches!(
            outcome,
            Err(IndexError::ReadDirectory { path, .. }) if path == blocked
        ));
    }
}
