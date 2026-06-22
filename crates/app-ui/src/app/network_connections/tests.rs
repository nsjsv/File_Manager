use std::path::PathBuf;

use desktop_linux::{NetworkConnection, NetworkMountState};

use super::*;
use crate::config;
use crate::network_connections::NetworkConnectionEditorMode;

fn connection() -> NetworkConnection {
    NetworkConnection::new(
        NetworkConnectionId::new("nas"),
        "NAS",
        NetworkProtocol::Smb,
        "smb://server/share",
    )
    .unwrap()
}

fn webdav_connection() -> NetworkConnection {
    NetworkConnection::new(
        NetworkConnectionId::new("webdav"),
        "WebDAV",
        NetworkProtocol::WebDav,
        "davs://webdav.123pan.cn/webdav",
    )
    .unwrap()
}

fn authenticated_smb_connection() -> NetworkConnection {
    NetworkConnection::new_with_username(
        NetworkConnectionId::new("smb-auth"),
        "SMB Auth",
        NetworkProtocol::Smb,
        "smb://server/share",
        Some("smbtest".to_owned()),
    )
    .unwrap()
}

fn browser_with_connection() -> (FileBrowser, NetworkConnectionId) {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let connection = connection();
    let id = connection.id.clone();
    browser.network_connections =
        crate::network_connections::NetworkConnectionState::from_connections(vec![connection]);
    (browser, id)
}

fn browser_with_webdav_connection() -> (FileBrowser, NetworkConnectionId) {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let connection = webdav_connection();
    let id = connection.id.clone();
    browser.network_connections =
        crate::network_connections::NetworkConnectionState::from_connections(vec![connection]);
    (browser, id)
}

fn browser_with_authenticated_smb_connection() -> (FileBrowser, NetworkConnectionId) {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let connection = authenticated_smb_connection();
    let id = connection.id.clone();
    browser.network_connections =
        crate::network_connections::NetworkConnectionState::from_connections(vec![connection]);
    (browser, id)
}

#[test]
fn add_network_connection_editor_starts_with_empty_uri() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());

    drop(browser.handle_network_connection_message(NetworkConnectionMessage::AddRequested));

    let editor = browser
        .network_connection_editor
        .as_ref()
        .expect("network connection editor");
    assert_eq!(editor.mode, NetworkConnectionEditorMode::Add);
    assert_eq!(editor.protocol, NetworkProtocol::Smb);
    assert!(editor.uri.is_empty());
}

#[test]
fn saving_bare_smb_connection_prefixes_smb_scheme() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());

    drop(browser.handle_network_connection_message(NetworkConnectionMessage::AddRequested));
    drop(
        browser.handle_network_connection_message(NetworkConnectionMessage::EditorLabelChanged(
            "NAS".to_owned(),
        )),
    );
    drop(
        browser.handle_network_connection_message(NetworkConnectionMessage::EditorUriChanged(
            "server/share".to_owned(),
        )),
    );
    drop(browser.handle_network_connection_message(NetworkConnectionMessage::EditorSaved));

    let saved_connection = browser
        .user_config
        .network_connections
        .first()
        .expect("saved connection");
    assert_eq!(saved_connection.protocol, NetworkProtocol::Smb);
    assert_eq!(saved_connection.uri, "smb://server/share");
    assert!(browser.network_connection_editor.is_none());
}

#[test]
fn saving_bare_smb_server_without_share_keeps_platform_validation() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());

    drop(browser.handle_network_connection_message(NetworkConnectionMessage::AddRequested));
    drop(
        browser.handle_network_connection_message(NetworkConnectionMessage::EditorUriChanged(
            "server".to_owned(),
        )),
    );
    drop(browser.handle_network_connection_message(NetworkConnectionMessage::EditorSaved));

    assert!(browser.user_config.network_connections.is_empty());
    assert!(browser
        .network_connection_editor
        .as_ref()
        .and_then(|editor| editor.error.as_deref())
        .is_some_and(|error| error.contains("SMB URI must include a share name")));
}

