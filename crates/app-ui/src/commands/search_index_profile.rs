use std::path::PathBuf;

use file_index::{IndexProfile, IndexServiceCommand, IndexServiceEvent};
use iced::Task;

use super::search_index_daemon;
use crate::config::UserConfig;
use crate::model::{Message, SearchIndexProfileSaveReason};

const DEFAULT_SEARCH_PROFILE_ID: &str = "default";

pub(crate) fn search_index_profile_load_command(config: UserConfig) -> Task<Message> {
    Task::perform(load_search_index_profile(config), |outcome| {
        Message::SearchIndexProfileLoaded(outcome)
    })
}

pub(crate) fn search_index_profile_save_command(
    profile: IndexProfile,
    config: UserConfig,
    reason: SearchIndexProfileSaveReason,
) -> Task<Message> {
    Task::perform(save_search_index_profile(profile, config), move |outcome| {
        Message::SearchIndexProfileSaved(reason, outcome)
    })
}

pub(crate) fn search_index_profile_delete_command(
    profile_id: String,
    config: UserConfig,
) -> Task<Message> {
    Task::perform(delete_search_index_profile(profile_id, config), |outcome| {
        Message::SearchIndexProfileDeleted(outcome)
    })
}

pub(crate) fn default_search_index_profile(
    config: &UserConfig,
    roots: Vec<PathBuf>,
) -> IndexProfile {
    let mut profile = IndexProfile::new(DEFAULT_SEARCH_PROFILE_ID, roots);
    profile.include_hidden = config.show_hidden_files;
    profile.exclude_patterns = config.search_index_exclude_patterns.clone();
    profile.directory_error_policy = config.search_index_directory_error_policy;
    profile.media.scope = config.search_index_media_scope;
    profile
}

pub(crate) fn default_search_profile_id() -> &'static str {
    DEFAULT_SEARCH_PROFILE_ID
}

async fn load_search_index_profile(config: UserConfig) -> Result<Option<IndexProfile>, String> {
    let profile_id = default_search_profile_id().to_owned();
    match search_index_daemon::execute_index_command(
        config.search_index_dir,
        IndexServiceCommand::LoadProfile(profile_id),
    )
    .await?
    {
        IndexServiceEvent::ProfileLoaded(profile) => Ok(profile),
        event => Err(format!("unexpected search index event: {event:?}")),
    }
}

async fn save_search_index_profile(
    profile: IndexProfile,
    config: UserConfig,
) -> Result<IndexProfile, String> {
    let saved_profile = profile.clone();
    match search_index_daemon::execute_index_command(
        config.search_index_dir,
        IndexServiceCommand::ConfigureProfile(profile),
    )
    .await?
    {
        IndexServiceEvent::ProfileConfigured(_) => Ok(saved_profile),
        event => Err(format!("unexpected search index event: {event:?}")),
    }
}

async fn delete_search_index_profile(
    profile_id: String,
    config: UserConfig,
) -> Result<String, String> {
    let deleted_id = profile_id.clone();
    match search_index_daemon::execute_index_command(
        config.search_index_dir,
        IndexServiceCommand::DeleteProfile(profile_id),
    )
    .await?
    {
        IndexServiceEvent::ProfileDeleted(_) => Ok(deleted_id),
        event => Err(format!("unexpected search index event: {event:?}")),
    }
}
