use std::fmt;
use std::num::NonZeroU32;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use file_search::{default_socket_path, SearchServiceStatus};
use iced::Task;
use tokio::process::Command;

use super::search_service_endpoint::{inspect_search_endpoint, SearchEndpointProbeFailure};
use crate::model::Message;

const SEARCH_SERVICE_UNIT: &str = "file-manager-search.service";
const SEARCH_CONTROL_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(1);
const SEARCH_SERVICE_READY_TIMEOUT: Duration = Duration::from_secs(30);
const SEARCH_SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const REQUIRED_MEMORY_HIGH: u64 = 80_000_000;
const REQUIRED_MEMORY_MAX: u64 = 96_000_000;
const REQUIRED_MEMORY_SWAP_MAX: u64 = 0;
const REQUIRED_CPU_QUOTA_PERCENT: u64 = 5;
const MAXIMUM_SUPPORTED_BASE_PAGE_BYTES: u64 = 65_536;

pub(crate) fn ensure_search_service_command() -> Task<Message> {
    Task::perform(ensure_search_service(), Message::SearchServiceEnsured)
}

pub(crate) fn search_service_status_command() -> Task<Message> {
    Task::perform(
        read_search_service_status(),
        Message::SearchServiceStatusLoaded,
    )
}

async fn ensure_search_service() -> Result<SearchServiceStatus, String> {
    let unit_controller = SearchUnitController::system();
    ensure_search_service_with(&unit_controller, &default_socket_path()).await
}

async fn read_search_service_status() -> Result<SearchServiceStatus, String> {
    let unit_controller = SearchUnitController::system();
    read_search_service_status_with(&unit_controller, &default_socket_path()).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchUnitAction {
    Show,
    DaemonReload,
    Start,
    Restart,
    KillControlGroup,
    ResetFailed,
}

impl SearchUnitAction {
    fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::Show => &[
                "--user",
                "--no-pager",
                "--property=ActiveState",
                "--property=SubState",
                "--property=MainPID",
                "--property=ControlGroup",
                "--property=MemoryHigh",
                "--property=MemoryMax",
                "--property=MemorySwapMax",
                "--property=Result",
                "--property=ExecMainStatus",
                "--property=NRestarts",
                "show",
                SEARCH_SERVICE_UNIT,
            ],
            Self::DaemonReload => &["--user", "--no-pager", "daemon-reload"],
            Self::Start => &[
                "--user",
                "--no-pager",
                "--no-block",
                "start",
                SEARCH_SERVICE_UNIT,
            ],
            Self::Restart => &[
                "--user",
                "--no-pager",
                "--no-block",
                "restart",
                SEARCH_SERVICE_UNIT,
            ],
            Self::KillControlGroup => &[
                "--user",
                "--no-pager",
                "--signal=SIGKILL",
                "--kill-whom=all",
                "kill",
                SEARCH_SERVICE_UNIT,
            ],
            Self::ResetFailed => &["--user", "--no-pager", "reset-failed", SEARCH_SERVICE_UNIT],
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::DaemonReload => "daemon-reload",
            Self::Start => "start",
            Self::Restart => "restart",
            Self::KillControlGroup => "kill",
            Self::ResetFailed => "reset-failed",
        }
    }
}

pub(super) struct SearchUnitController {
    systemctl_executable: PathBuf,
    cgroup_root: PathBuf,
}

impl SearchUnitController {
    pub(super) fn system() -> Self {
        Self {
            systemctl_executable: PathBuf::from("systemctl"),
            cgroup_root: PathBuf::from("/sys/fs/cgroup"),
        }
    }

