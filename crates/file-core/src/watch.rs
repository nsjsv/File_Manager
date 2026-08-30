use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::FileError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryChange {
    pub path: PathBuf,
}

pub struct DirectoryWatcher {
    _watcher: RecommendedWatcher,
    events: mpsc::UnboundedReceiver<DirectoryChange>,
}

impl DirectoryWatcher {
    pub async fn recv(&mut self) -> Option<DirectoryChange> {
        self.events.recv().await
    }
}
pub fn watch_directory(
    path: impl AsRef<Path>,
    debounce: Duration,
) -> Result<DirectoryWatcher, FileError> {
    let path = path.as_ref().to_path_buf();
    let (raw_tx, raw_rx) = mpsc::unbounded_channel();
    let (change_tx, change_rx) = mpsc::unbounded_channel();

    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if result.is_ok() {
            let _ = raw_tx.send(());
        }
    })
    .map_err(|source| FileError::Watch {
        path: path.clone(),
        message: source.to_string(),
    })?;

    watcher
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(|source| FileError::Watch {
            path: path.clone(),
            message: source.to_string(),
        })?;

    tokio::spawn(debounce_events(path, raw_rx, change_tx, debounce));

    Ok(DirectoryWatcher {
        _watcher: watcher,
        events: change_rx,
    })
}

async fn debounce_events(
    path: PathBuf,
    mut raw_rx: mpsc::UnboundedReceiver<()>,
    change_tx: mpsc::UnboundedSender<DirectoryChange>,
    debounce: Duration,
) {
    while raw_rx.recv().await.is_some() {
        sleep(debounce).await;
        while raw_rx.try_recv().is_ok() {}
        let _ = change_tx.send(DirectoryChange { path: path.clone() });
    }
}
