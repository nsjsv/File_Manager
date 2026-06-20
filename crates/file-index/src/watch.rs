use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::IndexError;

const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(750);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexFileChangeBatch {
    pub(crate) paths: Vec<PathBuf>,
}

pub(crate) struct IndexWatcher {
    _watcher: RecommendedWatcher,
    changes: mpsc::UnboundedReceiver<IndexFileChangeBatch>,
}

impl IndexWatcher {
    pub(crate) async fn recv(&mut self) -> Option<IndexFileChangeBatch> {
        self.changes.recv().await
    }
}

pub(crate) fn watch_index_root(root: &Path) -> Result<IndexWatcher, IndexError> {
    watch_index_root_with_debounce(root, DEFAULT_DEBOUNCE)
}

#[cfg(test)]
pub(crate) fn watch_index_root_for_test(
    root: &Path,
    debounce: Duration,
) -> Result<IndexWatcher, IndexError> {
    watch_index_root_with_debounce(root, debounce)
}

fn watch_index_root_with_debounce(
    root: &Path,
    debounce: Duration,
) -> Result<IndexWatcher, IndexError> {
    let root = root.to_path_buf();
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

    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|error| IndexError::store(&root, error))?;

    tokio::spawn(coalesce_index_changes(raw_rx, change_tx, debounce));

    Ok(IndexWatcher {
        _watcher: watcher,
        changes: change_rx,
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
        let mut watcher = watch_index_root_for_test(dir.path(), Duration::from_millis(50)).unwrap();
        let path = dir.path().join("note.txt");

        std::fs::write(&path, "one").unwrap();
        std::fs::write(&path, "two").unwrap();

        let batch = tokio::time::timeout(Duration::from_secs(5), watcher.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(batch.paths.iter().any(|changed| changed == &path));
    }
}
