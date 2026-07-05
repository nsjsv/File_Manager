use std::time::Duration;

use file_index::daemon::{run, IndexDaemonConfig};
use file_index::{
    DirectoryErrorPolicy, IndexClient, IndexClientError, IndexError, IndexProfile,
    IndexServiceCommand, IndexServiceEvent, SearchMode, SearchQuery,
};
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn daemon_client_configures_builds_and_queries_profile() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("file-indexd.sock");
    let index_base_dir = dir.path().join("index-base");
    let root = dir.path().join("root");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("needle.txt"), b"match")
        .await
        .unwrap();
    let daemon = tokio::spawn(run(IndexDaemonConfig {
        socket_path: socket_path.clone(),
    }));
    let client = IndexClient::new(index_base_dir.clone(), socket_path.clone());
    wait_for_daemon(&client).await;
    assert!(
        !index_base_dir.exists(),
        "ping should not open the daemon core or create the control DB directory"
    );
    let profile = IndexProfile {
        id: "main".to_owned(),
        roots: vec![root.clone()],
        include_hidden: false,
        exclude_patterns: Vec::new(),
        directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
        media: Default::default(),
    };

    let configured = client
        .execute(IndexServiceCommand::ConfigureProfile(profile))
        .await
        .unwrap();
    assert_eq!(
        configured,
        IndexServiceEvent::ProfileConfigured("main".to_owned())
    );

    let rebuilt = client
        .execute(IndexServiceCommand::Rebuild {
            profile_id: "main".to_owned(),
            root: root.clone(),
        })
        .await
        .unwrap();
    assert!(matches!(rebuilt, IndexServiceEvent::RebuildFinished(_)));

    let queried = client
        .execute(IndexServiceCommand::Query(SearchQuery {
            profile_id: "main".to_owned(),
            root,
            text: "needle".to_owned(),
            mode: SearchMode::Files,
            limit: 10,
        }))
        .await
        .unwrap();
    let IndexServiceEvent::QueryFinished(outcome) = queried else {
        panic!("expected query event");
    };

    assert_eq!(outcome.matches.len(), 1);
    assert_eq!(outcome.matches[0].name.to_string_lossy(), "needle.txt");
    daemon.abort();
}

#[tokio::test]
async fn daemon_query_with_cancel_returns_cancelled() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("file-indexd.sock");
    let index_base_dir = dir.path().join("index-base");
    let root = dir.path().join("root");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("needle.txt"), b"match")
        .await
        .unwrap();
    let daemon = tokio::spawn(run(IndexDaemonConfig {
        socket_path: socket_path.clone(),
    }));
    let client = IndexClient::new(index_base_dir, socket_path);
    wait_for_daemon(&client).await;
    client
        .execute(IndexServiceCommand::ConfigureProfile(IndexProfile {
            id: "main".to_owned(),
            roots: vec![root.clone()],
            include_hidden: false,
            exclude_patterns: Vec::new(),
            directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
            media: Default::default(),
        }))
        .await
        .unwrap();
    client
        .execute(IndexServiceCommand::Rebuild {
            profile_id: "main".to_owned(),
            root: root.clone(),
        })
        .await
        .unwrap();

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = client
        .execute_with_cancel(
            IndexServiceCommand::Query(SearchQuery {
                profile_id: "main".to_owned(),
                root,
                text: "needle".to_owned(),
                mode: SearchMode::Files,
                limit: 10,
            }),
            cancellation,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        IndexClientError::Service(message) if message == IndexError::Cancelled.to_string()
    ));
    daemon.abort();
}

#[tokio::test]
async fn daemon_restart_loads_stored_profile() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("file-indexd.sock");
    let index_base_dir = dir.path().join("index-base");
    let root = dir.path().join("root");
    tokio::fs::create_dir_all(&root).await.unwrap();

    let daemon = tokio::spawn(run(IndexDaemonConfig {
        socket_path: socket_path.clone(),
    }));
    let client = IndexClient::new(index_base_dir.clone(), socket_path.clone());
    wait_for_daemon(&client).await;

    let profile = IndexProfile {
        id: "main".to_owned(),
        roots: vec![root.clone()],
        include_hidden: true,
        exclude_patterns: vec!["target/".to_owned()],
        directory_error_policy: DirectoryErrorPolicy::Abort,
        media: Default::default(),
    };
    client
        .execute(IndexServiceCommand::ConfigureProfile(profile.clone()))
        .await
        .unwrap();
    daemon.abort();
    let _ = daemon.await;

    let restarted = tokio::spawn(run(IndexDaemonConfig {
        socket_path: socket_path.clone(),
    }));
    let restarted_client = IndexClient::new(index_base_dir, socket_path);
    wait_for_daemon(&restarted_client).await;

    let loaded = restarted_client
        .execute(IndexServiceCommand::LoadProfile("main".to_owned()))
        .await
        .unwrap();
    assert_eq!(loaded, IndexServiceEvent::ProfileLoaded(Some(profile)));
    restarted.abort();
}

#[tokio::test]
async fn daemon_shutdown_exits_cleanly() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("file-indexd.sock");
    let index_base_dir = dir.path().join("index-base");
    let daemon = tokio::spawn(run(IndexDaemonConfig {
        socket_path: socket_path.clone(),
    }));
    let client = IndexClient::new(index_base_dir, socket_path);
    wait_for_daemon(&client).await;

    let shutdown = client.execute(IndexServiceCommand::Shutdown).await.unwrap();
    assert_eq!(shutdown, IndexServiceEvent::Shutdown);
    tokio::time::timeout(Duration::from_secs(5), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        client.execute(IndexServiceCommand::Ping).await,
        Err(IndexClientError::Connect { .. })
    ));
}

async fn wait_for_daemon(client: &IndexClient) {
    for _ in 0..50 {
        match client.execute(IndexServiceCommand::Ping).await {
            Ok(IndexServiceEvent::Pong { daemon_version }) => {
                assert_eq!(daemon_version, env!("CARGO_PKG_VERSION"));
                return;
            }
            Ok(event) => panic!("daemon ping returned unexpected event: {event:?}"),
            Err(IndexClientError::Connect { .. }) => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => panic!("daemon probe failed: {error}"),
        }
    }
    panic!("daemon socket was not created");
}
