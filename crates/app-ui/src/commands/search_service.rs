use std::num::NonZeroU32;
use std::path::Path;
use std::time::{Duration, Instant};

use file_search::{SearchRuntimeIdentity, SearchServiceStatus};
use iced::Task;

use super::search_service_endpoint::{inspect_search_endpoint, SearchEndpointProbeFailure};
pub(super) use super::search_service_systemd::{
    SearchUnitAction, SearchUnitController, SearchUnitSnapshot, UnitActiveState,
};
use crate::model::{
    Message, SearchServiceDiagnostic, SearchServiceDiagnosticKind, SearchServiceStatusRequest,
};

const SEARCH_SERVICE_READY_TIMEOUT: Duration = Duration::from_secs(30);
const SEARCH_SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(crate) fn ensure_search_service_command(request: SearchServiceStatusRequest) -> Task<Message> {
    Task::perform(ensure_search_service(), move |outcome| {
        Message::SearchServiceEnsured(request, outcome)
    })
}

pub(crate) fn search_service_status_command(request: SearchServiceStatusRequest) -> Task<Message> {
    Task::perform(read_search_service_status(), move |outcome| {
        Message::SearchServiceStatusLoaded(request, outcome)
    })
}

async fn ensure_search_service() -> Result<SearchServiceStatus, SearchServiceDiagnostic> {
    let runtime_identity = configured_runtime_identity().map_err(|error| {
        SearchServiceDiagnostic::new(SearchServiceDiagnosticKind::ServiceUnverified, error)
    })?;
    let unit_controller = SearchUnitController::system(runtime_identity);
    ensure_search_service_with(&unit_controller, &runtime_identity.socket_path())
        .await
        .map_err(|error| {
            SearchServiceDiagnostic::new(SearchServiceDiagnosticKind::ServiceUnverified, error)
        })
}

async fn read_search_service_status() -> Result<SearchServiceStatus, SearchServiceDiagnostic> {
    let runtime_identity = configured_runtime_identity().map_err(|error| {
        SearchServiceDiagnostic::new(SearchServiceDiagnosticKind::ServiceUnverified, error)
    })?;
    let unit_controller = SearchUnitController::system(runtime_identity);
    read_search_service_status_with(&unit_controller, &runtime_identity.socket_path()).await
}

fn configured_runtime_identity() -> Result<SearchRuntimeIdentity, String> {
    SearchRuntimeIdentity::from_environment().map_err(|error| error.to_string())
}

async fn read_search_service_status_with(
    unit_controller: &SearchUnitController,
    socket_path: &Path,
) -> Result<SearchServiceStatus, SearchServiceDiagnostic> {
    Ok(
        read_validated_search_service_with(unit_controller, socket_path)
            .await
            .map_err(ValidatedSearchServiceFailure::into_diagnostic)?
            .status,
    )
}

pub(super) struct ValidatedSearchService {
    pub(super) main_pid: NonZeroU32,
    pub(super) status: SearchServiceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ValidatedSearchServiceFailure {
    OwnerUnverified(String),
    OwnerChanged(String),
    StableOwnerEndpoint {
        main_pid: NonZeroU32,
        endpoint_failure: SearchEndpointProbeFailure,
        unit_description: String,
    },
}

impl ValidatedSearchServiceFailure {
    pub(super) fn into_message(self) -> String {
        match self {
            Self::OwnerUnverified(message) | Self::OwnerChanged(message) => message,
            Self::StableOwnerEndpoint {
                endpoint_failure,
                unit_description,
                ..
            } => format!("{}; {unit_description}", endpoint_failure.into_message()),
        }
    }