    pub(super) async fn execute(&self, action: SearchUnitAction) -> Result<String, String> {
        let mut command = Command::new(&self.systemctl_executable);
        command.args(action.arguments()).kill_on_drop(true);
        let command_output = tokio::time::timeout(SEARCH_CONTROL_ATTEMPT_TIMEOUT, command.output())
            .await
            .map_err(|_| {
                format!(
                    "systemctl --user {} {} timed out",
                    action.name(),
                    SEARCH_SERVICE_UNIT
                )
            })?
            .map_err(|error| {
                format!(
                    "could not execute systemctl --user {} {}: {error}",
                    action.name(),
                    SEARCH_SERVICE_UNIT
                )
            })?;

        if !command_output.status.success() {
            let stderr = String::from_utf8_lossy(&command_output.stderr)
                .trim()
                .to_owned();
            let detail = if stderr.is_empty() {
                command_output.status.to_string()
            } else {
                stderr
            };
            return Err(format!(
                "systemctl --user {} {} failed: {detail}",
                action.name(),
                SEARCH_SERVICE_UNIT
            ));
        }

        Ok(String::from_utf8_lossy(&command_output.stdout).into_owned())
    }

    pub(super) async fn show(&self) -> Result<SearchUnitSnapshot, String> {
        SearchUnitSnapshot::parse(&self.execute(SearchUnitAction::Show).await?)
    }

    pub(super) async fn reload_then_execute(&self, action: SearchUnitAction) -> Result<(), String> {
        self.execute(SearchUnitAction::DaemonReload).await?;
        self.execute(action).await?;
        Ok(())
    }

    async fn validate_effective_cgroup(&self, snapshot: &SearchUnitSnapshot) -> Result<(), String> {
        let relative_control_group = snapshot
            .control_group
            .as_ref()
            .expect("ready service snapshot has a ControlGroup")
            .strip_prefix("/")
            .expect("validated ControlGroup is absolute");
        let cgroup_directory = self.cgroup_root.join(relative_control_group);

        for (file_name, required_value) in [
            ("memory.high", REQUIRED_MEMORY_HIGH),
            ("memory.max", REQUIRED_MEMORY_MAX),
        ] {
            let limit_path = cgroup_directory.join(file_name);
            let limit_text = read_effective_cgroup_setting(&limit_path, snapshot).await?;
            let effective_value = limit_text.trim().parse::<u64>().map_err(|error| {
                format!(
                    "effective cgroup limit {} is invalid: {error}; {}",
                    limit_path.display(),
                    snapshot.description()
                )
            })?;
            // Linux memcg may round limits down to the base-page boundary. Accept only a
            // small downward adjustment so the kernel can never loosen the configured limit.
            let rounding_difference = required_value.saturating_sub(effective_value);
            let is_safe_kernel_rounding = effective_value <= required_value
                && rounding_difference < MAXIMUM_SUPPORTED_BASE_PAGE_BYTES;
            if !is_safe_kernel_rounding {
                return Err(format!(
                    "effective cgroup limit {}={} is not a safe page-rounded value for {}; {}",
                    limit_path.display(),
                    effective_value,
                    required_value,
                    snapshot.description()
                ));
            }
        }

        let swap_limit_path = cgroup_directory.join("memory.swap.max");
        let swap_limit_text = read_effective_cgroup_setting(&swap_limit_path, snapshot).await?;
        let effective_swap_limit = swap_limit_text.trim().parse::<u64>().map_err(|error| {
            format!(
                "effective cgroup limit {} is invalid: {error}; {}",
                swap_limit_path.display(),
                snapshot.description()
            )
        })?;
        if effective_swap_limit != REQUIRED_MEMORY_SWAP_MAX {
            return Err(format!(
                "effective cgroup limit {}={} does not equal required value {}; {}",
                swap_limit_path.display(),
                effective_swap_limit,
                REQUIRED_MEMORY_SWAP_MAX,
                snapshot.description()
            ));
        }

        let cpu_max_path = cgroup_directory.join("cpu.max");
        let cpu_max_text = read_effective_cgroup_setting(&cpu_max_path, snapshot).await?;
        let cpu_max_fields = cpu_max_text.split_whitespace().collect::<Vec<_>>();
        let [cpu_quota_text, cpu_period_text] = cpu_max_fields.as_slice() else {
            return Err(format!(
                "effective cgroup limit {} must contain quota and period; {}",
                cpu_max_path.display(),
                snapshot.description()
            ));
        };
        let cpu_quota = cpu_quota_text.parse::<u64>().map_err(|error| {
            format!(
                "effective cgroup quota {} is invalid: {error}; {}",
                cpu_max_path.display(),
                snapshot.description()
            )
        })?;
        let cpu_period = cpu_period_text.parse::<u64>().map_err(|error| {
            format!(
                "effective cgroup period {} is invalid: {error}; {}",
                cpu_max_path.display(),
                snapshot.description()
            )
        })?;
        if cpu_period == 0
            || u128::from(cpu_quota) * 100
                > u128::from(cpu_period) * u128::from(REQUIRED_CPU_QUOTA_PERCENT)
        {
            return Err(format!(
                "effective cgroup limit {}={} {} exceeds {}%; {}",
                cpu_max_path.display(),
                cpu_quota,
                cpu_period,
                REQUIRED_CPU_QUOTA_PERCENT,
                snapshot.description()
            ));
        }

        Ok(())
    }

