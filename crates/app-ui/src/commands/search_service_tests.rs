use std::num::NonZeroU32;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use file_search::{
    daemon_build_id, read_service_request, write_service_event, IndexedQueryAvailability,
    SearchRuntimeIdentity, SearchServiceEvent, SearchServicePhase, SearchServiceRequest,
    SearchServiceStatus, PROTOCOL_VERSION,
};
use tempfile::tempdir;
use tokio::net::UnixListener;

use super::{
    inspect_search_endpoint, SearchEndpointProbeFailure, SearchUnitAction, SearchUnitController,
    SearchUnitSnapshot, UnitActiveState, ValidatedSearchServiceFailure,
};
use crate::model::{SearchServiceDiagnosticKind, SearchServiceRecoveryAction};

fn valid_snapshot_text() -> &'static str {
    "NRestarts=0\nMemorySwapMax=0\nSubState=running\nResult=success\nControlGroup=/user.slice/search.service\nMemoryMax=96000000\nActiveState=active\nExecMainStatus=0\nMainPID=42\nMemoryHigh=80000000\nFragmentPath=/home/test/.config/systemd/user/file-manager-search.service\nDropInPaths=/home/test/.config/systemd/user/file-manager-search.service.d/override.conf\nExecStart={ path=/home/test/.local/share/file-manager-dev/file-searchd ; argv[]=/home/test/.local/share/file-manager-dev/file-searchd ; }\n"
}

async fn create_valid_search_cgroup(cgroup_root: &Path) -> PathBuf {
    let cgroup_directory = cgroup_root.join("user.slice/search.service");
    tokio::fs::create_dir_all(&cgroup_directory).await.unwrap();
    tokio::fs::write(cgroup_directory.join("memory.high"), "80000000\n")
        .await
        .unwrap();
    tokio::fs::write(cgroup_directory.join("memory.max"), "96000000\n")
        .await
        .unwrap();
    tokio::fs::write(cgroup_directory.join("memory.swap.max"), "0\n")
        .await
        .unwrap();
    tokio::fs::write(cgroup_directory.join("cpu.max"), "5000 100000\n")
        .await
        .unwrap();
    cgroup_directory
}

async fn create_sequenced_recovery_systemctl(
    temporary_directory: &Path,
    initial_snapshot: &str,
    replacement_snapshot: &str,
) -> (PathBuf, PathBuf) {
    let systemctl_log_path = temporary_directory.join("recovery-systemctl.log");
    let show_count_path = temporary_directory.join("recovery-show-count");
    let systemctl_path = temporary_directory.join("recovery-systemctl");
    let systemctl_script = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>\"{}\"\nif [[ \"$*\" == *\" show file-manager-search.service\" ]]; then\n    if [[ -f \"{}\" ]]; then\n        read -r show_count <\"{}\"\n    else\n        show_count=0\n    fi\n    if (( show_count == 0 )); then\n        printf '%s' '{}'\n    else\n        printf '%s' '{}'\n    fi\n    printf '%s' \"$((show_count + 1))\" >\"{}\"\nfi\n",
        systemctl_log_path.display(),
        show_count_path.display(),
        show_count_path.display(),
        initial_snapshot,
        replacement_snapshot,
        show_count_path.display(),
    );
    tokio::fs::write(&systemctl_path, systemctl_script)
        .await
        .unwrap();
    let mut permissions = std::fs::metadata(&systemctl_path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&systemctl_path, permissions).unwrap();
    (systemctl_path, systemctl_log_path)
}

fn spawn_compatible_endpoint(
    listener: UnixListener,
    status: SearchServiceStatus,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert_eq!(
            read_service_request(&mut stream).await.unwrap(),
            SearchServiceRequest::Version
        );
        write_service_event(
            &mut stream,
            &SearchServiceEvent::Version {
                protocol: PROTOCOL_VERSION,
                build: daemon_build_id(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            read_service_request(&mut stream).await.unwrap(),
            SearchServiceRequest::Status
        );
        write_service_event(&mut stream, &SearchServiceEvent::Status(status))
            .await
            .unwrap();
    })
}