    pub(super) fn into_diagnostic(self) -> SearchServiceDiagnostic {
        match self {
            Self::OwnerUnverified(message) => SearchServiceDiagnostic::new(
                SearchServiceDiagnosticKind::ServiceUnverified,
                message,
            ),
            Self::OwnerChanged(message) => {
                SearchServiceDiagnostic::new(SearchServiceDiagnosticKind::ServiceChanging, message)
            }
            Self::StableOwnerEndpoint {
                endpoint_failure,
                unit_description,
                ..
            } => {
                let diagnostic_kind = match &endpoint_failure {
                    SearchEndpointProbeFailure::TimedOut => {
                        SearchServiceDiagnosticKind::EndpointTimedOut
                    }
                    SearchEndpointProbeFailure::TemporarilyUnavailable(_) => {
                        SearchServiceDiagnosticKind::EndpointUnavailable
                    }
                    SearchEndpointProbeFailure::RestartRequired(_) => {
                        SearchServiceDiagnosticKind::EndpointInvalid
                    }
                    SearchEndpointProbeFailure::Incompatible { .. } => {
                        SearchServiceDiagnosticKind::ComponentIncompatible
                    }
                };
                SearchServiceDiagnostic::new(
                    diagnostic_kind,
                    format!("{}; {unit_description}", endpoint_failure.into_message()),
                )
            }
        }
    }
}

impl From<String> for ValidatedSearchServiceFailure {
    fn from(message: String) -> Self {
        Self::OwnerUnverified(message)
    }
}

pub(super) async fn read_validated_search_service_with(
    unit_controller: &SearchUnitController,
    socket_path: &Path,
) -> Result<ValidatedSearchService, ValidatedSearchServiceFailure> {
    let initial_snapshot = unit_controller.show().await?;
    let expected_main_pid = unit_controller
        .validated_main_pid(&initial_snapshot)
        .await?;
    let endpoint_observation = inspect_search_endpoint(socket_path, expected_main_pid).await;
    let confirmed_snapshot = unit_controller.show().await.map_err(|error| {
        ValidatedSearchServiceFailure::OwnerChanged(format!(
            "search service changed while its endpoint was inspected: {error}"
        ))
    })?;
    let confirmed_main_pid = unit_controller
        .validated_main_pid(&confirmed_snapshot)
        .await
        .map_err(|error| {
            ValidatedSearchServiceFailure::OwnerChanged(format!(
                "search service changed while its endpoint was inspected: {error}"
            ))
        })?;
    if confirmed_main_pid != expected_main_pid {
        return Err(ValidatedSearchServiceFailure::OwnerChanged(format!(
            "search service changed while its endpoint was inspected: {}",
            confirmed_snapshot.description()
        )));
    }
    let service_status = match endpoint_observation {
        Ok(service_status) => service_status,
        Err(endpoint_failure) => {
            return Err(ValidatedSearchServiceFailure::StableOwnerEndpoint {
                main_pid: expected_main_pid,
                endpoint_failure,
                unit_description: confirmed_snapshot.description(),
            });
        }
    };
    Ok(ValidatedSearchService {
        main_pid: expected_main_pid,
        status: service_status,
    })
}

async fn ensure_search_service_with(
    unit_controller: &SearchUnitController,
    socket_path: &Path,
) -> Result<SearchServiceStatus, String> {
    let initial_snapshot = unit_controller.show().await?;
    let mut restart_issued = false;
    match (&initial_snapshot.active_state, initial_snapshot.main_pid) {
        (UnitActiveState::Active, Some(_)) | (UnitActiveState::Activating, _) => {}
        (UnitActiveState::Active, None) => {
            unit_controller
                .reload_then_execute(SearchUnitAction::Restart)
                .await
                .map_err(|error| format!("{error}; {}", initial_snapshot.description()))?;
            restart_issued = true;
        }
        (
            UnitActiveState::Inactive | UnitActiveState::Failed | UnitActiveState::Deactivating,
            _,
        ) => {
            unit_controller
                .reload_then_execute(SearchUnitAction::Start)
                .await
                .map_err(|error| format!("{error}; {}", initial_snapshot.description()))?;
        }
        (UnitActiveState::Other(active_state), _) => {
            return Err(format!(
                "search service unit has unsupported ActiveState={active_state}: {}",
                initial_snapshot.description()
            ));
        }
    }

    let readiness_deadline = Instant::now() + SEARCH_SERVICE_READY_TIMEOUT;
    loop {
        let current_snapshot = unit_controller.show().await?;
        let last_observation = match (&current_snapshot.active_state, current_snapshot.main_pid) {
            (UnitActiveState::Active, Some(expected_main_pid)) => {
                let validated_main_pid = unit_controller
                    .validated_main_pid(&current_snapshot)
                    .await?;
                debug_assert_eq!(validated_main_pid, expected_main_pid);
                match inspect_search_endpoint(socket_path, expected_main_pid).await {
                    Ok(service_status) => {
                        let confirmed_snapshot = unit_controller.show().await?;
                        if unit_controller
                            .validated_main_pid(&confirmed_snapshot)
                            .await
                            .is_ok_and(|confirmed_main_pid| confirmed_main_pid == expected_main_pid)
                        {
                            return Ok(service_status);
                        }
                        format!(
                            "service owner changed during endpoint inspection: {}",
                            confirmed_snapshot.description()
                        )
                    }
                    Err(
                        probe_failure @ (SearchEndpointProbeFailure::RestartRequired(_)
                        | SearchEndpointProbeFailure::Incompatible { .. }),
                    ) if !restart_issued => {
                        let message = probe_failure.into_message();
                        let confirmed_snapshot = unit_controller.show().await?;
                        if unit_controller
                            .validated_main_pid(&confirmed_snapshot)
                            .await
                            .is_ok_and(|confirmed_main_pid| confirmed_main_pid == expected_main_pid)
                        {
                            unit_controller
                                .reload_then_execute(SearchUnitAction::Restart)
                                .await
                                .map_err(|error| {
                                    format!("{error}; {}", confirmed_snapshot.description())
                                })?;
                            restart_issued = true;
                            format!("{message}; {}", confirmed_snapshot.description())
                        } else {
                            format!(
                                "service owner changed before restart: {}",
                                confirmed_snapshot.description()
                            )
                        }
                    }
                    Err(probe_failure) => format!(
                        "{}; {}",
                        probe_failure.into_message(),
                        current_snapshot.description()
                    ),
                }
            }
            (UnitActiveState::Active, None) => current_snapshot.description(),
            (UnitActiveState::Activating | UnitActiveState::Deactivating, _) => {
                current_snapshot.description()
            }
            (UnitActiveState::Inactive | UnitActiveState::Failed, _) => {
                return Err(format!(
                    "search service stopped before endpoint readiness: {}",
                    current_snapshot.description()
                ));
            }
            (UnitActiveState::Other(active_state), _) => {
                return Err(format!(
                    "search service unit has unsupported ActiveState={active_state}: {}",
                    current_snapshot.description()
                ));
            }
        };

        if Instant::now() >= readiness_deadline {
            return Err(format!(
                "search service did not become ready within {} seconds; last observation: {last_observation}",
                SEARCH_SERVICE_READY_TIMEOUT.as_secs()
            ));
        }
        tokio::time::sleep(SEARCH_SERVICE_POLL_INTERVAL).await;
    }
}

#[cfg(test)]
#[path = "search_service_tests.rs"]
mod tests;