    async fn validated_main_pid(
        &self,
        snapshot: &SearchUnitSnapshot,
    ) -> Result<NonZeroU32, String> {
        let main_pid = snapshot.ready_main_pid()?;
        self.validate_effective_cgroup(snapshot).await?;
        Ok(main_pid)
    }
}

async fn read_effective_cgroup_setting(
    setting_path: &Path,
    snapshot: &SearchUnitSnapshot,
) -> Result<String, String> {
    tokio::fs::read_to_string(setting_path)
        .await
        .map_err(|error| {
            format!(
                "could not read effective cgroup limit {}: {error}; {}",
                setting_path.display(),
                snapshot.description()
            )
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UnitActiveState {
    Active,
    Activating,
    Inactive,
    Failed,
    Deactivating,
    Other(String),
}

impl UnitActiveState {
    fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty() {
            return Err("systemd reported empty ActiveState".to_owned());
        }
        Ok(match value {
            "active" => Self::Active,
            "activating" => Self::Activating,
            "inactive" => Self::Inactive,
            "failed" => Self::Failed,
            "deactivating" => Self::Deactivating,
            value => Self::Other(value.to_owned()),
        })
    }
}

impl fmt::Display for UnitActiveState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => formatter.write_str("active"),
            Self::Activating => formatter.write_str("activating"),
            Self::Inactive => formatter.write_str("inactive"),
            Self::Failed => formatter.write_str("failed"),
            Self::Deactivating => formatter.write_str("deactivating"),
            Self::Other(value) => formatter.write_str(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchUnitSnapshot {
    active_state: UnitActiveState,
    sub_state: String,
    main_pid: Option<NonZeroU32>,
    control_group: Option<PathBuf>,
    memory_high: u64,
    memory_max: u64,
    memory_swap_max: u64,
    service_result: String,
    exec_main_status: u32,
    restart_count: u64,
}

impl SearchUnitSnapshot {
    fn parse(command_stdout: &str) -> Result<Self, String> {
        let mut active_state = None;
        let mut sub_state = None;
        let mut main_pid = None;
        let mut main_pid_observed = false;
        let mut control_group = None;
        let mut memory_high = None;
        let mut memory_max = None;
        let mut memory_swap_max = None;
        let mut service_result = None;
        let mut exec_main_status = None;
        let mut restart_count = None;

        for property_line in command_stdout.lines() {
            let Some((property_name, property_value)) = property_line.split_once('=') else {
                return Err(format!(
                    "systemctl show returned malformed property line: {property_line}"
                ));
            };
            let property_name = property_name.trim();
            let property_value = property_value.trim();
            match property_name {
                "ActiveState" => {
                    if active_state
                        .replace(UnitActiveState::parse(property_value)?)
                        .is_some()
                    {
                        return Err("systemctl show repeated ActiveState".to_owned());
                    }
                }
                "SubState" => {
                    if property_value.is_empty() {
                        return Err("systemd reported empty SubState".to_owned());
                    }
                    if sub_state.replace(property_value.to_owned()).is_some() {
                        return Err("systemctl show repeated SubState".to_owned());
                    }
                }
                "MainPID" => {
                    let parsed_pid = property_value
                        .parse::<u32>()
                        .map_err(|error| format!("systemd reported invalid MainPID: {error}"))?;
                    if main_pid_observed {
                        return Err("systemctl show repeated MainPID".to_owned());
                    }
                    main_pid = NonZeroU32::new(parsed_pid);
                    main_pid_observed = true;
                }
                "ControlGroup" => {
                    let parsed_control_group = Self::parse_control_group(property_value)?;
                    if control_group.replace(parsed_control_group).is_some() {
                        return Err("systemctl show repeated ControlGroup".to_owned());
                    }
                }
                "MemoryHigh" => {
                    let parsed_value = property_value
                        .parse::<u64>()
                        .map_err(|error| format!("systemd reported invalid MemoryHigh: {error}"))?;
                    if memory_high.replace(parsed_value).is_some() {
                        return Err("systemctl show repeated MemoryHigh".to_owned());
                    }
                }
                "MemoryMax" => {
                    let parsed_value = property_value
                        .parse::<u64>()
                        .map_err(|error| format!("systemd reported invalid MemoryMax: {error}"))?;
                    if memory_max.replace(parsed_value).is_some() {
                        return Err("systemctl show repeated MemoryMax".to_owned());
                    }
                }
                "MemorySwapMax" => {
                    let parsed_value = property_value.parse::<u64>().map_err(|error| {
                        format!("systemd reported invalid MemorySwapMax: {error}")
                    })?;
                    if memory_swap_max.replace(parsed_value).is_some() {
                        return Err("systemctl show repeated MemorySwapMax".to_owned());
                    }
                }
                "Result" => {
                    if property_value.is_empty() {
                        return Err("systemd reported empty Result".to_owned());
                    }
                    if service_result.replace(property_value.to_owned()).is_some() {
                        return Err("systemctl show repeated Result".to_owned());
                    }
                }
                "ExecMainStatus" => {
                    let parsed_value = property_value.parse::<u32>().map_err(|error| {
                        format!("systemd reported invalid ExecMainStatus: {error}")
                    })?;
                    if exec_main_status.replace(parsed_value).is_some() {
                        return Err("systemctl show repeated ExecMainStatus".to_owned());
                    }
                }
                "NRestarts" => {
                    let parsed_value = property_value
                        .parse::<u64>()
                        .map_err(|error| format!("systemd reported invalid NRestarts: {error}"))?;
                    if restart_count.replace(parsed_value).is_some() {
                        return Err("systemctl show repeated NRestarts".to_owned());
                    }
                }
                _ => {}
            }
        }

        let active_state =
            active_state.ok_or_else(|| "systemctl show omitted ActiveState".to_owned())?;
        let sub_state = sub_state.ok_or_else(|| "systemctl show omitted SubState".to_owned())?;
        if !main_pid_observed {
            return Err("systemctl show omitted MainPID".to_owned());
        }
        let control_group =
            control_group.ok_or_else(|| "systemctl show omitted ControlGroup".to_owned())?;
        let memory_high =
            memory_high.ok_or_else(|| "systemctl show omitted MemoryHigh".to_owned())?;
        let memory_max = memory_max.ok_or_else(|| "systemctl show omitted MemoryMax".to_owned())?;
        let memory_swap_max =
            memory_swap_max.ok_or_else(|| "systemctl show omitted MemorySwapMax".to_owned())?;
        let service_result =
            service_result.ok_or_else(|| "systemctl show omitted Result".to_owned())?;
        let exec_main_status =
            exec_main_status.ok_or_else(|| "systemctl show omitted ExecMainStatus".to_owned())?;
        let restart_count =
            restart_count.ok_or_else(|| "systemctl show omitted NRestarts".to_owned())?;
        Ok(Self {
            active_state,
            sub_state,
            main_pid,
            control_group,
            memory_high,
            memory_max,
            memory_swap_max,
            service_result,
            exec_main_status,
            restart_count,
        })
    }

    fn parse_control_group(property_value: &str) -> Result<Option<PathBuf>, String> {
        if property_value.is_empty() {
            return Ok(None);
        }
        let control_group = PathBuf::from(property_value);
        let mut components = control_group.components();
        let has_absolute_root = components.next() == Some(Component::RootDir);
        let has_safe_first_segment = matches!(components.next(), Some(Component::Normal(_)));
        let remaining_segments_are_safe =
            components.all(|component| matches!(component, Component::Normal(_)));
        if !has_absolute_root || !has_safe_first_segment || !remaining_segments_are_safe {
            return Err(format!(
                "systemd reported unsafe ControlGroup={property_value}"
            ));
        }
        Ok(Some(control_group))
    }

    fn ready_main_pid(&self) -> Result<NonZeroU32, String> {
        if self.active_state != UnitActiveState::Active || self.sub_state != "running" {
            return Err(format!(
                "search service unit is not active/running: {}",
                self.description()
            ));
        }
        if self.control_group.is_none() {
            return Err(format!(
                "active/running search service unit reported an empty ControlGroup: {}",
                self.description()
            ));
        }
        if self.memory_high != REQUIRED_MEMORY_HIGH
            || self.memory_max != REQUIRED_MEMORY_MAX
            || self.memory_swap_max != REQUIRED_MEMORY_SWAP_MAX
        {
            return Err(format!(
                "search service unit effective memory properties do not match the required envelope: {}",
                self.description()
            ));
        }
        if self.service_result != "success" || self.exec_main_status != 0 || self.restart_count != 0
        {
            return Err(format!(
                "search service unit has failed or restarted: {}",
                self.description()
            ));
        }
        self.main_pid.ok_or_else(|| {
            format!(
                "active/running search service unit reported MainPID=0: {}",
                self.description()
            )
        })
    }

    pub(super) fn main_pid(&self) -> Option<NonZeroU32> {
        self.main_pid
    }

    pub(super) fn may_have_processes(&self) -> bool {
        self.main_pid.is_some()
            || !matches!(
                &self.active_state,
                UnitActiveState::Inactive | UnitActiveState::Failed
            )
    }

    pub(super) fn description(&self) -> String {
        let control_group = self
            .control_group
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_owned());
        format!(
            "ActiveState={}, SubState={}, MainPID={}, ControlGroup={}, MemoryHigh={}, MemoryMax={}, MemorySwapMax={}, Result={}, ExecMainStatus={}, NRestarts={}",
            self.active_state,
            self.sub_state,
            self.main_pid.map_or(0, NonZeroU32::get),
            control_group,
            self.memory_high,
            self.memory_max,
            self.memory_swap_max,
            self.service_result,
            self.exec_main_status,
            self.restart_count
        )
    }
}

async fn read_search_service_status_with(
    unit_controller: &SearchUnitController,
    socket_path: &Path,
) -> Result<SearchServiceStatus, String> {
    Ok(
        read_validated_search_service_with(unit_controller, socket_path)
            .await
            .map_err(ValidatedSearchServiceFailure::into_message)?
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
    StableOwnerEndpoint {
        main_pid: NonZeroU32,
        endpoint_failure: SearchEndpointProbeFailure,
        unit_description: String,
    },
}

impl ValidatedSearchServiceFailure {
    pub(super) fn into_message(self) -> String {
        match self {
            Self::OwnerUnverified(message) => message,
            Self::StableOwnerEndpoint {
                endpoint_failure,
                unit_description,
                ..
            } => format!("{}; {unit_description}", endpoint_failure.into_message()),
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
        ValidatedSearchServiceFailure::OwnerUnverified(format!(
            "search service changed while its endpoint was inspected: {error}"
        ))
    })?;
    let confirmed_main_pid = unit_controller
        .validated_main_pid(&confirmed_snapshot)
        .await
        .map_err(|error| {
            ValidatedSearchServiceFailure::OwnerUnverified(format!(
                "search service changed while its endpoint was inspected: {error}"
            ))
        })?;
    if confirmed_main_pid != expected_main_pid {
        return Err(ValidatedSearchServiceFailure::OwnerUnverified(format!(
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
