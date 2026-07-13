use std::num::NonZeroU32;
use std::path::PathBuf;

use file_search::{
    daemon_build_id, read_service_request, write_service_event, IndexedQueryAvailability,
    SearchServiceEvent, SearchServicePhase, SearchServiceRequest, SearchServiceStatus,
    PROTOCOL_VERSION,
};
use tempfile::tempdir;
use tokio::net::UnixListener;

use super::{
    inspect_search_endpoint, SearchUnitAction, SearchUnitController, SearchUnitSnapshot,
    UnitActiveState,
};

fn valid_snapshot_text() -> &'static str {
    "NRestarts=0\nMemorySwapMax=0\nSubState=running\nResult=success\nControlGroup=/user.slice/search.service\nMemoryMax=96000000\nActiveState=active\nExecMainStatus=0\nMainPID=42\nMemoryHigh=80000000\n"
}

#[test]
fn unit_snapshot_parses_properties_without_order_dependency() {
    let snapshot = SearchUnitSnapshot::parse(valid_snapshot_text()).unwrap();

    assert_eq!(snapshot.active_state, UnitActiveState::Active);
    assert_eq!(snapshot.sub_state, "running");
    assert_eq!(snapshot.main_pid, NonZeroU32::new(42));
    assert_eq!(
        snapshot.control_group,
        PathBuf::from("/user.slice/search.service")
    );
    assert_eq!(snapshot.memory_high, 80_000_000);
    assert_eq!(snapshot.memory_max, 96_000_000);
    assert_eq!(snapshot.memory_swap_max, 0);
    assert_eq!(snapshot.service_result, "success");
    assert_eq!(snapshot.exec_main_status, 0);
    assert_eq!(snapshot.restart_count, 0);
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
        let snapshot = SearchUnitSnapshot::parse(&snapshot_text).unwrap();

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
    ] {
        let snapshot_without_property = valid_snapshot_text()
            .lines()
            .filter(|line| !line.starts_with(&format!("{property_name}=")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            SearchUnitSnapshot::parse(&snapshot_without_property).is_err(),
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
            SearchUnitSnapshot::parse(&invalid_snapshot).is_err(),
            "{invalid_property} must fail closed"
        );
    }
}

#[test]
fn unit_actions_use_user_systemd_without_a_shell() {
    assert_eq!(
        SearchUnitAction::Show.arguments(),
        &[
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
            "file-manager-search.service",
        ]
    );
    assert_eq!(
        SearchUnitAction::DaemonReload.arguments(),
        &["--user", "--no-pager", "daemon-reload"]
    );
    assert_eq!(
        SearchUnitAction::Start.arguments(),
        &[
            "--user",
            "--no-pager",
            "--no-block",
            "start",
            "file-manager-search.service"
        ]
    );
    assert_eq!(
        SearchUnitAction::Restart.arguments(),
        &[
            "--user",
            "--no-pager",
            "--no-block",
            "restart",
            "file-manager-search.service"
        ]
    );
}

#[tokio::test]
async fn effective_cgroup_accepts_only_safe_kernel_rounding() {
    let temporary_directory = tempdir().unwrap();
    let cgroup_directory = temporary_directory.path().join("user.slice/search.service");
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
    let unit_controller = SearchUnitController {
        systemctl_executable: PathBuf::from("unused-systemctl"),
        cgroup_root: temporary_directory.path().to_path_buf(),
    };
    let snapshot = SearchUnitSnapshot::parse(valid_snapshot_text()).unwrap();

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