#[test]
fn saving_bare_webdav_connection_prefixes_https_for_platform_normalization() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());

    drop(browser.handle_network_connection_message(NetworkConnectionMessage::AddRequested));
    drop(browser.handle_network_connection_message(
        NetworkConnectionMessage::EditorProtocolSelected(NetworkProtocol::WebDav),
    ));
    drop(
        browser.handle_network_connection_message(NetworkConnectionMessage::EditorLabelChanged(
            "Docs".to_owned(),
        )),
    );
    drop(
        browser.handle_network_connection_message(NetworkConnectionMessage::EditorUriChanged(
            "example.test/docs".to_owned(),
        )),
    );
    drop(browser.handle_network_connection_message(NetworkConnectionMessage::EditorSaved));

    let saved_connection = browser
        .user_config
        .network_connections
        .first()
        .expect("saved connection");
    assert_eq!(saved_connection.protocol, NetworkProtocol::WebDav);
    assert_eq!(saved_connection.uri, "davs://example.test/docs");
    assert!(browser.network_connection_editor.is_none());
}

#[test]
fn saving_explicit_webdav_scheme_preserves_scheme_semantics() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());

    drop(browser.handle_network_connection_message(NetworkConnectionMessage::AddRequested));
    drop(browser.handle_network_connection_message(
        NetworkConnectionMessage::EditorProtocolSelected(NetworkProtocol::WebDav),
    ));
    drop(
        browser.handle_network_connection_message(NetworkConnectionMessage::EditorUriChanged(
            "http://example.test/docs".to_owned(),
        )),
    );
    drop(browser.handle_network_connection_message(NetworkConnectionMessage::EditorSaved));

    let saved_connection = browser
        .user_config
        .network_connections
        .first()
        .expect("saved connection");
    assert_eq!(saved_connection.uri, "dav://example.test/docs");
}

#[test]
fn saving_explicit_smb_scheme_preserves_scheme() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());

    drop(browser.handle_network_connection_message(NetworkConnectionMessage::AddRequested));
    drop(
        browser.handle_network_connection_message(NetworkConnectionMessage::EditorUriChanged(
            "smb://server/share".to_owned(),
        )),
    );
    drop(browser.handle_network_connection_message(NetworkConnectionMessage::EditorSaved));

    let saved_connection = browser
        .user_config
        .network_connections
        .first()
        .expect("saved connection");
    assert_eq!(saved_connection.uri, "smb://server/share");
}

#[test]
fn saving_bare_webdav_credentials_keeps_password_out_of_config() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());

    drop(browser.handle_network_connection_message(NetworkConnectionMessage::AddRequested));
    drop(browser.handle_network_connection_message(
        NetworkConnectionMessage::EditorProtocolSelected(NetworkProtocol::WebDav),
    ));
    drop(
        browser.handle_network_connection_message(NetworkConnectionMessage::EditorUriChanged(
            "example.test/docs".to_owned(),
        )),
    );
    drop(browser.handle_network_connection_message(
        NetworkConnectionMessage::EditorUsernameChanged("user@example.com".to_owned()),
    ));
    drop(browser.handle_network_connection_message(
        NetworkConnectionMessage::EditorPasswordChanged("secret-password".to_owned()),
    ));
    drop(browser.handle_network_connection_message(NetworkConnectionMessage::EditorSaved));

    let saved_connection = browser
        .user_config
        .network_connections
        .first()
        .expect("saved connection");
    assert_eq!(
        saved_connection.uri,
        "davs://user%40example.com@example.test/docs"
    );
    assert!(!saved_connection.uri.contains("secret-password"));
    assert!(!browser
        .user_config
        .network_connections
        .iter()
        .any(|connection| connection.uri.contains("secret-password")));
}

#[test]
fn pressing_mounted_network_connection_navigates_to_mount_path() {
    let (mut browser, id) = browser_with_connection();
    let mount_path = PathBuf::from("/run/user/1000/gvfs/smb-share:server=server,share=share");
    browser.network_connections.accept_loaded(vec![(
        id.clone(),
        NetworkMountState::Mounted(mount_path.clone()),
    )]);

    let command = browser.handle_network_connection_message(NetworkConnectionMessage::Pressed(id));
    drop(command);

    assert_eq!(browser.current_dir, mount_path);
    assert!(browser.network_connections.pending_action.is_none());
}

#[test]
fn pressing_disconnected_network_connection_marks_it_connecting() {
    let (mut browser, id) = browser_with_connection();

    let command =
        browser.handle_network_connection_message(NetworkConnectionMessage::Pressed(id.clone()));
    drop(command);

    assert_eq!(
        browser.network_connections.pending_action.as_ref(),
        Some(&id)
    );
    assert!(matches!(
        browser
            .network_connections
            .entry(&id)
            .map(|entry| &entry.state),
        Some(NetworkMountState::Connecting)
    ));
}

