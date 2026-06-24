use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use std::{future::Future, io};

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
const INDEX_DAEMON_EXECUTABLE_NAME: &str = "file-indexd";
const DAEMON_START_SETTLE_DELAY: Duration = Duration::from_millis(150);

#[derive(Clone, Copy)]
enum IndexDaemonServiceAction {
    Start,
    Restart,
    Stop,
}

impl IndexDaemonServiceAction {
    fn as_systemctl_arg(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Restart => "restart",
            Self::Stop => "stop",
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
    ensure_index_daemon_started(index_base_dir.clone()).await?;
    let client = IndexClient::for_index_base_dir(index_base_dir);
    let socket_path = client.socket_path().to_path_buf();
    let command_for_retry = command.clone();
    let first_result = client.execute(command.clone()).await;
    run_with_protocol_retry(first_result, None, socket_path, || {
        let client = IndexClient::for_index_base_dir(client.index_base_dir().to_path_buf());
        let command = command_for_retry.clone();
        async move { client.execute(command).await }
    })
    .await
}

pub(super) async fn subscribe_index_maintenance(
    index_base_dir: PathBuf,
    profile_id: String,
) -> Result<IndexMaintenanceSubscription, String> {
    ensure_index_daemon_started(index_base_dir.clone()).await?;
    let client = IndexClient::for_index_base_dir(index_base_dir);
    client
        .subscribe_maintenance(profile_id)
        .await
        .map_err(|error| index_daemon_error_message(&error, None))
}

pub(super) async fn build_selected_paths_with_progress(
    index_base_dir: PathBuf,
    request: BuildSelectedPathsRequest,
    cancel: CancellationToken,
    progress: impl FnMut(FileSearchIndexProgress) + Send,
) -> Result<IndexServiceEvent, String> {
    ensure_index_daemon_started(index_base_dir.clone()).await?;
    let client = IndexClient::for_index_base_dir(index_base_dir);
    client
        .build_selected_paths_with_progress_and_cancel(request, cancel, progress)
        .await
        .map_err(|error| index_daemon_error_message(&error, None))
}

async fn ensure_index_daemon_started(index_base_dir: PathBuf) -> Result<(), String> {
    let client = IndexClient::for_index_base_dir(index_base_dir);
    match probe_index_daemon_client(&client).await {
        Ok(()) => Ok(()),
        Err(error) if matches!(error, IndexClientError::Connect { .. }) => {
            recover_index_daemon(&client, error, IndexDaemonServiceAction::Start).await
        }
        Err(error) if index_daemon_error_requires_restart(&error) => {
            recover_index_daemon(&client, error, IndexDaemonServiceAction::Restart).await
        }
        Err(error) => Err(error.to_string()),
    }
}

async fn restart_index_daemon(index_base_dir: PathBuf) -> Result<SearchIndexDaemonStatus, String> {
    let restart_error = run_index_daemon_service_action(IndexDaemonServiceAction::Restart)
        .await
        .err();
    let client = IndexClient::for_index_base_dir(index_base_dir);
    match probe_index_daemon_client(&client).await {
        Ok(()) => Ok(SearchIndexDaemonStatus::Reachable),
        Err(error) if index_daemon_error_allows_local_launch(&error) => {
            start_local_index_daemon(client.socket_path()).await?;
            load_index_daemon_status(client.index_base_dir().to_path_buf()).await
        }
        Err(error) => Ok(SearchIndexDaemonStatus::Unreachable(
            index_daemon_error_message(&error, restart_error.as_deref()),
        )),
    }
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
    probe_index_daemon_client(&client)
        .await
        .map_err(|error| error.to_string())
}

async fn probe_index_daemon_client(client: &IndexClient) -> Result<(), IndexClientError> {
    match client.execute(IndexServiceCommand::Ping).await {
        Ok(IndexServiceEvent::Pong) => Ok(()),
        Ok(event) => Err(IndexClientError::Protocol(format!(
            "unexpected search index event: {event:?}"
        ))),
        Err(error) => Err(error),
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

async fn start_local_index_daemon(socket_path: &Path) -> Result<(), String> {
    let executable = local_index_daemon_executable()?;
    let _ = run_index_daemon_service_action(IndexDaemonServiceAction::Stop).await;
    Command::new(&executable)
        .arg(socket_path.as_os_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not start local index daemon {executable:?}: {error}"))?;
    tokio::time::sleep(DAEMON_START_SETTLE_DELAY).await;
    Ok(())
}

async fn recover_index_daemon(
    client: &IndexClient,
    first_error: IndexClientError,
    service_action: IndexDaemonServiceAction,
) -> Result<(), String> {
    let mut recovery_errors = vec![first_error.to_string()];
    match run_index_daemon_service_action(service_action).await {
        Ok(()) => match probe_index_daemon_client(client).await {
            Ok(()) => return Ok(()),
            Err(error) if index_daemon_error_allows_local_launch(&error) => {
                recovery_errors.push(error.to_string());
            }
            Err(error) => return Err(error.to_string()),
        },
        Err(error) => recovery_errors.push(error),
    }

    if let Err(error) = start_local_index_daemon(client.socket_path()).await {
        recovery_errors.push(error);
        return Err(recovery_errors.join("; "));
    }
    match probe_index_daemon_client(client).await {
        Ok(()) => Ok(()),
        Err(error) => {
            recovery_errors.push(error.to_string());
            Err(recovery_errors.join("; "))
        }
    }
}

fn local_index_daemon_executable() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("could not locate current executable: {error}"))?;
    let Some(executable) = sibling_index_daemon_executable(&current_exe) else {
        return Err("current executable has no parent directory".to_owned());
    };
    if !executable.is_file() {
        return Err(format!(
            "could not find matching {INDEX_DAEMON_EXECUTABLE_NAME} next to current executable at {executable:?}"
        ));
    }
    Ok(executable)
}

fn sibling_index_daemon_executable(current_exe: &Path) -> Option<PathBuf> {
    current_exe
        .parent()
        .map(|directory| directory.join(INDEX_DAEMON_EXECUTABLE_NAME))
}

async fn run_with_protocol_retry<T, Retry, RetryFuture>(
    first_result: Result<T, IndexClientError>,
    start_error: Option<&str>,
    socket_path: PathBuf,
    mut retry: Retry,
) -> Result<T, String>
where
    Retry: FnMut() -> RetryFuture,
    RetryFuture: Future<Output = Result<T, IndexClientError>>,
{
    match first_result {
        Ok(value) => Ok(value),
        Err(error) if index_daemon_error_allows_local_launch(&error) => {
            let mut recovery_errors = vec![index_daemon_error_message(&error, start_error)];
            if index_daemon_error_requires_restart(&error) {
                match run_index_daemon_service_action(IndexDaemonServiceAction::Restart).await {
                    Ok(()) => match retry().await {
                        Ok(value) => return Ok(value),
                        Err(error) if index_daemon_error_allows_local_launch(&error) => {
                            recovery_errors.push(index_daemon_error_message(&error, None));
                        }
                        Err(error) => return Err(index_daemon_error_message(&error, None)),
                    },
                    Err(error) => recovery_errors.push(error),
                }
            }
            if let Err(error) = start_local_index_daemon(&socket_path).await {
                recovery_errors.push(error);
                return Err(recovery_errors.join("; "));
            }
            retry()
                .await
                .map_err(|error| index_daemon_error_message(&error, None))
        }
        Err(error) => Err(index_daemon_error_message(&error, start_error)),
    }
}

fn index_daemon_error_message(error: &IndexClientError, start_error: Option<&str>) -> String {
    match start_error {
        Some(start_error) if matches!(error, IndexClientError::Connect { .. }) => {
            format!("{error}; {start_error}")
        }
        _ => error.to_string(),
    }
}

fn index_daemon_error_requires_restart(error: &IndexClientError) -> bool {
    match error {
        IndexClientError::ProtocolMismatch { .. } | IndexClientError::Codec(_) => true,
        IndexClientError::Io(error) => matches!(
            error.kind(),
            io::ErrorKind::UnexpectedEof
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::BrokenPipe
        ),
        _ => false,
    }
}

fn index_daemon_error_allows_local_launch(error: &IndexClientError) -> bool {
    matches!(error, IndexClientError::Connect { .. }) || index_daemon_error_requires_restart(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_drift_errors_request_daemon_restart() {
        assert!(index_daemon_error_requires_restart(
            &IndexClientError::ProtocolMismatch {
                expected: 2,
                actual: 1
            }
        ));
        assert!(index_daemon_error_requires_restart(&IndexClientError::Io(
            io::Error::new(io::ErrorKind::UnexpectedEof, "old daemon closed stream")
        )));
        assert!(!index_daemon_error_requires_restart(
            &IndexClientError::Service("missing profile default".to_owned())
        ));
    }

    #[test]
    fn local_daemon_candidate_uses_current_executable_directory() {
        let current_exe = PathBuf::from("/tmp/file-manager/app-ui");

        assert_eq!(
            sibling_index_daemon_executable(&current_exe),
            Some(PathBuf::from("/tmp/file-manager/file-indexd"))
        );
    }

    #[test]
    fn daemon_connect_errors_can_use_local_launch() {
        assert!(index_daemon_error_allows_local_launch(
            &IndexClientError::Connect {
                path: PathBuf::from("/tmp/file-indexd.sock"),
                source: io::Error::new(io::ErrorKind::NotFound, "missing socket"),
            }
        ));
        assert!(!index_daemon_error_allows_local_launch(
            &IndexClientError::Service("missing profile default".to_owned())
        ));
    }

    #[test]
    fn protocol_mismatch_can_use_local_daemon_fallback() {
        assert!(index_daemon_error_allows_local_launch(
            &IndexClientError::ProtocolMismatch {
                expected: 2,
                actual: 1,
            }
        ));
    }
}
