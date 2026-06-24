use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::search::{watchable_search_directories, DirectoryErrorPolicy, SearchCrawlOptions};
use crate::IndexError;
use file_core::ScanWarning;

const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(750);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexFileChangeBatch {
    pub(crate) paths: Vec<PathBuf>,
}

pub(crate) struct IndexWatcher {
    _watcher: RecommendedWatcher,
    changes: mpsc::UnboundedReceiver<IndexFileChangeBatch>,
    registration_warnings: Vec<ScanWarning>,
}

impl IndexWatcher {
    pub(crate) async fn recv(&mut self) -> Option<IndexFileChangeBatch> {
        self.changes.recv().await
    }

    pub(crate) fn registration_warnings(&self) -> &[ScanWarning] {
        &self.registration_warnings
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WatchIndexRootOptions {
    pub(crate) include_hidden: bool,
    pub(crate) exclude_patterns: Vec<String>,
    pub(crate) directory_error_policy: DirectoryErrorPolicy,
    pub(crate) excluded_index_dir: Option<PathBuf>,
}

impl Default for WatchIndexRootOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
            exclude_patterns: Vec::new(),
            directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
            excluded_index_dir: None,
        }
    }
}

pub(crate) fn watch_index_root(
    root: &Path,
    options: WatchIndexRootOptions,
) -> Result<IndexWatcher, IndexError> {
    watch_index_root_with_debounce(root, options, DEFAULT_DEBOUNCE)
}

#[cfg(test)]
pub(crate) fn watch_index_root_for_test(
    root: &Path,
    options: WatchIndexRootOptions,
    debounce: Duration,
) -> Result<IndexWatcher, IndexError> {
    watch_index_root_with_debounce(root, options, debounce)
}

fn watch_index_root_with_debounce(
    root: &Path,
    options: WatchIndexRootOptions,
    debounce: Duration,
) -> Result<IndexWatcher, IndexError> {
    let root = root.to_path_buf();
    let (directories, mut registration_warnings) = watchable_search_directories(
        &root,
        &SearchCrawlOptions {
            include_hidden: options.include_hidden,
            exclude_patterns: options.exclude_patterns.clone(),
            directory_error_policy: options.directory_error_policy,
            excluded_index_dir: options.excluded_index_dir.clone(),
            throttle: false,
            cancel: None,
        },
    )?;
    let (raw_tx, raw_rx) = mpsc::unbounded_channel();
    let (change_tx, change_rx) = mpsc::unbounded_channel();
    let callback_root = root.clone();

    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            if index_event_may_change_catalog(&event.kind) {
                let paths = event
                    .paths
                    .into_iter()
                    .filter(|path| path.starts_with(&callback_root))
                    .collect::<Vec<_>>();
                if !paths.is_empty() {
                    let _ = raw_tx.send(paths);
                }
            }
        }
    })
    .map_err(|error| IndexError::store(&root, error))?;

    for directory in directories {
        if let Err(error) = watcher.watch(&directory, RecursiveMode::NonRecursive) {
            if directory == root || options.directory_error_policy == DirectoryErrorPolicy::Abort {
                return Err(IndexError::store(&directory, error));
            }
            registration_warnings.push(ScanWarning {
                path: directory,
                message: error.to_string(),
            });
        }
    }

    tokio::spawn(coalesce_index_changes(raw_rx, change_tx, debounce));

    Ok(IndexWatcher {
        _watcher: watcher,
        changes: change_rx,
        registration_warnings,
    })
}

fn index_event_may_change_catalog(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Modify(_)
            | EventKind::Remove(_)
            | EventKind::Any
            | EventKind::Other
    )
}

async fn coalesce_index_changes(
    mut raw_rx: mpsc::UnboundedReceiver<Vec<PathBuf>>,
    change_tx: mpsc::UnboundedSender<IndexFileChangeBatch>,
    debounce: Duration,
) {
    while let Some(paths) = raw_rx.recv().await {
        let mut pending = path_set_from(paths);
        sleep(debounce).await;
        while let Ok(paths) = raw_rx.try_recv() {
            pending.extend(paths);
        }
        if !pending.is_empty() {
            let mut paths = pending.into_iter().collect::<Vec<_>>();
            paths.sort_unstable();
            let _ = change_tx.send(IndexFileChangeBatch { paths });
        }
    }
}

fn path_set_from(paths: Vec<PathBuf>) -> HashSet<PathBuf> {
    paths.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn watcher_coalesces_create_and_modify_paths() {
        let dir = tempdir().unwrap();
        let mut watcher = watch_index_root_for_test(
            dir.path(),
            WatchIndexRootOptions::default(),
            Duration::from_millis(50),
        )
        .unwrap();
        let path = dir.path().join("note.txt");

        std::fs::write(&path, "one").unwrap();
        std::fs::write(&path, "two").unwrap();

        let batch = tokio::time::timeout(Duration::from_secs(5), watcher.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(batch.paths.iter().any(|changed| changed == &path));
    }

    #[tokio::test]
    async fn watcher_does_not_emit_changes_from_excluded_existing_directories() {
        let dir = tempdir().unwrap();
        let ignored = dir.path().join("node_modules");
        std::fs::create_dir_all(&ignored).unwrap();
        let mut watcher = watch_index_root_for_test(
            dir.path(),
            WatchIndexRootOptions {
                exclude_patterns: vec!["node_modules/".to_owned()],
                ..WatchIndexRootOptions::default()
            },
            Duration::from_millis(50),
        )
        .unwrap();

        std::fs::write(ignored.join("package.json"), "{}").unwrap();
        let ignored_event = tokio::time::timeout(Duration::from_millis(250), watcher.recv()).await;
        assert!(ignored_event.is_err());

        let visible = dir.path().join("note.txt");
        std::fs::write(&visible, "one").unwrap();
        let batch = tokio::time::timeout(Duration::from_secs(5), watcher.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(batch.paths.iter().any(|changed| changed == &visible));
    }
}
