use std::path::{Path, PathBuf};

use file_index::{IndexProfile, IndexService, IndexServiceCommand, IndexServiceEvent};
use iced::futures::SinkExt;
use iced::{Subscription, Task};

use crate::config::UserConfig;
use crate::model::Message;

const SEARCH_INDEX_CONTROL_DB: &str = "control.sqlite";
const DEFAULT_SEARCH_PROFILE_ID: &str = "default";

pub(crate) fn search_index_profile_load_command(config: UserConfig) -> Task<Message> {
    Task::perform(load_search_index_profile(config), |outcome| {
        Message::SearchIndexProfileLoaded(outcome)
    })
}

pub(crate) fn search_index_profile_save_command(
    profile: IndexProfile,
    config: UserConfig,
) -> Task<Message> {
    Task::perform(save_search_index_profile(profile, config), |outcome| {
        Message::SearchIndexProfileSaved(outcome)
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
            control_db_path: search_index_control_db_path(&config.search_index_dir),
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
    profile.media.enabled = config.search_index_media_enabled;
    profile
}

pub(crate) fn search_index_control_db_path(base_dir: &Path) -> PathBuf {
    base_dir.join(SEARCH_INDEX_CONTROL_DB)
}

pub(crate) fn default_search_profile_id() -> &'static str {
    DEFAULT_SEARCH_PROFILE_ID
}

async fn load_search_index_profile(config: UserConfig) -> Result<Option<IndexProfile>, String> {
    let profile_id = default_search_profile_id().to_owned();
    let service = search_index_service(&config)?;
    match service
        .load_profile(&profile_id)
        .map_err(|error| error.to_string())?
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
    let service = search_index_service(&config)?;
    match service
        .configure_profile(profile)
        .map_err(|error| error.to_string())?
    {
        IndexServiceEvent::ProfileConfigured(_) => Ok(saved_profile),
        event => Err(format!("unexpected search index event: {event:?}")),
    }
}

async fn delete_search_index_profile(
    profile_id: String,
    config: UserConfig,
) -> Result<String, String> {
    let control_db = search_index_control_db_path(&config.search_index_dir);
    let index_base_dir = config.search_index_dir.clone();
    let deleted_id = profile_id.clone();
    let service =
        IndexService::open(control_db, index_base_dir).map_err(|error| error.to_string())?;
    service
        .delete_profile(&profile_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(deleted_id)
}

async fn save_search_index_maintenance_pause(
    _profile_id: String,
    config: UserConfig,
    paused: bool,
) -> Result<(), String> {
    let service = search_index_service(&config)?;
    let command = if paused {
        IndexServiceCommand::Pause
    } else {
        IndexServiceCommand::Resume
    };
    let event = service
        .execute(command)
        .await
        .map_err(|error| error.to_string())?;
    match (paused, event) {
        (true, IndexServiceEvent::Paused) | (false, IndexServiceEvent::Resumed) => Ok(()),
        (_, event) => Err(format!("unexpected search index event: {event:?}")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SearchIndexMaintenanceSubscription {
    profile_id: String,
    control_db_path: PathBuf,
    index_base_dir: PathBuf,
    generation: u64,
}

fn search_index_maintenance_stream(
    subscription: &SearchIndexMaintenanceSubscription,
) -> impl iced::futures::Stream<Item = Message> + 'static {
    let subscription = subscription.clone();
    iced::stream::channel(64, async move |mut output| {
        let service = match IndexService::open(
            subscription.control_db_path,
            subscription.index_base_dir.clone(),
        ) {
            Ok(service) => service,
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
        let mut events = service.status_stream();
        let _maintenance = service.maintain_profile(subscription.profile_id.clone());
        let _ = output
            .send(Message::SearchIndexMaintenanceUpdated(
                subscription.generation,
                Ok(false),
            ))
            .await;

        while let Ok(event) = events.recv().await {
            if maintenance_event_is_user_visible(&event)
                && output
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

fn search_index_service(config: &UserConfig) -> Result<IndexService, String> {
    IndexService::open(
        search_index_control_db_path(&config.search_index_dir),
        config.search_index_dir.clone(),
    )
    .map_err(|error| error.to_string())
}
