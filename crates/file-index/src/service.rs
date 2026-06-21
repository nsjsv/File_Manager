use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use crate::profile::{IndexProfile, IndexTaskPhase, ProfileStore, SearchMode};
use crate::search::{
    build_file_search_index, build_file_search_index_for_paths,
    build_file_search_index_for_paths_with_progress, clear_file_search_index_failures,
    file_search_index_snapshot, file_search_index_status, remove_file_search_index,
    search_file_index, FileSearchIndexMode, FileSearchIndexOptions, FileSearchIndexOutcome,
    FileSearchIndexProgress, FileSearchIndexStatus, FileSearchOptions, FileSearchOutcome,
};
use crate::watch::{watch_index_root, IndexFileChangeBatch};
use crate::IndexError;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

const STATUS_STREAM_CAPACITY: usize = 128;
const PAUSED_MAINTENANCE_POLL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub profile_id: String,
    pub root: PathBuf,
    pub text: String,
    pub mode: SearchMode,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSelectedPathsRequest {
    pub profile_id: String,
    pub root: PathBuf,
    pub selected_paths: Vec<PathBuf>,
    pub mode: FileSearchIndexMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexServiceCommand {
    ConfigureProfile(IndexProfile),
    LoadProfile(String),
    Query(SearchQuery),
    Rebuild { profile_id: String, root: PathBuf },
    BuildSelectedPaths(BuildSelectedPathsRequest),
    Status { profile_id: String, root: PathBuf },
    ClearFailures { profile_id: String, root: PathBuf },
    RemoveRoot { profile_id: String, root: PathBuf },
    Pause,
    Resume,
    DeleteProfile(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexServiceEvent {
    ProfileConfigured(String),
    ProfileLoaded(Option<IndexProfile>),
    QueryFinished(FileSearchOutcome),
    RebuildFinished(FileSearchIndexOutcome),
    StatusLoaded(FileSearchIndexStatus),
    FailuresCleared(FileSearchIndexStatus),
    RootRemoved(FileSearchIndexStatus),
    IncrementalUpdateStarted {
        profile_id: String,
        root: PathBuf,
        changed_paths: usize,
    },
    IncrementalUpdateFinished {
        profile_id: String,
        outcome: FileSearchIndexOutcome,
    },
    IncrementalUpdateFailed {
        profile_id: String,
        root: PathBuf,
        message: String,
    },
    Paused,
    Resumed,
    ProfileDeleted(String),
    WatchStarted {
        profile_id: String,
        root: PathBuf,
    },
    WatchFailed {
        profile_id: String,
        root: PathBuf,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct IndexServiceCore {
    inner: Arc<IndexServiceInner>,
}

pub type IndexService = IndexServiceCore;

#[derive(Debug)]
pub struct IndexMaintenanceHandle {
    token: CancellationToken,
}

impl Drop for IndexMaintenanceHandle {
    fn drop(&mut self) {
        self.token.cancel();
    }
}

#[derive(Debug)]
struct IndexServiceInner {
    profile_store: ProfileStore,
    index_base_dir: PathBuf,
    paused: AtomicBool,
    events: broadcast::Sender<IndexServiceEvent>,
    maintenance_tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl IndexServiceCore {
    pub fn open(
        control_db_path: impl Into<PathBuf>,
        index_base_dir: impl Into<PathBuf>,
    ) -> Result<Self, IndexError> {
        let (events, _) = broadcast::channel(STATUS_STREAM_CAPACITY);
        Ok(Self {
            inner: Arc::new(IndexServiceInner {
                profile_store: ProfileStore::open(control_db_path)?,
                index_base_dir: index_base_dir.into(),
                paused: AtomicBool::new(false),
                events,
                maintenance_tokens: Mutex::new(HashMap::new()),
            }),
        })
    }

    pub async fn execute(
        &self,
        command: IndexServiceCommand,
    ) -> Result<IndexServiceEvent, IndexError> {
        match command {
            IndexServiceCommand::ConfigureProfile(profile) => self.configure_profile(profile),
            IndexServiceCommand::LoadProfile(profile_id) => self.load_profile(&profile_id),
            IndexServiceCommand::Query(query) => self.query(query).await,
            IndexServiceCommand::Rebuild { profile_id, root } => {
                self.rebuild(&profile_id, root).await
            }
            IndexServiceCommand::BuildSelectedPaths(request) => {
                self.build_selected_paths(request, |_| {}).await
            }
            IndexServiceCommand::Status { profile_id, root } => {
                self.status(&profile_id, root).await
            }
            IndexServiceCommand::ClearFailures { profile_id, root } => {
                self.clear_failures(&profile_id, root).await
            }
            IndexServiceCommand::RemoveRoot { profile_id, root } => {
                self.remove_root(&profile_id, root).await
            }
            IndexServiceCommand::Pause => self.pause(),
            IndexServiceCommand::Resume => self.resume(),
            IndexServiceCommand::DeleteProfile(id) => self.delete_profile(&id).await,
        }
    }

    pub fn status_stream(&self) -> broadcast::Receiver<IndexServiceEvent> {
        self.inner.events.subscribe()
    }

    pub fn load_profiles(&self) -> Result<Vec<IndexProfile>, IndexError> {
        self.inner.profile_store.load_profiles()
    }

    pub fn configure_profile(
        &self,
        mut profile: IndexProfile,
    ) -> Result<IndexServiceEvent, IndexError> {
        profile.normalize_roots();
        let id = profile.id.clone();
        self.inner.profile_store.save_profile(&profile)?;
        for root in &profile.roots {
            self.inner.profile_store.save_task_status(
                &id,
                Some(root),
                IndexTaskPhase::Queued,
                Some("profile configured"),
            )?;
        }
        Ok(self.publish(IndexServiceEvent::ProfileConfigured(id)))
    }

    pub fn load_profile(&self, id: &str) -> Result<IndexServiceEvent, IndexError> {
        let profile = self
            .inner
            .profile_store
            .load_profiles()?
            .into_iter()
            .find(|profile| profile.id == id);
        Ok(self.publish(IndexServiceEvent::ProfileLoaded(profile)))
    }

    pub async fn query(&self, query: SearchQuery) -> Result<IndexServiceEvent, IndexError> {
        let profile = self.profile(&query.profile_id)?;
        let outcome = search_file_index(
            self.index_dir_for_root(&query.root),
            query.root,
            query.text,
            FileSearchOptions {
                include_hidden: profile.include_hidden,
                exclude_patterns: profile.exclude_patterns.clone(),
                directory_error_policy: profile.directory_error_policy,
                limit: query.limit,
                mode: query.mode,
                content_index_enabled: profile.content.enabled,
                content_max_file_bytes: profile.content.max_file_bytes,
                media_index_enabled: profile.media.enabled,
            },
        )
        .await?;
        Ok(self.publish(IndexServiceEvent::QueryFinished(outcome)))
    }

    pub async fn rebuild(
        &self,
        profile_id: &str,
        root: PathBuf,
    ) -> Result<IndexServiceEvent, IndexError> {
        if self.is_paused() {
            return Err(IndexError::store(&root, "index service is paused"));
        }
        let profile = self.profile(profile_id)?;
        self.inner.profile_store.save_task_status(
            profile_id,
            Some(&root),
            IndexTaskPhase::Running,
            Some("manual rebuild started"),
        )?;
        let outcome = build_file_search_index(
            &root,
            self.index_dir_for_root(&root),
            self.index_options_for_profile(&profile),
        )
        .await?;
        self.save_control_snapshot(profile_id, &root, &profile)
            .await?;
        self.inner.profile_store.save_task_status(
            profile_id,
            Some(&root),
            IndexTaskPhase::Finished,
            Some("manual rebuild finished"),
        )?;
        Ok(self.publish(IndexServiceEvent::RebuildFinished(outcome)))
    }

    pub async fn build_selected_paths(
        &self,
        request: BuildSelectedPathsRequest,
        progress: impl FnMut(FileSearchIndexProgress) + Send + 'static,
    ) -> Result<IndexServiceEvent, IndexError> {
        self.build_selected_paths_with_cancel(request, CancellationToken::new(), progress)
            .await
    }

    pub async fn build_selected_paths_with_cancel(
        &self,
        request: BuildSelectedPathsRequest,
        cancel: CancellationToken,
        mut progress: impl FnMut(FileSearchIndexProgress) + Send + 'static,
    ) -> Result<IndexServiceEvent, IndexError> {
        if self.is_paused() {
            return Err(IndexError::store(&request.root, "index service is paused"));
        }
        let profile = self.profile(&request.profile_id)?;
        self.inner.profile_store.save_task_status(
            &request.profile_id,
            Some(&request.root),
            IndexTaskPhase::Running,
            Some("manual build started"),
        )?;
        let mut options = self.index_options_for_profile(&profile);
        options.mode = request.mode;
        let outcome = build_file_search_index_for_paths_with_progress(
            &request.root,
            self.index_dir_for_root(&request.root),
            request.selected_paths,
            options,
            cancel,
            move |update| progress(update),
        )
        .await?;
        self.save_control_snapshot(&request.profile_id, &request.root, &profile)
            .await?;
        self.inner.profile_store.save_task_status(
            &request.profile_id,
            Some(&request.root),
            IndexTaskPhase::Finished,
            Some("manual build finished"),
        )?;
        Ok(self.publish(IndexServiceEvent::RebuildFinished(outcome)))
    }

    pub async fn status(
        &self,
        profile_id: &str,
        root: PathBuf,
    ) -> Result<IndexServiceEvent, IndexError> {
        let profile = self.profile(profile_id)?;
        let status = file_search_index_status(
            self.index_dir_for_root(&root),
            &root,
            self.index_options_for_profile(&profile),
        )
        .await?;
        Ok(self.publish(IndexServiceEvent::StatusLoaded(status)))
    }

    pub async fn clear_failures(
        &self,
        profile_id: &str,
        root: PathBuf,
    ) -> Result<IndexServiceEvent, IndexError> {
        let profile = self.profile(profile_id)?;
        clear_file_search_index_failures(self.index_dir_for_root(&root)).await?;
        self.save_control_snapshot(profile_id, &root, &profile)
            .await?;
        let status = file_search_index_status(
            self.index_dir_for_root(&root),
            &root,
            self.index_options_for_profile(&profile),
        )
        .await?;
        Ok(self.publish(IndexServiceEvent::FailuresCleared(status)))
    }

    pub async fn remove_root(
        &self,
        profile_id: &str,
        root: PathBuf,
    ) -> Result<IndexServiceEvent, IndexError> {
        let profile = self.profile(profile_id)?;
        remove_file_search_index(self.index_dir_for_root(&root)).await?;
        self.inner.profile_store.save_task_status(
            profile_id,
            Some(&root),
            IndexTaskPhase::Deleted,
            Some("root index removed"),
        )?;
        self.inner
            .profile_store
            .save_root_snapshot(profile_id, &root, &[], &[])?;
        let status = file_search_index_status(
            self.index_dir_for_root(&root),
            &root,
            self.index_options_for_profile(&profile),
        )
        .await?;
        Ok(self.publish(IndexServiceEvent::RootRemoved(status)))
    }

    pub fn pause(&self) -> Result<IndexServiceEvent, IndexError> {
        self.inner.paused.store(true, Ordering::SeqCst);
        for profile in self.inner.profile_store.load_profiles()? {
            for root in profile.roots {
                self.inner.profile_store.save_task_status(
                    &profile.id,
                    Some(&root),
                    IndexTaskPhase::Paused,
                    Some("maintenance paused"),
                )?;
            }
        }
        Ok(self.publish(IndexServiceEvent::Paused))
    }

    pub fn resume(&self) -> Result<IndexServiceEvent, IndexError> {
        self.inner.paused.store(false, Ordering::SeqCst);
        for profile in self.inner.profile_store.load_profiles()? {
            for root in profile.roots {
                self.inner.profile_store.save_task_status(
                    &profile.id,
                    Some(&root),
                    IndexTaskPhase::Queued,
                    Some("maintenance resumed"),
                )?;
            }
        }
        Ok(self.publish(IndexServiceEvent::Resumed))
    }

    pub async fn delete_profile(&self, id: &str) -> Result<IndexServiceEvent, IndexError> {
        self.cancel_profile_maintenance(id);
        if let Some(profile) = self
            .inner
            .profile_store
            .load_profiles()?
            .into_iter()
            .find(|profile| profile.id == id)
        {
            for root in &profile.roots {
                self.inner.profile_store.save_task_status(
                    id,
                    Some(root),
                    IndexTaskPhase::Deleted,
                    Some("profile deleted"),
                )?;
            }
            for root in profile.roots {
                remove_file_search_index(self.index_dir_for_root(&root)).await?;
            }
        }
        self.inner.profile_store.delete_profile(id)?;
        Ok(self.publish(IndexServiceEvent::ProfileDeleted(id.to_owned())))
    }

    pub fn maintain_profile(&self, profile_id: impl Into<String>) -> IndexMaintenanceHandle {
        let service = self.clone();
        let profile_id = profile_id.into();
        let token = self.replace_profile_maintenance_token(&profile_id);
        let handle = IndexMaintenanceHandle {
            token: token.clone(),
        };
        tokio::spawn(async move {
            service.maintain_profile_task(profile_id, token).await;
        });
        handle
    }

    async fn maintain_profile_task(self, profile_id: String, token: CancellationToken) {
        let profile = match self.profile(&profile_id) {
            Ok(profile) => profile,
            Err(error) => {
                self.publish(IndexServiceEvent::WatchFailed {
                    profile_id,
                    root: self.inner.index_base_dir.clone(),
                    message: error.to_string(),
                });
                return;
            }
        };

        for root in profile.roots {
            let service = self.clone();
            let profile_id = profile.id.clone();
            let token = token.child_token();
            tokio::spawn(async move {
                service.maintain_root_task(profile_id, root, token).await;
            });
        }
    }

    async fn maintain_root_task(self, profile_id: String, root: PathBuf, token: CancellationToken) {
        let mut watcher = match watch_index_root(&root) {
            Ok(watcher) => watcher,
            Err(error) => {
                let message = error.to_string();
                let _ = self.inner.profile_store.save_task_status(
                    &profile_id,
                    Some(&root),
                    IndexTaskPhase::Failed,
                    Some(&message),
                );
                self.publish(IndexServiceEvent::WatchFailed {
                    profile_id,
                    root,
                    message,
                });
                return;
            }
        };

        let _ = self.inner.profile_store.save_task_status(
            &profile_id,
            Some(&root),
            IndexTaskPhase::Queued,
            Some("watch started"),
        );
        self.publish(IndexServiceEvent::WatchStarted {
            profile_id: profile_id.clone(),
            root: root.clone(),
        });
        self.reconcile_root_after_startup(&profile_id, &root).await;

        loop {
            let batch = tokio::select! {
                _ = token.cancelled() => break,
                batch = watcher.recv() => batch,
            };
            let Some(batch) = batch else {
                break;
            };
            self.wait_until_resumed().await;
            if token.is_cancelled() {
                break;
            }
            self.apply_change_batch(&profile_id, &root, batch).await;
        }
    }

    async fn reconcile_root_after_startup(&self, profile_id: &str, root: &Path) {
        let index_dir = self.index_dir_for_root(root);
        if !index_dir.join("catalog.sqlite").is_file() {
            return;
        }
        self.wait_until_resumed().await;
        self.publish(IndexServiceEvent::IncrementalUpdateStarted {
            profile_id: profile_id.to_owned(),
            root: root.to_path_buf(),
            changed_paths: 1,
        });
        if let Err(error) = self.inner.profile_store.save_task_status(
            profile_id,
            Some(root),
            IndexTaskPhase::Running,
            Some("startup reconcile started"),
        ) {
            self.publish_incremental_failure(profile_id, root, error.to_string());
            return;
        }

        let profile = match self.profile(profile_id) {
            Ok(profile) => profile,
            Err(error) => {
                self.publish_incremental_failure(profile_id, root, error.to_string());
                return;
            }
        };
        let mut options = self.index_options_for_profile(&profile);
        options.mode = FileSearchIndexMode::Incremental;
        let outcome = build_file_search_index(root, index_dir, options).await;

        match outcome {
            Ok(outcome) => {
                if let Err(error) = self.save_control_snapshot(profile_id, root, &profile).await {
                    self.publish_incremental_failure(profile_id, root, error.to_string());
                    return;
                }
                let _ = self.inner.profile_store.save_task_status(
                    profile_id,
                    Some(root),
                    IndexTaskPhase::Finished,
                    Some("startup reconcile finished"),
                );
                self.publish(IndexServiceEvent::IncrementalUpdateFinished {
                    profile_id: profile_id.to_owned(),
                    outcome,
                });
            }
            Err(error) => {
                self.publish_incremental_failure(profile_id, root, error.to_string());
            }
        }
    }

    async fn apply_change_batch(&self, profile_id: &str, root: &Path, batch: IndexFileChangeBatch) {
        let selected_paths = selected_paths_for_incremental_update(root, batch.paths);
        if selected_paths.is_empty() {
            return;
        }

        self.publish(IndexServiceEvent::IncrementalUpdateStarted {
            profile_id: profile_id.to_owned(),
            root: root.to_path_buf(),
            changed_paths: selected_paths.len(),
        });
        if let Err(error) = self.inner.profile_store.save_task_status(
            profile_id,
            Some(root),
            IndexTaskPhase::Running,
            Some("incremental update started"),
        ) {
            self.publish_incremental_failure(profile_id, root, error.to_string());
            return;
        }

        let profile = match self.profile(profile_id) {
            Ok(profile) => profile,
            Err(error) => {
                self.publish_incremental_failure(profile_id, root, error.to_string());
                return;
            }
        };

        let mut options = self.index_options_for_profile(&profile);
        options.mode = FileSearchIndexMode::Incremental;
        let outcome = build_file_search_index_for_paths(
            root,
            self.index_dir_for_root(root),
            selected_paths,
            options,
        )
        .await;

        match outcome {
            Ok(outcome) => {
                if let Err(error) = self.save_control_snapshot(profile_id, root, &profile).await {
                    self.publish_incremental_failure(profile_id, root, error.to_string());
                    return;
                }
                let _ = self.inner.profile_store.save_task_status(
                    profile_id,
                    Some(root),
                    IndexTaskPhase::Finished,
                    Some("incremental update finished"),
                );
                self.publish(IndexServiceEvent::IncrementalUpdateFinished {
                    profile_id: profile_id.to_owned(),
                    outcome,
                });
            }
            Err(error) => {
                self.publish_incremental_failure(profile_id, root, error.to_string());
            }
        }
    }

    fn profile(&self, id: &str) -> Result<IndexProfile, IndexError> {
        self.inner
            .profile_store
            .load_profiles()?
            .into_iter()
            .find(|profile| profile.id == id)
            .ok_or_else(|| {
                IndexError::store(&self.inner.index_base_dir, format!("missing profile {id}"))
            })
    }

    async fn save_control_snapshot(
        &self,
        profile_id: &str,
        root: &Path,
        profile: &IndexProfile,
    ) -> Result<(), IndexError> {
        let (records, failures) = file_search_index_snapshot(
            self.index_dir_for_root(root),
            root,
            self.index_options_for_profile(profile),
        )
        .await?;
        self.inner
            .profile_store
            .save_root_snapshot(profile_id, root, &records, &failures)
    }

    fn index_dir_for_root(&self, root: &Path) -> PathBuf {
        crate::layout::search_index_dir_for_root(&self.inner.index_base_dir, root)
    }

    fn index_options_for_profile(&self, profile: &IndexProfile) -> FileSearchIndexOptions {
        FileSearchIndexOptions {
            include_hidden: profile.include_hidden,
            exclude_patterns: profile.exclude_patterns.clone(),
            directory_error_policy: profile.directory_error_policy,
            content_index_enabled: profile.content.enabled,
            content_max_file_bytes: profile.content.max_file_bytes,
            media_index_enabled: profile.media.enabled,
            ..FileSearchIndexOptions::default()
        }
    }

    fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::SeqCst)
    }

    async fn wait_until_resumed(&self) {
        while self.is_paused() {
            tokio::time::sleep(PAUSED_MAINTENANCE_POLL).await;
        }
    }

    fn publish(&self, event: IndexServiceEvent) -> IndexServiceEvent {
        let _ = self.inner.events.send(event.clone());
        event
    }

    fn publish_incremental_failure(&self, profile_id: &str, root: &Path, message: String) {
        let _ = self.inner.profile_store.save_task_status(
            profile_id,
            Some(root),
            IndexTaskPhase::Failed,
            Some(&message),
        );
        self.publish(IndexServiceEvent::IncrementalUpdateFailed {
            profile_id: profile_id.to_owned(),
            root: root.to_path_buf(),
            message,
        });
    }

    fn replace_profile_maintenance_token(&self, profile_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        let mut tokens = self
            .inner
            .maintenance_tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(previous) = tokens.insert(profile_id.to_owned(), token.clone()) {
            previous.cancel();
        }
        token
    }

    fn cancel_profile_maintenance(&self, profile_id: &str) {
        let mut tokens = self
            .inner
            .maintenance_tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(token) = tokens.remove(profile_id) {
            token.cancel();
        }
    }
}

fn selected_paths_for_incremental_update(root: &Path, paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut selected = paths
        .into_iter()
        .filter_map(|path| {
            if path == root {
                Some(path)
            } else {
                path.parent()
                    .filter(|parent| parent.starts_with(root))
                    .map(Path::to_path_buf)
            }
        })
        .collect::<Vec<_>>();
    selected.sort_unstable();
    selected.dedup();
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_update_rescans_parent_directory_for_deleted_file() {
        let root = PathBuf::from("/tmp/root");
        let selected =
            selected_paths_for_incremental_update(&root, vec![root.join("nested/note.txt")]);

        assert_eq!(selected, vec![root.join("nested")]);
    }
}
