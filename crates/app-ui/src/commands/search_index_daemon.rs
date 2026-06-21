use std::path::PathBuf;
use std::time::Duration;

use file_index::{
    BuildSelectedPathsRequest, FileSearchIndexProgress, IndexClient, IndexClientError,
    IndexMaintenanceSubscription, IndexServiceCommand, IndexServiceEvent,
};
use iced::Task;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::config::UserConfig;
use crate::model::{Message, SearchIndexDaemonStatus};

const SEARCH_INDEX_SERVICE_UNIT: &str = "file-manager-index.service";
const DAEMON_START_SETTLE_DELAY: Duration = Duration::from_millis(150);
const DAEMON_STATUS_PROBE_PROFILE_ID: &str = "__daemon_status_probe__";

#[derive(Clone, Copy)]
enum IndexDaemonServiceAction {
    Start,
    Restart,
}

impl IndexDaemonServiceAction {
    fn as_systemctl_arg(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Restart => "restart",
        }
    }
}

pub(crate) fn search_index_daemon_status_command(config: UserConfig) -> Task<Message> {
    Task::perform(
        load_index_daemon_status(config.search_index_dir),
        |outcome| Message::SearchIndexDaemonStatusLoaded(outcome),
    )
}

pub(crate) fn search_index_daemon_restart_command(config: UserConfig) -> Task<Message> {
    Task::perform(restart_index_daemon(config.search_index_dir), |outcome| {
        Message::SearchIndexDaemonRestarted(outcome)
    })
}

pub(super) async fn execute_index_command(
    index_base_dir: PathBuf,
    command: IndexServiceCommand,
) -> Result<IndexServiceEvent, String> {
    let start_error = start_index_daemon_service().await.err();
    let client = IndexClient::for_index_base_dir(index_base_dir);
    client
        .execute(command)
        .await
        .map_err(|error| index_daemon_error_message(error, start_error.as_deref()))
}

pub(super) async fn subscribe_index_maintenance(
    index_base_dir: PathBuf,
    profile_id: String,
) -> Result<IndexMaintenanceSubscription, String> {
    let start_error = start_index_daemon_service().await.err();
    let client = IndexClient::for_index_base_dir(index_base_dir);
    client
        .subscribe_maintenance(profile_id)
        .await
        .map_err(|error| index_daemon_error_message(error, start_error.as_deref()))
}

pub(super) async fn build_selected_paths_with_progress(
    index_base_dir: PathBuf,
    request: BuildSelectedPathsRequest,
    cancel: CancellationToken,
    progress: impl FnMut(FileSearchIndexProgress) + Send,
) -> Result<IndexServiceEvent, String> {
    let start_error = start_index_daemon_service().await.err();
    let client = IndexClient::for_index_base_dir(index_base_dir);
    client
        .build_selected_paths_with_progress_and_cancel(request, cancel, progress)
        .await
        .map_err(|error| index_daemon_error_message(error, start_error.as_deref()))
}

async fn start_index_daemon_service() -> Result<(), String> {
    run_index_daemon_service_action(IndexDaemonServiceAction::Start).await
}

async fn restart_index_daemon(index_base_dir: PathBuf) -> Result<SearchIndexDaemonStatus, String> {
    run_index_daemon_service_action(IndexDaemonServiceAction::Restart).await?;
    load_index_daemon_status(index_base_dir).await
}

async fn load_index_daemon_status(
    index_base_dir: PathBuf,
) -> Result<SearchIndexDaemonStatus, String> {
    match probe_index_daemon(index_base_dir).await {
        Ok(()) => Ok(SearchIndexDaemonStatus::Reachable),
        Err(error) => Ok(SearchIndexDaemonStatus::Unreachable(error)),
    }
}

async fn probe_index_daemon(index_base_dir: PathBuf) -> Result<(), String> {
    let client = IndexClient::for_index_base_dir(index_base_dir);
    match client
        .execute(IndexServiceCommand::LoadProfile(
            DAEMON_STATUS_PROBE_PROFILE_ID.to_owned(),
        ))
        .await
    {
        Ok(IndexServiceEvent::ProfileLoaded(_)) => Ok(()),
        Ok(event) => Err(format!("unexpected search index event: {event:?}")),
        Err(error) => Err(error.to_string()),
    }
}

async fn run_index_daemon_service_action(action: IndexDaemonServiceAction) -> Result<(), String> {
    let output = Command::new("systemctl")
        .args([
            "--user",
            action.as_systemctl_arg(),
            SEARCH_INDEX_SERVICE_UNIT,
        ])
        .output()
        .await
        .map_err(|error| {
            format!(
                "could not {} index daemon service: {error}",
                action.as_systemctl_arg()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!(
                "could not {} index daemon service: systemctl exited with {}",
                action.as_systemctl_arg(),
                output.status
            )
        } else {
            format!(
                "could not {} index daemon service: {stderr}",
                action.as_systemctl_arg()
            )
        });
    }
    tokio::time::sleep(DAEMON_START_SETTLE_DELAY).await;
    Ok(())
}

fn index_daemon_error_message(error: IndexClientError, start_error: Option<&str>) -> String {
    match start_error {
        Some(start_error) if matches!(error, IndexClientError::Connect { .. }) => {
            format!("{error}; {start_error}")
        }
        _ => error.to_string(),
    }
}
