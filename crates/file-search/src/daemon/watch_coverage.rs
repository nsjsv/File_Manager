use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::config::{SearchExcludeRules, SearchIndexConfig};
use crate::database::{SearchDatabase, MAX_KNOWN_ENTRY_PAGE_ENTRIES};
use crate::error::SearchResult;
use crate::filesystem::{FilesystemObservation, LocalFilesystemBoundary};
use crate::model::SearchFileKind;

use super::bounded_paths::{estimated_path_bytes, BoundedPathSet};
use super::watch_ingress::{deliver_watch_event, WatchOverflowState};

const TARGET_REGISTERED_DIRECTORIES: usize = 32_768;
const RESERVED_SYSTEM_WATCHES: usize = 500;
const MAX_REGISTERED_PATH_BYTES: usize = 16_000_000;
const MAX_COVERAGE_GAPS: usize = 256;
const MAX_COVERAGE_GAP_BYTES: usize = 262_144;
const MAX_COVERAGE_MESSAGE_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WatchCoverageHealth {
    Healthy,
    Incomplete { gap_count: usize, message: String },
    BackendUnavailable { message: String },
}

pub(super) struct WatchCoverageRefresh {
    pub(super) repair_paths: Vec<PathBuf>,
    pub(super) patrol_paths: Vec<PathBuf>,
    pub(super) health: WatchCoverageHealth,
}

trait DirectoryWatchBackend {
    fn watch_directory(&mut self, path: &Path) -> notify::Result<()>;
    fn unwatch_directory(&mut self, path: &Path) -> notify::Result<()>;
}

impl DirectoryWatchBackend for RecommendedWatcher {
    fn watch_directory(&mut self, path: &Path) -> notify::Result<()> {
        Watcher::watch(self, path, RecursiveMode::NonRecursive)
    }

    fn unwatch_directory(&mut self, path: &Path) -> notify::Result<()> {
        Watcher::unwatch(self, path)
    }
}

pub(super) struct RecommendedWatchCoverage {
    inner: DirectoryWatchCoverage<RecommendedWatcher>,
    database_path: PathBuf,
}

impl RecommendedWatchCoverage {
    pub(super) fn create(
        config: SearchIndexConfig,
        database_path: PathBuf,
        event_sender: SyncSender<notify::Result<Event>>,
        overflow_state: Arc<WatchOverflowState>,
    ) -> SearchResult<Self> {
        let watcher = notify::recommended_watcher(move |event_result| {
            deliver_watch_event(&event_sender, &overflow_state, event_result);
        })?;
        Ok(Self {
            inner: DirectoryWatchCoverage::new(watcher, config),
            database_path,
        })
    }

    pub(super) fn initialize(&mut self) -> SearchResult<WatchCoverageRefresh> {
        self.inner.register_roots();
        self.restore_snapshot_directories()
    }

    pub(super) fn restore_snapshot_directories(&mut self) -> SearchResult<WatchCoverageRefresh> {
        let previous_gaps = self.inner.current_gaps();
        let database = SearchDatabase::open_read_only(&self.database_path)?;
        let mut after_path = None;
        loop {
            let page = database.directory_snapshot_paths_page(
                after_path.as_deref(),
                MAX_KNOWN_ENTRY_PAGE_ENTRIES,
            )?;
            if page.is_empty() {
                break;
            }
            after_path = page.last().cloned();
            self.inner.register_snapshot_directories(page);
        }
        Ok(self.inner.finish_refresh(previous_gaps))
    }

    pub(super) fn refresh_changed_paths(
        &mut self,
        changed_paths: &[PathBuf],
    ) -> WatchCoverageRefresh {
        self.inner.refresh_changed_paths(changed_paths)
    }

    pub(super) fn retry_gaps(&mut self) -> WatchCoverageRefresh {
        self.inner.retry_gaps()
    }

    pub(super) fn record_notify_error(&mut self, error: &notify::Error) {
        self.inner.record_notify_error(error);
    }

    pub(super) fn mark_backend_unavailable(&mut self, message: impl Into<String>) {
        self.inner.backend_failure = Some(bounded_message(message.into()));
    }

    pub(super) fn health(&self) -> WatchCoverageHealth {
        self.inner.health()
    }
}

struct BoundedCoverageGaps {
    entries: BTreeMap<PathBuf, String>,
    estimated_bytes: usize,
    max_entries: usize,
    max_estimated_bytes: usize,
}

