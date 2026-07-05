use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::profile::{IndexProfile, IndexTaskPhase, ProfileStore, SearchMode};
use crate::search::{
    build_file_search_index, build_file_search_index_for_paths_with_progress,
    clear_file_search_index_failures, file_search_index_status, remove_file_search_index,
    search_file_index_with_cancel, FileSearchIndexOptions, FileSearchIndexOutcome,
    FileSearchIndexProgress, FileSearchIndexStatus, FileSearchOptions, FileSearchOutcome,
};
use crate::IndexError;
use tokio_util::sync::CancellationToken;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexServiceCommand {
    Ping,
    Shutdown,
    ConfigureProfile(IndexProfile),
    LoadProfile(String),
    Query(SearchQuery),
    Rebuild { profile_id: String, root: PathBuf },
    BuildSelectedPaths(BuildSelectedPathsRequest),
    Status { profile_id: String, root: PathBuf },
    ClearFailures { profile_id: String, root: PathBuf },
    RemoveRoot { profile_id: String, root: PathBuf },
    DeleteProfile(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexServiceEvent {
    Pong { daemon_version: String },
    Shutdown,
    ProfileConfigured(String),
    ProfileLoaded(Option<IndexProfile>),
    QueryFinished(FileSearchOutcome),
    RebuildFinished(FileSearchIndexOutcome),
    StatusLoaded(FileSearchIndexStatus),
    FailuresCleared(FileSearchIndexStatus),
    RootRemoved(FileSearchIndexStatus),
    ProfileDeleted(String),
}

#[derive(Debug, Clone)]
pub struct IndexServiceCore {
    inner: Arc<IndexServiceInner>,
}

pub type IndexService = IndexServiceCore;

#[derive(Debug)]
struct IndexServiceInner {
    profile_store: ProfileStore,
    index_base_dir: PathBuf,
}

impl IndexServiceCore {
    pub fn open(
        control_db_path: impl Into<PathBuf>,
        index_base_dir: impl Into<PathBuf>,
    ) -> Result<Self, IndexError> {
        Ok(Self {
            inner: Arc::new(IndexServiceInner {
                profile_store: ProfileStore::open(control_db_path)?,
                index_base_dir: index_base_dir.into(),
            }),
        })
    }

    pub async fn execute(
        &self,
        command: IndexServiceCommand,
    ) -> Result<IndexServiceEvent, IndexError> {
        match command {
            IndexServiceCommand::Ping => Ok(IndexServiceEvent::Pong {
                daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
            }),
            IndexServiceCommand::Shutdown => Ok(IndexServiceEvent::Shutdown),
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
            IndexServiceCommand::DeleteProfile(id) => self.delete_profile(&id).await,
        }
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
        Ok(IndexServiceEvent::ProfileConfigured(id))
    }

    pub fn load_profile(&self, id: &str) -> Result<IndexServiceEvent, IndexError> {
        let profile = self
            .inner
            .profile_store
            .load_profiles()?
            .into_iter()
            .find(|profile| profile.id == id);
        Ok(IndexServiceEvent::ProfileLoaded(profile))
    }

    pub async fn query(&self, query: SearchQuery) -> Result<IndexServiceEvent, IndexError> {
        self.query_with_cancel(query, CancellationToken::new())
            .await
    }

    pub async fn query_with_cancel(
        &self,
        query: SearchQuery,
        cancel: CancellationToken,
    ) -> Result<IndexServiceEvent, IndexError> {
        let profile = self.profile(&query.profile_id)?;
        let outcome = search_file_index_with_cancel(
            self.index_dir_for_root(&query.root),
            &query.root,
            &query.text,
            self.query_options_for_profile(&profile, query.mode, query.limit),
            cancel,
        )
        .await?;
        Ok(IndexServiceEvent::QueryFinished(outcome))
    }

    pub async fn rebuild(
        &self,
        profile_id: &str,
        root: PathBuf,
    ) -> Result<IndexServiceEvent, IndexError> {
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
        self.inner.profile_store.save_task_status(
            profile_id,
            Some(&root),
            IndexTaskPhase::Finished,
            Some("manual rebuild finished"),
        )?;
        Ok(IndexServiceEvent::RebuildFinished(outcome))
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
        let profile = self.profile(&request.profile_id)?;
        self.inner.profile_store.save_task_status(
            &request.profile_id,
            Some(&request.root),
            IndexTaskPhase::Running,
            Some("manual build started"),
        )?;
        let outcome = build_file_search_index_for_paths_with_progress(
            &request.root,
            self.index_dir_for_root(&request.root),
            request.selected_paths,
            self.index_options_for_profile(&profile),
            cancel,
            move |update| progress(update),
        )
        .await?;
        self.inner.profile_store.save_task_status(
            &request.profile_id,
            Some(&request.root),
            IndexTaskPhase::Finished,
            Some("manual build finished"),
        )?;
        Ok(IndexServiceEvent::RebuildFinished(outcome))
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
        Ok(IndexServiceEvent::StatusLoaded(status))
    }

    pub async fn clear_failures(
        &self,
        profile_id: &str,
        root: PathBuf,
    ) -> Result<IndexServiceEvent, IndexError> {
        let profile = self.profile(profile_id)?;
        clear_file_search_index_failures(self.index_dir_for_root(&root)).await?;
        let status = file_search_index_status(
            self.index_dir_for_root(&root),
            &root,
            self.index_options_for_profile(&profile),
        )
        .await?;
        Ok(IndexServiceEvent::FailuresCleared(status))
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
        let status = file_search_index_status(
            self.index_dir_for_root(&root),
            &root,
            self.index_options_for_profile(&profile),
        )
        .await?;
        Ok(IndexServiceEvent::RootRemoved(status))
    }

    pub async fn delete_profile(&self, id: &str) -> Result<IndexServiceEvent, IndexError> {
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
        Ok(IndexServiceEvent::ProfileDeleted(id.to_owned()))
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

    fn index_dir_for_root(&self, root: &Path) -> PathBuf {
        crate::layout::search_index_dir_for_root(&self.inner.index_base_dir, root)
    }

    fn index_options_for_profile(&self, profile: &IndexProfile) -> FileSearchIndexOptions {
        FileSearchIndexOptions {
            include_hidden: profile.include_hidden,
            exclude_patterns: profile.exclude_patterns.clone(),
            directory_error_policy: profile.directory_error_policy,
            excluded_index_dir: Some(self.inner.index_base_dir.clone()),
            media_metadata_scope: profile.media.scope,
        }
    }

    fn query_options_for_profile(
        &self,
        profile: &IndexProfile,
        mode: SearchMode,
        limit: usize,
    ) -> FileSearchOptions {
        FileSearchOptions {
            include_hidden: profile.include_hidden,
            exclude_patterns: profile.exclude_patterns.clone(),
            directory_error_policy: profile.directory_error_policy,
            limit,
            mode,
            media_metadata_scope: profile.media.scope,
        }
    }
}