#[test]
fn pressing_authenticated_smb_connection_opens_connect_editor() {
    let (mut browser, id) = browser_with_authenticated_smb_connection();

    let command =
        browser.handle_network_connection_message(NetworkConnectionMessage::Pressed(id.clone()));
    drop(command);

    assert!(browser.network_connections.pending_action.is_none());
    let editor = browser
        .network_connection_editor
        .as_ref()
        .expect("connect editor");
    assert_eq!(editor.mode, NetworkConnectionEditorMode::Connect);
    assert_eq!(editor.protocol, NetworkProtocol::Smb);
    assert_eq!(editor.username, "smbtest");
}

#[test]
fn connect_action_for_authenticated_smb_opens_connect_editor() {
    let (mut browser, id) = browser_with_authenticated_smb_connection();

    let command =
        browser.handle_network_connection_message(NetworkConnectionMessage::ActionSelected(
            id.clone(),
            SidebarNetworkConnectionAction::Connect,
        ));
    drop(command);

    assert!(browser.network_connections.pending_action.is_none());
    let editor = browser
        .network_connection_editor
        .as_ref()
        .expect("connect editor");
    assert_eq!(editor.mode, NetworkConnectionEditorMode::Connect);
    assert_eq!(editor.protocol, NetworkProtocol::Smb);
    assert_eq!(editor.username, "smbtest");
}

#[test]
fn submitting_smb_credentials_saves_username_without_password() {
    let (mut browser, id) = browser_with_authenticated_smb_connection();

    drop(browser.handle_network_connection_message(NetworkConnectionMessage::Pressed(id.clone())));
    drop(browser.handle_network_connection_message(
        NetworkConnectionMessage::EditorPasswordChanged("secret-password".to_owned()),
    ));
    let command = browser.handle_network_connection_message(NetworkConnectionMessage::EditorSaved);
    drop(command);

    let saved_connection = browser.network_connections.connection(&id).unwrap();
    assert_eq!(saved_connection.uri, "smb://smbtest@server/share");
    assert_eq!(
        browser.network_connections.pending_action.as_ref(),
        Some(&id)
    );
    assert!(browser
        .user_config
        .network_connections
        .iter()
        .all(|connection| !connection.uri.contains("secret-password")));
    assert!(browser.network_connection_editor.is_none());
}

#[test]
fn pressing_disconnected_webdav_connection_opens_connect_editor() {
    let (mut browser, id) = browser_with_webdav_connection();

    let command =
        browser.handle_network_connection_message(NetworkConnectionMessage::Pressed(id.clone()));
    drop(command);

    assert!(browser.network_connections.pending_action.is_none());
    assert!(matches!(
        browser
            .network_connection_editor
            .as_ref()
            .map(|editor| editor.mode),
        Some(NetworkConnectionEditorMode::Connect)
    ));
}

#[test]
fn submitting_webdav_credentials_saves_username_without_password() {
    let (mut browser, id) = browser_with_webdav_connection();

    drop(browser.handle_network_connection_message(NetworkConnectionMessage::Pressed(id.clone())));
    drop(browser.handle_network_connection_message(
        NetworkConnectionMessage::EditorUsernameChanged("user@example.com".to_owned()),
    ));
    drop(browser.handle_network_connection_message(
        NetworkConnectionMessage::EditorPasswordChanged("secret-password".to_owned()),
    ));
    let command = browser.handle_network_connection_message(NetworkConnectionMessage::EditorSaved);
    drop(command);

    let saved_connection = browser.network_connections.connection(&id).unwrap();
    assert_eq!(
        saved_connection.uri,
        "davs://user%40example.com@webdav.123pan.cn/webdav"
    );
    assert_eq!(
        browser.network_connections.pending_action.as_ref(),
        Some(&id)
    );
    assert!(browser
        .user_config
        .network_connections
        .iter()
        .all(|connection| !connection.uri.contains("secret-password")));
    assert!(browser.network_connection_editor.is_none());
}

#[test]
fn mount_failure_sets_global_error_and_entry_error() {
    let (mut browser, id) = browser_with_connection();

    let command =
        browser.handle_network_connection_message(NetworkConnectionMessage::MountFinished(
            id.clone(),
            Err("authentication failed".to_owned()),
        ));
    drop(command);

    assert_eq!(
        browser.error.as_deref(),
        Some("Could not connect network location: authentication failed")
    );
    assert!(matches!(
        browser.network_connections.entry(&id).map(|entry| &entry.state),
        Some(NetworkMountState::Error(error)) if error == "authentication failed"
    ));
}