#[test]
fn unit_snapshot_parses_properties_without_order_dependency() {
    let snapshot =
        SearchUnitSnapshot::parse(valid_snapshot_text(), SearchRuntimeIdentity::Release).unwrap();

    assert_eq!(snapshot.active_state, UnitActiveState::Active);
    assert_eq!(snapshot.sub_state, "running");
    assert_eq!(snapshot.main_pid, NonZeroU32::new(42));
    assert_eq!(
        snapshot.control_group,
        Some(PathBuf::from("/user.slice/search.service"))
    );
    assert_eq!(snapshot.memory_high, 80_000_000);
    assert_eq!(snapshot.memory_max, 96_000_000);
    assert_eq!(snapshot.memory_swap_max, 0);
    assert_eq!(snapshot.service_result, "success");
    assert_eq!(snapshot.exec_main_status, 0);
    assert_eq!(snapshot.restart_count, 0);
    assert!(snapshot
        .description()
        .contains("FragmentPath=/home/test/.config/systemd/user/file-manager-search.service"));
    assert!(snapshot
        .description()
        .contains("ExecStartPath=/home/test/.local/share/file-manager-dev/file-searchd"));
    assert!(snapshot.description().contains("unit source guidance"));
}

#[test]
fn endpoint_timeout_maps_to_a_stable_user_diagnostic_category() {
    let diagnostic = ValidatedSearchServiceFailure::StableOwnerEndpoint {
        main_pid: NonZeroU32::new(42).unwrap(),
        endpoint_failure: SearchEndpointProbeFailure::TimedOut,
        unit_description: "Unit=file-manager-search.service".to_owned(),
    }
    .into_diagnostic();

    assert_eq!(
        diagnostic.kind,
        SearchServiceDiagnosticKind::EndpointTimedOut
    );
    assert!(diagnostic
        .technical_detail
        .contains("endpoint inspection timed out"));
    assert!(diagnostic
        .technical_detail
        .contains("Unit=file-manager-search.service"));
}

#[test]
fn unit_snapshot_reports_only_the_exec_start_executable_path() {
    let snapshot_text = valid_snapshot_text().replace(
        "argv[]=/home/test/.local/share/file-manager-dev/file-searchd ;",
        "argv[]=/home/test/.local/share/file-manager-dev/file-searchd --api-key very-secret ;",
    );

    let snapshot =
        SearchUnitSnapshot::parse(&snapshot_text, SearchRuntimeIdentity::Release).unwrap();
    let description = snapshot.description();

    assert!(
        description.contains("ExecStartPath=/home/test/.local/share/file-manager-dev/file-searchd")
    );
    assert!(!description.contains("--api-key"));
    assert!(!description.contains("very-secret"));
}