impl BoundedCoverageGaps {
    fn new(max_entries: usize, max_estimated_bytes: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            estimated_bytes: 0,
            max_entries,
            max_estimated_bytes,
        }
    }

    fn insert(&mut self, path: PathBuf, message: String) -> Result<(), ()> {
        let message = bounded_message(message);
        if let Some(previous_message) = self.entries.get_mut(&path) {
            let next_bytes = self
                .estimated_bytes
                .saturating_sub(previous_message.len())
                .saturating_add(message.len());
            if next_bytes > self.max_estimated_bytes {
                return Err(());
            }
            self.estimated_bytes = next_bytes;
            *previous_message = message;
            return Ok(());
        }

        let entry_bytes = estimated_path_bytes(&path).saturating_add(message.len());
        let next_bytes = self.estimated_bytes.saturating_add(entry_bytes);
        if self.entries.len() >= self.max_entries || next_bytes > self.max_estimated_bytes {
            return Err(());
        }
        self.entries.insert(path, message);
        self.estimated_bytes = next_bytes;
        Ok(())
    }

    fn remove(&mut self, path: &Path) -> Option<String> {
        let message = self.entries.remove(path)?;
        self.estimated_bytes = self
            .estimated_bytes
            .saturating_sub(estimated_path_bytes(path).saturating_add(message.len()));
        Some(message)
    }

    fn keys(&self) -> impl Iterator<Item = &PathBuf> {
        self.entries.keys()
    }

    #[cfg(test)]
    fn contains_key(&self, path: &Path) -> bool {
        self.entries.contains_key(path)
    }

    fn retain(&mut self, mut retain_entry: impl FnMut(&Path) -> bool) {
        let removed_paths = self
            .entries
            .keys()
            .filter(|path| !retain_entry(path))
            .cloned()
            .collect::<Vec<_>>();
        for removed_path in removed_paths {
            self.remove(&removed_path);
        }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchRegistration {
    Ready,
    Failed,
    CapacityReached,
}

struct DirectoryWatchCoverage<Backend> {
    backend: Backend,
    config: SearchIndexConfig,
    rules: SearchExcludeRules,
    registered_directories: BoundedPathSet,
    coverage_gaps: BoundedCoverageGaps,
    coverage_budget_exceeded_roots: BTreeSet<PathBuf>,
    backend_failure: Option<String>,
}

impl<Backend: DirectoryWatchBackend> DirectoryWatchCoverage<Backend> {
    fn new(backend: Backend, config: SearchIndexConfig) -> Self {
        Self {
            rules: SearchExcludeRules::new(config.excluded_paths.clone()),
            backend,
            config,
            registered_directories: BoundedPathSet::new(
                recommended_watch_limit(),
                MAX_REGISTERED_PATH_BYTES,
            ),
            coverage_gaps: BoundedCoverageGaps::new(MAX_COVERAGE_GAPS, MAX_COVERAGE_GAP_BYTES),
            coverage_budget_exceeded_roots: BTreeSet::new(),
            backend_failure: None,
        }
    }

    fn register_roots(&mut self) {
        self.backend_failure = None;
        let roots = self.config.roots.clone();
        for root in roots {
            self.refresh_path(&root);
        }
    }

    fn register_snapshot_directories(&mut self, directories: Vec<PathBuf>) {
        for directory in directories {
            self.register_directory(&directory);
        }
    }

    fn refresh_changed_paths(&mut self, changed_paths: &[PathBuf]) -> WatchCoverageRefresh {
        let previous_gaps = self.current_gaps();
        for changed_path in changed_paths {
            self.refresh_path(changed_path);
        }
        self.finish_refresh(previous_gaps)
    }

    fn retry_gaps(&mut self) -> WatchCoverageRefresh {
        let previous_gaps = self.current_gaps();
        let retry_paths = previous_gaps.iter().cloned().collect::<Vec<_>>();
        for retry_path in retry_paths {
            self.refresh_path(&retry_path);
        }
        self.finish_refresh(previous_gaps)
    }

    fn current_gaps(&self) -> BTreeSet<PathBuf> {
        self.coverage_gaps.keys().cloned().collect()
    }

    fn finish_refresh(&self, previous_gaps: BTreeSet<PathBuf>) -> WatchCoverageRefresh {
        let current_gaps = self.current_gaps();
        let repair_paths = previous_gaps.difference(&current_gaps).cloned().collect();
        let patrol_paths = current_gaps.into_iter().collect::<Vec<_>>();
        // ponytail: 容量耗尽时只降级，不把整个 root 偷换成周期巡检；超过 32,768 个目录后需持久游标分片。
        WatchCoverageRefresh {
            repair_paths,
            patrol_paths,
            health: self.health(),
        }
    }

    fn refresh_path(&mut self, path: &Path) {
        let Some(root) = self
            .config
            .roots
            .iter()
            .filter(|root| path == root.as_path() || path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .cloned()
        else {
            return;
        };

        let boundary = match LocalFilesystemBoundary::observe(&root, &self.rules) {
            Ok(FilesystemObservation::Complete(boundary)) => boundary,
            Ok(FilesystemObservation::Inaccessible { scope }) => {
                self.record_gap(scope, "search root is inaccessible");
                return;
            }
            Ok(FilesystemObservation::Missing { scope }) => {
                self.prune_scope(&scope);
                self.record_gap(scope, "search root is missing");
                return;
            }
            Ok(FilesystemObservation::PolicyExcluded { scope }) => {
                self.prune_scope(&scope);
                return;
            }
            Err(error) => {
                self.record_gap(root, error.to_string());
                return;
            }
        };

        match boundary.inspect_path(path) {
            Ok(FilesystemObservation::Complete(entry)) => {
                self.coverage_gaps.remove(path);
                if entry.kind() == SearchFileKind::Directory {
                    self.register_directory(path);
                } else {
                    self.prune_scope(path);
                }
            }
            Ok(FilesystemObservation::Inaccessible { scope }) => {
                self.record_gap(scope, "directory is inaccessible");
            }
            Ok(FilesystemObservation::Missing { scope })
            | Ok(FilesystemObservation::PolicyExcluded { scope }) => {
                self.prune_scope(&scope);
            }
            Err(error) => self.record_gap(path.to_path_buf(), error.to_string()),
        }
    }

    fn register_directory(&mut self, directory: &Path) -> WatchRegistration {
        if self.registered_directories.contains(directory) {
            self.coverage_gaps.remove(directory);
            return WatchRegistration::Ready;
        }
        if self
            .registered_directories
            .insert(directory.to_path_buf())
            .is_err()
        {
            self.mark_root_coverage_budget_exceeded(directory);
            return WatchRegistration::CapacityReached;
        }
        match self.backend.watch_directory(directory) {
            Ok(()) => {
                self.coverage_gaps.remove(directory);
                WatchRegistration::Ready
            }
            Err(error) => {
                self.registered_directories.remove(directory);
                self.record_gap(directory.to_path_buf(), error.to_string());
                WatchRegistration::Failed
            }
        }
    }

    fn prune_scope(&mut self, scope: &Path) {
        let registered_descendants = self
            .registered_directories
            .iter()
            .filter(|directory| directory.as_path() == scope || directory.starts_with(scope))
            .cloned()
            .collect::<Vec<_>>();
        for registered_directory in registered_descendants.into_iter().rev() {
            let _ = self.backend.unwatch_directory(&registered_directory);
            self.registered_directories.remove(&registered_directory);
        }
        self.coverage_gaps
            .retain(|gap| gap != scope && !gap.starts_with(scope));
    }

    fn record_gap(&mut self, scope: PathBuf, message: impl Into<String>) {
        if self
            .coverage_gaps
            .insert(scope.clone(), message.into())
            .is_err()
        {
            self.mark_root_coverage_budget_exceeded(&scope);
        }
    }

    fn record_notify_error(&mut self, error: &notify::Error) {
        if error.paths.is_empty() {
            self.backend_failure = Some(bounded_message(error.to_string()));
            return;
        }
        for path in &error.paths {
            self.record_gap(path.clone(), error.to_string());
        }
    }

    fn health(&self) -> WatchCoverageHealth {
        if let Some(message) = &self.backend_failure {
            return WatchCoverageHealth::BackendUnavailable {
                message: message.clone(),
            };
        }
        if self.coverage_gaps.is_empty() && self.coverage_budget_exceeded_roots.is_empty() {
            WatchCoverageHealth::Healthy
        } else if !self.coverage_budget_exceeded_roots.is_empty() {
            WatchCoverageHealth::Incomplete {
                gap_count: self
                    .coverage_gaps
                    .len()
                    .saturating_add(self.coverage_budget_exceeded_roots.len()),
                message: format!(
                    "{} search roots exceed the real-time watch budget; targeted patrol is active",
                    self.coverage_budget_exceeded_roots.len()
                ),
            }
        } else {
            WatchCoverageHealth::Incomplete {
                gap_count: self.coverage_gaps.len(),
                message: format!(
                    "{} search directory watches are unavailable; retrying with targeted patrol",
                    self.coverage_gaps.len()
                ),
            }
        }
    }

    fn mark_root_coverage_budget_exceeded(&mut self, path: &Path) {
        if let Some(root) = self
            .config
            .roots
            .iter()
            .filter(|root| path == root.as_path() || path.starts_with(root))
            .max_by_key(|root| root.components().count())
        {
            self.coverage_budget_exceeded_roots.insert(root.clone());
        } else {
            self.coverage_budget_exceeded_roots
                .extend(self.config.roots.iter().cloned());
        }
    }
}

fn recommended_watch_limit() -> usize {
    std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(watch_limit_for)
        .unwrap_or(TARGET_REGISTERED_DIRECTORIES)
}

fn watch_limit_for(system_limit: usize) -> usize {
    system_limit
        .saturating_sub(RESERVED_SYSTEM_WATCHES)
        .min(TARGET_REGISTERED_DIRECTORIES)
        .max(1)
}

fn bounded_message(mut message: String) -> String {
    if message.len() <= MAX_COVERAGE_MESSAGE_BYTES {
        return message;
    }
    let mut boundary = MAX_COVERAGE_MESSAGE_BYTES;
    while !message.is_char_boundary(boundary) {
        boundary -= 1;
    }
    message.truncate(boundary);
    message
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::io;

    use tempfile::tempdir;

    use super::*;

    #[derive(Default)]
    struct RecordingBackend {
        watched: BTreeSet<PathBuf>,
        failing_paths: BTreeSet<PathBuf>,
    }

    impl DirectoryWatchBackend for RecordingBackend {
        fn watch_directory(&mut self, path: &Path) -> notify::Result<()> {
            if self.failing_paths.contains(path) {
                return Err(notify::Error::io(io::Error::from(
                    io::ErrorKind::PermissionDenied,
                )));
            }
            self.watched.insert(path.to_path_buf());
            Ok(())
        }

        fn unwatch_directory(&mut self, path: &Path) -> notify::Result<()> {
            self.watched.remove(path);
            Ok(())
        }
    }

    fn config_for(root: &Path) -> SearchIndexConfig {
        SearchIndexConfig {
            roots: vec![root.to_path_buf()],
            excluded_paths: Vec::new(),
            content_indexing_enabled: true,
            max_extract_bytes: 1024,
        }
    }

    #[test]
    fn startup_registers_roots_and_persisted_directories_without_walking() {
        let directory = tempdir().unwrap();
        let visible = directory.path().join("visible");
        let nested = visible.join("nested");
        std::fs::create_dir_all(&nested).unwrap();

        let mut coverage =
            DirectoryWatchCoverage::new(RecordingBackend::default(), config_for(directory.path()));
        coverage.register_roots();
        coverage.register_snapshot_directories(vec![visible.clone(), nested.clone()]);

        assert!(coverage.registered_directories.contains(directory.path()));
        assert!(coverage.registered_directories.contains(&visible));
        assert!(coverage.registered_directories.contains(&nested));
    }

    #[test]
    fn one_directory_watch_failure_does_not_discard_sibling_coverage() {
        let directory = tempdir().unwrap();
        let denied = directory.path().join("denied");
        let healthy = directory.path().join("healthy");
        std::fs::create_dir_all(&denied).unwrap();
        std::fs::create_dir_all(&healthy).unwrap();
        let mut backend = RecordingBackend::default();
        backend.failing_paths.insert(denied.clone());

        let mut coverage = DirectoryWatchCoverage::new(backend, config_for(directory.path()));
        coverage.register_roots();
        coverage.register_snapshot_directories(vec![denied.clone(), healthy.clone()]);

        assert!(coverage.registered_directories.contains(&healthy));
        assert!(coverage.coverage_gaps.contains_key(&denied));
    }

    #[test]
    fn successful_retry_clears_gap_and_requests_a_repair_scan() {
        let directory = tempdir().unwrap();
        let denied = directory.path().join("denied");
        std::fs::create_dir_all(&denied).unwrap();
        let mut backend = RecordingBackend::default();
        backend.failing_paths.insert(denied.clone());
        let mut coverage = DirectoryWatchCoverage::new(backend, config_for(directory.path()));
        coverage.register_snapshot_directories(vec![denied.clone()]);

        coverage.backend.failing_paths.remove(&denied);
        let refresh = coverage.retry_gaps();

        assert_eq!(refresh.health, WatchCoverageHealth::Healthy);
        assert_eq!(refresh.repair_paths, vec![denied]);
    }

    #[test]
    fn directory_registration_capacity_degrades_by_root_without_growing_paths() {
        let directory = tempdir().unwrap();
        let child = directory.path().join("child");
        std::fs::create_dir_all(&child).unwrap();
        let mut coverage =
            DirectoryWatchCoverage::new(RecordingBackend::default(), config_for(directory.path()));
        coverage.registered_directories = BoundedPathSet::new(1, usize::MAX);

        coverage.register_roots();
        coverage.register_snapshot_directories(vec![child]);

        assert_eq!(coverage.registered_directories.len(), 1);
        assert_eq!(
            coverage.coverage_budget_exceeded_roots,
            BTreeSet::from([directory.path().to_path_buf()])
        );
        let refresh = coverage.finish_refresh(BTreeSet::new());
        assert!(refresh.patrol_paths.is_empty());
        assert!(matches!(
            refresh.health,
            WatchCoverageHealth::Incomplete { .. }
        ));
    }

    #[test]
    fn target_limit_can_cover_the_observed_twenty_thousand_directories() {
        assert!(watch_limit_for(524_288) >= 20_825);
    }
}
