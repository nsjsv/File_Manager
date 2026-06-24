use std::path::PathBuf;

use file_index::{IndexProfile, IndexServiceCommand, IndexServiceEvent};
use iced::futures::SinkExt;
use iced::{Subscription, Task};

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

pub(crate) fn search_index_maintenance_pause_command(
    profile_id: String,
    config: UserConfig,
    paused: bool,
    generation: u64,
) -> Task<Message> {
    Task::perform(
        save_search_index_maintenance_pause(profile_id, config, paused),
        move |outcome| Message::SearchIndexMaintenanceUpdated(generation, outcome.map(|_| paused)),
    )
}

pub(crate) fn search_index_maintenance_subscription(
    profile_id: String,
    config: UserConfig,
    generation: u64,
) -> Subscription<Message> {
    Subscription::run_with(
        SearchIndexMaintenanceSubscription {
            profile_id,
            index_base_dir: config.search_index_dir,
            generation,
        },
        search_index_maintenance_stream,
    )
}

pub(crate) fn default_search_index_profile(
    config: &UserConfig,
    roots: Vec<PathBuf>,
) -> IndexProfile {
    let mut profile = IndexProfile::new(DEFAULT_SEARCH_PROFILE_ID, roots);
    profile.include_hidden = config.show_hidden_files;
    profile.exclude_patterns = config.search_index_exclude_patterns.clone();
    profile.directory_error_policy = config.search_index_directory_error_policy;
    profile.content.enabled = config.search_index_content_enabled;
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

async fn save_search_index_maintenance_pause(
    _profile_id: String,
    config: UserConfig,
    paused: bool,
) -> Result<(), String> {
    let command = if paused {
        IndexServiceCommand::Pause
    } else {
        IndexServiceCommand::Resume
    };
    let event =
        search_index_daemon::execute_index_command(config.search_index_dir, command).await?;
    match (paused, event) {
        (true, IndexServiceEvent::Paused) | (false, IndexServiceEvent::Resumed) => Ok(()),
        (_, event) => Err(format!("unexpected search index event: {event:?}")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SearchIndexMaintenanceSubscription {
    profile_id: String,
    index_base_dir: PathBuf,
    generation: u64,
}

fn search_index_maintenance_stream(
    subscription: &SearchIndexMaintenanceSubscription,
) -> impl iced::futures::Stream<Item = Message> + 'static {
    let subscription = subscription.clone();
    iced::stream::channel(64, async move |mut output| {
        let mut events = match search_index_daemon::subscribe_index_maintenance(
            subscription.index_base_dir.clone(),
            subscription.profile_id.clone(),
        )
        .await
        {
            Ok(events) => events,
            Err(error) => {
                let _ = output
                    .send(Message::SearchIndexMaintenanceUpdated(
                        subscription.generation,
                        Err(error.to_string()),
                    ))
                    .await;
                iced::futures::future::pending::<()>().await;
                return;
            }
        };
        let _ = output
            .send(Message::SearchIndexMaintenanceUpdated(
                subscription.generation,
                Ok(false),
            ))
            .await;

        loop {
            match events.next_event().await {
                Ok(Some(event)) if maintenance_event_is_user_visible(&event) => {
                    if output
                        .send(Message::SearchIndexMaintenanceEvent(
                            subscription.generation,
                            event,
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(error) => {
                    let _ = output
                        .send(Message::SearchIndexMaintenanceUpdated(
                            subscription.generation,
                            Err(error.to_string()),
                        ))
                        .await;
                    break;
                }
            }
        }
        iced::futures::future::pending::<()>().await
    })
}

fn maintenance_event_is_user_visible(event: &IndexServiceEvent) -> bool {
    matches!(
        event,
        IndexServiceEvent::WatchStarted { .. }
            | IndexServiceEvent::WatchFailed { .. }
            | IndexServiceEvent::IncrementalUpdateStarted { .. }
            | IndexServiceEvent::IncrementalUpdateFinished { .. }
            | IndexServiceEvent::IncrementalUpdateFailed { .. }
    )
}