#[test]
fn unit_snapshot_rejects_failed_or_restarted_service_as_ready() {
    for failed_property in ["Result=exit-code", "ExecMainStatus=1", "NRestarts=1"] {
        let property_name = failed_property.split_once('=').unwrap().0;
        let snapshot_text = valid_snapshot_text()
            .lines()
            .map(|line| {
                if line.starts_with(&format!("{property_name}=")) {
                    failed_property
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let snapshot =
            SearchUnitSnapshot::parse(&snapshot_text, SearchRuntimeIdentity::Release).unwrap();

        assert!(snapshot.ready_main_pid().is_err());
    }
}

#[test]
fn unit_snapshot_rejects_missing_and_invalid_properties() {
    for property_name in [
        "ActiveState",
        "SubState",
        "MainPID",
        "ControlGroup",
        "MemoryHigh",
        "MemoryMax",
        "MemorySwapMax",
        "Result",
        "ExecMainStatus",
        "NRestarts",
        "FragmentPath",
        "DropInPaths",
        "ExecStart",
    ] {
        let snapshot_without_property = valid_snapshot_text()
            .lines()
            .filter(|line| !line.starts_with(&format!("{property_name}=")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            SearchUnitSnapshot::parse(&snapshot_without_property, SearchRuntimeIdentity::Release)
                .is_err(),
            "missing {property_name} must fail closed"
        );
    }

    for invalid_property in [
        "ActiveState=",
        "SubState=",
        "MainPID=invalid",
        "ControlGroup=../search.service",
        "ControlGroup=/user.slice/../search.service",
        "MemoryHigh=infinity",
        "MemoryMax=max",
        "MemorySwapMax=-1",
        "Result=",
        "ExecMainStatus=invalid",
        "NRestarts=invalid",
    ] {
        let property_name = invalid_property.split_once('=').unwrap().0;
        let invalid_snapshot = valid_snapshot_text()
            .lines()
            .map(|line| {
                if line.starts_with(&format!("{property_name}=")) {
                    invalid_property
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            SearchUnitSnapshot::parse(&invalid_snapshot, SearchRuntimeIdentity::Release).is_err(),
            "{invalid_property} must fail closed"
        );
    }

    let control_character_snapshot = valid_snapshot_text().replace(
        "DropInPaths=/home/test/.config/systemd/user/file-manager-search.service.d/override.conf",
        "DropInPaths=/home/test/.config/systemd/user/file-manager-search.service.d/\toverride.conf",
    );
    assert!(
        SearchUnitSnapshot::parse(&control_character_snapshot, SearchRuntimeIdentity::Release)
            .is_err()
    );

    let oversized_exec_start = format!(
        "{{ path=/tmp/file-searchd ; argv[]={}; }}",
        "x".repeat(20_000)
    );
    let oversized_snapshot = valid_snapshot_text().replace(
        "{ path=/home/test/.local/share/file-manager-dev/file-searchd ; argv[]=/home/test/.local/share/file-manager-dev/file-searchd ; }",
        &oversized_exec_start,
    );
    assert!(
        SearchUnitSnapshot::parse(&oversized_snapshot, SearchRuntimeIdentity::Release).is_err()
    );
}

#[tokio::test]
async fn systemctl_output_is_drained_with_a_hard_size_limit() {
    let temporary_directory = tempdir().unwrap();
    let systemctl_path = temporary_directory.path().join("oversized-systemctl");
    tokio::fs::write(
        &systemctl_path,
        "#!/usr/bin/env bash\nprintf '%070000d' 0\n",
    )
    .await
    .unwrap();
    let mut permissions = std::fs::metadata(&systemctl_path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&systemctl_path, permissions).unwrap();
    let unit_controller = SearchUnitController {
        runtime_identity: SearchRuntimeIdentity::Release,
        systemctl_executable: systemctl_path,
        cgroup_root: temporary_directory.path().to_path_buf(),
    };

    let error = unit_controller
        .execute(SearchUnitAction::Show)
        .await
        .expect_err("oversized stdout must be rejected");

    assert!(error.contains("stdout exceeded"));
}

#[test]
fn inactive_snapshot_allows_an_empty_control_group_for_recovery() {
    let snapshot_text = valid_snapshot_text()
        .replace("ActiveState=active", "ActiveState=inactive")
        .replace("SubState=running", "SubState=dead")
        .replace("MainPID=42", "MainPID=0")
        .replace("ControlGroup=/user.slice/search.service", "ControlGroup=");

    let snapshot =
        SearchUnitSnapshot::parse(&snapshot_text, SearchRuntimeIdentity::Release).unwrap();

    assert_eq!(snapshot.control_group, None);
    assert!(!snapshot.may_have_processes());
    assert!(snapshot.ready_main_pid().is_err());
}

#[test]
fn unit_actions_use_user_systemd_without_a_shell() {
    let release_identity = SearchRuntimeIdentity::Release;
    let development_identity = SearchRuntimeIdentity::Development;
    let show_arguments = SearchUnitAction::Show.arguments(release_identity);
    assert!(show_arguments.contains(&"--property=FragmentPath".into()));
    assert!(show_arguments.contains(&"--property=DropInPaths".into()));
    assert!(show_arguments.contains(&"--property=ExecStart".into()));
    assert_eq!(
        show_arguments.last().unwrap(),
        release_identity.systemd_unit()
    );
    assert_eq!(
        SearchUnitAction::DaemonReload.arguments(release_identity),
        ["--user", "--no-pager", "daemon-reload"]
    );
    assert_eq!(
        SearchUnitAction::Start.arguments(development_identity),
        [
            "--user",
            "--no-pager",
            "--no-block",
            "start",
            "file-manager-search-dev.service"
        ]
    );
    assert_eq!(
        SearchUnitAction::Restart.arguments(release_identity),
        [
            "--user",
            "--no-pager",
            "--no-block",
            "restart",
            "file-manager-search.service"
        ]
    );
    assert_eq!(
        SearchUnitAction::KillControlGroup.arguments(release_identity),
        [
            "--user",
            "--no-pager",
            "--signal=SIGKILL",
            "--kill-whom=all",
            "kill",
            "file-manager-search.service"
        ]
    );
    assert_eq!(
        SearchUnitAction::ResetFailed.arguments(release_identity),
        [
            "--user",
            "--no-pager",
            "reset-failed",
            "file-manager-search.service"
        ]
    );
}

#[tokio::test]
async fn effective_cgroup_accepts_only_safe_kernel_rounding() {
    let temporary_directory = tempdir().unwrap();
    let cgroup_directory = create_valid_search_cgroup(temporary_directory.path()).await;
    let unit_controller = SearchUnitController {
        runtime_identity: SearchRuntimeIdentity::Release,
        systemctl_executable: PathBuf::from("unused-systemctl"),
        cgroup_root: temporary_directory.path().to_path_buf(),
    };
    let snapshot =
        SearchUnitSnapshot::parse(valid_snapshot_text(), SearchRuntimeIdentity::Release).unwrap();

    assert_eq!(
        unit_controller.validated_main_pid(&snapshot).await.unwrap(),
        NonZeroU32::new(42).unwrap()
    );

    tokio::fs::write(cgroup_directory.join("memory.high"), "79998976\n")
        .await
        .unwrap();
    tokio::fs::write(cgroup_directory.join("memory.max"), "95997952\n")
        .await
        .unwrap();
    assert_eq!(
        unit_controller.validated_main_pid(&snapshot).await.unwrap(),
        NonZeroU32::new(42).unwrap()
    );

    tokio::fs::write(cgroup_directory.join("memory.max"), "100000000\n")
        .await
        .unwrap();
    assert!(unit_controller.validated_main_pid(&snapshot).await.is_err());

    tokio::fs::write(cgroup_directory.join("memory.high"), "79934464\n")
        .await
        .unwrap();
    assert!(unit_controller.validated_main_pid(&snapshot).await.is_err());

    tokio::fs::write(cgroup_directory.join("memory.high"), "80000000\n")
        .await
        .unwrap();
    tokio::fs::write(cgroup_directory.join("memory.max"), "95934464\n")
        .await
        .unwrap();
    assert!(unit_controller.validated_main_pid(&snapshot).await.is_err());

    tokio::fs::write(cgroup_directory.join("memory.max"), "max\n")
        .await
        .unwrap();
    assert!(unit_controller.validated_main_pid(&snapshot).await.is_err());

    tokio::fs::write(cgroup_directory.join("memory.max"), "96000000\n")
        .await
        .unwrap();
    tokio::fs::write(cgroup_directory.join("memory.swap.max"), "1\n")
        .await
        .unwrap();
    assert!(unit_controller.validated_main_pid(&snapshot).await.is_err());

    tokio::fs::write(cgroup_directory.join("memory.swap.max"), "0\n")
        .await
        .unwrap();

    tokio::fs::write(cgroup_directory.join("cpu.max"), "6000 100000\n")
        .await
        .unwrap();
    assert!(unit_controller.validated_main_pid(&snapshot).await.is_err());

    tokio::fs::write(cgroup_directory.join("cpu.max"), "max 100000\n")
        .await
        .unwrap();
    assert!(unit_controller.validated_main_pid(&snapshot).await.is_err());

    tokio::fs::write(cgroup_directory.join("cpu.max"), "5000 100000\n")
        .await
        .unwrap();

    tokio::fs::remove_file(cgroup_directory.join("memory.max"))
        .await
        .unwrap();
    assert!(unit_controller.validated_main_pid(&snapshot).await.is_err());
}

#[tokio::test]
async fn endpoint_probe_uses_one_connection_for_owner_version_and_status() {
    let temporary_directory = tempdir().unwrap();
    let socket_path = temporary_directory.path().join("search.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let expected_status = SearchServiceStatus {
        phase: SearchServicePhase::Ready,
        query_availability: IndexedQueryAvailability::Available,
        index_status: None,
    };
    let served_status = expected_status.clone();
    let endpoint_server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert_eq!(
            read_service_request(&mut stream).await.unwrap(),
            SearchServiceRequest::Version
        );
        write_service_event(
            &mut stream,
            &SearchServiceEvent::Version {
                protocol: PROTOCOL_VERSION,
                build: daemon_build_id(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            read_service_request(&mut stream).await.unwrap(),
            SearchServiceRequest::Status
        );
        write_service_event(&mut stream, &SearchServiceEvent::Status(served_status))
            .await
            .unwrap();
    });

    let inspected_status =
        inspect_search_endpoint(&socket_path, NonZeroU32::new(std::process::id()).unwrap())
            .await
            .map_err(|failure| failure.into_message())
            .unwrap();

    assert_eq!(inspected_status, expected_status);
    endpoint_server.await.unwrap();
}

#[tokio::test]
async fn endpoint_probe_reports_expected_and_actual_identity() {
    let temporary_directory = tempdir().unwrap();
    let socket_path = temporary_directory.path().join("search.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let actual_protocol = PROTOCOL_VERSION - 1;
    let actual_build = "installed-old-build".to_owned();
    let served_build = actual_build.clone();
    let endpoint_server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert_eq!(
            read_service_request(&mut stream).await.unwrap(),
            SearchServiceRequest::Version
        );
        write_service_event(
            &mut stream,
            &SearchServiceEvent::Version {
                protocol: actual_protocol,
                build: served_build,
            },
        )
        .await
        .unwrap();
    });

    let message =
        inspect_search_endpoint(&socket_path, NonZeroU32::new(std::process::id()).unwrap())
            .await
            .unwrap_err()
            .into_message();

    assert_eq!(
        message,
        format!(
            "search service endpoint is incompatible: expected_protocol={}, actual_protocol={actual_protocol}, expected_build={}, actual_build={actual_build}",
            PROTOCOL_VERSION,
            daemon_build_id()
        )
    );
    endpoint_server.await.unwrap();
}

#[tokio::test]
async fn incompatible_endpoint_is_restarted_only_once_before_compatibility() {
    let temporary_directory = tempdir().unwrap();
    let cgroup_root = temporary_directory.path().join("cgroup");
    create_valid_search_cgroup(&cgroup_root).await;
    let systemctl_log_path = temporary_directory.path().join("systemctl.log");
    let systemctl_path = temporary_directory.path().join("systemctl");
    let snapshot_text =
        valid_snapshot_text().replace("MainPID=42", &format!("MainPID={}", std::process::id()));
    let systemctl_script = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>\"{}\"\nif [[ \"$*\" == *\" show file-manager-search.service\" ]]; then\n    printf '%s' '{}'\nfi\n",
        systemctl_log_path.display(),
        snapshot_text
    );
    tokio::fs::write(&systemctl_path, systemctl_script)
        .await
        .unwrap();
    let mut systemctl_permissions = std::fs::metadata(&systemctl_path).unwrap().permissions();
    systemctl_permissions.set_mode(0o755);
    std::fs::set_permissions(&systemctl_path, systemctl_permissions).unwrap();

    let socket_path = temporary_directory.path().join("search.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let expected_status = SearchServiceStatus {
        phase: SearchServicePhase::Ready,
        query_availability: IndexedQueryAvailability::Available,
        index_status: None,
    };
    let served_status = expected_status.clone();
    let endpoint_server = tokio::spawn(async move {
        for connection_index in 0..3 {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(
                read_service_request(&mut stream).await.unwrap(),
                SearchServiceRequest::Version
            );
            let compatible = connection_index == 2;
            write_service_event(
                &mut stream,
                &SearchServiceEvent::Version {
                    protocol: if compatible {
                        PROTOCOL_VERSION
                    } else {
                        PROTOCOL_VERSION - 1
                    },
                    build: if compatible {
                        daemon_build_id()
                    } else {
                        "installed-old-build".to_owned()
                    },
                },
            )
            .await
            .unwrap();
            if compatible {
                assert_eq!(
                    read_service_request(&mut stream).await.unwrap(),
                    SearchServiceRequest::Status
                );
                write_service_event(
                    &mut stream,
                    &SearchServiceEvent::Status(served_status.clone()),
                )
                .await
                .unwrap();
            }
        }
    });
    let unit_controller = SearchUnitController {
        runtime_identity: SearchRuntimeIdentity::Release,
        systemctl_executable: systemctl_path,
        cgroup_root,
    };

    let service_status = super::ensure_search_service_with(&unit_controller, &socket_path)
        .await
        .unwrap();

    assert_eq!(service_status, expected_status);
    endpoint_server.await.unwrap();
    let systemctl_log = tokio::fs::read_to_string(systemctl_log_path).await.unwrap();
    assert_eq!(
        systemctl_log
            .lines()
            .filter(|line| line.ends_with("restart file-manager-search.service"))
            .count(),
        1
    );
}

#[tokio::test]
async fn graceful_recovery_retries_a_transient_disconnect_without_sending_sigkill() {
    let temporary_directory = tempdir().unwrap();
    let cgroup_root = temporary_directory.path().join("cgroup");
    create_valid_search_cgroup(&cgroup_root).await;
    let replacement_pid = std::process::id();
    let initial_pid = if replacement_pid == 42 { 43 } else { 42 };
    let initial_snapshot =
        valid_snapshot_text().replace("MainPID=42", &format!("MainPID={initial_pid}"));
    let replacement_snapshot =
        valid_snapshot_text().replace("MainPID=42", &format!("MainPID={replacement_pid}"));
    let (systemctl_executable, systemctl_log_path) = create_sequenced_recovery_systemctl(
        temporary_directory.path(),
        &initial_snapshot,
        &replacement_snapshot,
    )
    .await;
    let socket_path = temporary_directory.path().join("recovery.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let expected_status = SearchServiceStatus {
        phase: SearchServicePhase::Ready,
        query_availability: IndexedQueryAvailability::Available,
        index_status: None,
    };
    let served_status = expected_status.clone();
    let endpoint_server = tokio::spawn(async move {
        let (first_stream, _) = listener.accept().await.unwrap();
        drop(first_stream);
        spawn_compatible_endpoint(listener, served_status)
            .await
            .unwrap();
    });
    let unit_controller = SearchUnitController {
        runtime_identity: SearchRuntimeIdentity::Release,
        systemctl_executable,
        cgroup_root,
    };

    let status = super::super::search_service_recovery::recover_search_service_with(
        &unit_controller,
        &socket_path,
        SearchServiceRecoveryAction::Restart,
    )
    .await
    .unwrap();

    assert_eq!(status, expected_status);
    endpoint_server.await.unwrap();
    let systemctl_log = tokio::fs::read_to_string(systemctl_log_path).await.unwrap();
    let mutation_actions = systemctl_log
        .lines()
        .filter(|line| !line.ends_with("show file-manager-search.service"))
        .collect::<Vec<_>>();
    assert_eq!(
        mutation_actions,
        [
            "--user --no-pager daemon-reload",
            "--user --no-pager --no-block restart file-manager-search.service",
        ]
    );
}

#[tokio::test]
async fn force_recovery_kills_the_whole_control_group_before_restart() {
    let temporary_directory = tempdir().unwrap();
    let cgroup_root = temporary_directory.path().join("cgroup");
    create_valid_search_cgroup(&cgroup_root).await;
    let replacement_pid = std::process::id();
    let initial_pid = if replacement_pid == 42 { 43 } else { 42 };
    let initial_snapshot =
        valid_snapshot_text().replace("MainPID=42", &format!("MainPID={initial_pid}"));
    let replacement_snapshot =
        valid_snapshot_text().replace("MainPID=42", &format!("MainPID={replacement_pid}"));
    let (systemctl_executable, systemctl_log_path) = create_sequenced_recovery_systemctl(
        temporary_directory.path(),
        &initial_snapshot,
        &replacement_snapshot,
    )
    .await;
    let socket_path = temporary_directory.path().join("force-recovery.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let expected_status = SearchServiceStatus {
        phase: SearchServicePhase::Ready,
        query_availability: IndexedQueryAvailability::Available,
        index_status: None,
    };
    let endpoint_server = spawn_compatible_endpoint(listener, expected_status.clone());
    let unit_controller = SearchUnitController {
        runtime_identity: SearchRuntimeIdentity::Release,
        systemctl_executable,
        cgroup_root,
    };

    let status = super::super::search_service_recovery::recover_search_service_with(
        &unit_controller,
        &socket_path,
        SearchServiceRecoveryAction::ForceRestart,
    )
    .await
    .unwrap();

    assert_eq!(status, expected_status);
    endpoint_server.await.unwrap();
    let systemctl_log = tokio::fs::read_to_string(systemctl_log_path).await.unwrap();
    let mutation_actions = systemctl_log
        .lines()
        .filter(|line| !line.ends_with("show file-manager-search.service"))
        .collect::<Vec<_>>();
    assert_eq!(
        mutation_actions,
        [
            "--user --no-pager --signal=SIGKILL --kill-whom=all kill file-manager-search.service",
            "--user --no-pager reset-failed file-manager-search.service",
            "--user --no-pager daemon-reload",
            "--user --no-pager --no-block restart file-manager-search.service",
        ]
    );
}

#[tokio::test]
async fn force_recovery_stops_after_a_systemctl_kill_failure() {
    let temporary_directory = tempdir().unwrap();
    let systemctl_log_path = temporary_directory.path().join("failed-systemctl.log");
    let systemctl_path = temporary_directory.path().join("failed-systemctl");
    let systemctl_script = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>\"{}\"\nif [[ \"$*\" == *\" show file-manager-search.service\" ]]; then\n    printf '%s' '{}'\nelif [[ \"$*\" == *\" kill file-manager-search.service\" ]]; then\n    printf '%s\\n' 'permission denied' >&2\n    exit 17\nfi\n",
        systemctl_log_path.display(),
        valid_snapshot_text(),
    );
    tokio::fs::write(&systemctl_path, systemctl_script)
        .await
        .unwrap();
    let mut permissions = std::fs::metadata(&systemctl_path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&systemctl_path, permissions).unwrap();
    let unit_controller = SearchUnitController {
        runtime_identity: SearchRuntimeIdentity::Release,
        systemctl_executable: systemctl_path,
        cgroup_root: temporary_directory.path().join("unused-cgroup"),
    };

    let error = super::super::search_service_recovery::recover_search_service_with(
        &unit_controller,
        &temporary_directory.path().join("unused.sock"),
        SearchServiceRecoveryAction::ForceRestart,
    )
    .await
    .unwrap_err();

    assert_eq!(error.kind, SearchServiceDiagnosticKind::RecoveryFailed);
    assert!(error
        .technical_detail
        .contains("systemctl --user kill file-manager-search.service failed"));
    assert!(error.technical_detail.contains("permission denied"));
    assert!(error.technical_detail.contains("before recovery:"));
    let systemctl_log = tokio::fs::read_to_string(systemctl_log_path).await.unwrap();
    assert_eq!(
        systemctl_log.lines().collect::<Vec<_>>(),
        [
            "--user --no-pager --property=ActiveState --property=SubState --property=MainPID --property=ControlGroup --property=MemoryHigh --property=MemoryMax --property=MemorySwapMax --property=Result --property=ExecMainStatus --property=NRestarts --property=FragmentPath --property=DropInPaths --property=ExecStart show file-manager-search.service",
            "--user --no-pager --signal=SIGKILL --kill-whom=all kill file-manager-search.service",
        ]
    );
}

#[tokio::test]
async fn recovery_immediately_reports_an_incompatible_replacement_endpoint() {
    let temporary_directory = tempdir().unwrap();
    let cgroup_root = temporary_directory.path().join("cgroup");
    create_valid_search_cgroup(&cgroup_root).await;
    let replacement_pid = std::process::id();
    let initial_pid = if replacement_pid == 42 { 43 } else { 42 };
    let initial_snapshot =
        valid_snapshot_text().replace("MainPID=42", &format!("MainPID={initial_pid}"));
    let replacement_snapshot =
        valid_snapshot_text().replace("MainPID=42", &format!("MainPID={replacement_pid}"));
    let (systemctl_executable, _) = create_sequenced_recovery_systemctl(
        temporary_directory.path(),
        &initial_snapshot,
        &replacement_snapshot,
    )
    .await;
    let socket_path = temporary_directory
        .path()
        .join("incompatible-recovery.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let endpoint_server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert_eq!(
            read_service_request(&mut stream).await.unwrap(),
            SearchServiceRequest::Version
        );
        write_service_event(
            &mut stream,
            &SearchServiceEvent::Version {
                protocol: PROTOCOL_VERSION - 1,
                build: daemon_build_id(),
            },
        )
        .await
        .unwrap();
    });
    let unit_controller = SearchUnitController {
        runtime_identity: SearchRuntimeIdentity::Release,
        systemctl_executable,
        cgroup_root,
    };

    let error = tokio::time::timeout(
        Duration::from_secs(1),
        super::super::search_service_recovery::recover_search_service_with(
            &unit_controller,
            &socket_path,
            SearchServiceRecoveryAction::Restart,
        ),
    )
    .await
    .expect("a stable incompatible replacement is not a transient readiness failure")
    .unwrap_err();

    endpoint_server.await.unwrap();
    assert_eq!(
        error.kind,
        SearchServiceDiagnosticKind::ComponentIncompatible
    );
    assert!(error
        .technical_detail
        .contains(&format!("expected_protocol={PROTOCOL_VERSION}")));
    assert!(error
        .technical_detail
        .contains(&format!("actual_protocol={}", PROTOCOL_VERSION - 1)));
    assert!(error
        .technical_detail
        .contains(&format!("expected_build={}", daemon_build_id())));
    assert!(error
        .technical_detail
        .contains(&format!("actual_build={}", daemon_build_id())));
    assert!(error
        .technical_detail
        .contains("reinstall the search service components"));
}
