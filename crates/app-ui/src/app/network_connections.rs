use desktop_linux::{
    NetworkConnection, NetworkConnectionId, NetworkMountCredentials, NetworkProtocol,
};
use iced::Task;

use super::FileBrowser;
use crate::commands::{
    network_connection_mount_command, network_connection_unmount_command,
    network_mount_states_command,
};
use crate::model::{ContextMenuState, Message, NavigationMode};
use crate::network_connections::{
    NetworkConnectionEditorState, NetworkConnectionMessage, SidebarNetworkConnectionAction,
    SidebarNetworkConnectionContextMenuState,
};

impl FileBrowser {
    pub(crate) fn network_connection_is_selected(&self, id: &NetworkConnectionId) -> bool {
        self.network_connections
            .selected_connection_id(&self.current_dir)
            .is_some_and(|selected_id| selected_id == id)
    }

    pub(crate) fn path_is_mounted_network(&self, path: &std::path::Path) -> bool {
        self.network_connections.path_is_mounted_network(path)
    }

    pub(super) fn refresh_network_mount_states(&mut self) -> Task<Message> {
        let connections = self.network_connections.connections();
        if connections.is_empty() {
            Task::none()
        } else {
            network_mount_states_command(connections)
        }
    }

    pub(super) fn handle_network_connection_message(
        &mut self,
        message: NetworkConnectionMessage,
    ) -> Task<Message> {
        match message {
            NetworkConnectionMessage::AddRequested => self.open_network_connection_editor(),
            NetworkConnectionMessage::StatusRefreshRequested => self.refresh_network_mount_states(),
            NetworkConnectionMessage::MountStatesLoaded(Ok(states)) => {
                self.network_connections.accept_loaded(states);
                Task::none()
            }
            NetworkConnectionMessage::MountStatesLoaded(Err(error)) => {
                self.network_connections.accept_unavailable(error);
                Task::none()
            }
            NetworkConnectionMessage::Hovered(id) => {
                self.hovered_network_connection = Some(id);
                Task::none()
            }
            NetworkConnectionMessage::HoverCleared(id) => {
                if self.hovered_network_connection.as_ref() == Some(&id) {
                    self.hovered_network_connection = None;
                }
                Task::none()
            }
            NetworkConnectionMessage::Pressed(id) => self.press_network_connection(id),
            NetworkConnectionMessage::MiddlePressed(pane_id, id) => {
                self.activate_pane(pane_id);
                let rename_command = self.commit_rename_if_active();
                let Some(path) = self.network_connections.mounted_path_for(&id) else {
                    return rename_command;
                };
                Task::batch([rename_command, self.open_directory_from_middle_click(path)])
            }
            NetworkConnectionMessage::RightClicked(id) => self.right_click_network_connection(id),
            NetworkConnectionMessage::ActionSelected(id, action) => {
                self.perform_network_connection_action(id, action)
            }
            NetworkConnectionMessage::MountFinished(id, result) => {
                self.accept_network_connection_mounted(id, result)
            }
            NetworkConnectionMessage::UnmountFinished(id, result) => {
                self.accept_network_connection_unmounted(id, result)
            }
            NetworkConnectionMessage::EditorProtocolSelected(protocol) => {
                self.select_network_connection_editor_protocol(protocol)
            }
            NetworkConnectionMessage::EditorLabelChanged(label) => {
                if let Some(editor) = &mut self.network_connection_editor {
                    editor.label = label;
                    editor.error = None;
                }
                Task::none()
            }
            NetworkConnectionMessage::EditorUriChanged(uri) => {
                if let Some(editor) = &mut self.network_connection_editor {
                    editor.uri = uri;
                    editor.error = None;
                }
                Task::none()
            }
            NetworkConnectionMessage::EditorUsernameChanged(username) => {
                if let Some(editor) = &mut self.network_connection_editor {
                    editor.username = username;
                    editor.error = None;
                }
                Task::none()
            }
            NetworkConnectionMessage::EditorPasswordChanged(password) => {
                if let Some(editor) = &mut self.network_connection_editor {
                    editor.password = password;
                    editor.error = None;
                }
                Task::none()
            }
            NetworkConnectionMessage::EditorSaved => self.save_network_connection_editor(),
            NetworkConnectionMessage::EditorCanceled => {
                self.network_connection_editor = None;
                Task::none()
            }
        }
    }

    fn press_network_connection(&mut self, id: NetworkConnectionId) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        self.context_menu = None;
        if let Some(path) = self.network_connections.mounted_path_for(&id) {
            return Task::batch([
                rename_command,
                self.navigate_to(path, NavigationMode::RecordHistory),
            ]);
        }
        let Some(connection) = self.network_connections.connection(&id).cloned() else {
            return rename_command;
        };
        if network_connection_needs_credentials_editor(&connection) {
            self.open_network_connection_credentials_editor(&connection);
            return rename_command;
        }
        self.network_connections.set_connecting(&id);
        Task::batch([
            rename_command,
            network_connection_mount_command(connection, None),
        ])
    }

    fn right_click_network_connection(&mut self, id: NetworkConnectionId) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        let Some(connection) = self.network_connections.entry(&id).cloned() else {
            return rename_command;
        };

        self.clear_preview();
        self.operation_queue.close_panel();
        self.file_drag = None;
        self.selection_marquee = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.context_menu = Some(ContextMenuState::NetworkConnection(
            SidebarNetworkConnectionContextMenuState {
                connection,
                position: self.cursor_position,
            },
        ));
        rename_command
    }

    fn perform_network_connection_action(
        &mut self,
        id: NetworkConnectionId,
        action: SidebarNetworkConnectionAction,
    ) -> Task<Message> {
        self.context_menu = None;
        match action {
            SidebarNetworkConnectionAction::Connect => {
                let Some(connection) = self.network_connections.connection(&id).cloned() else {
                    return Task::none();
                };
                if network_connection_needs_credentials_editor(&connection) {
                    return self.connect_network_connection(connection);
                }
                self.network_connections.set_connecting(&id);
                network_connection_mount_command(connection, None)
            }
            SidebarNetworkConnectionAction::Disconnect => {
                let Some(connection) = self.network_connections.connection(&id).cloned() else {
                    return Task::none();
                };
                self.network_connections.pending_action = Some(id.clone());
                network_connection_unmount_command(id, connection)
            }
            SidebarNetworkConnectionAction::Edit => self.edit_network_connection(id),
            SidebarNetworkConnectionAction::Remove => {
                self.network_connections.remove(&id);
                self.user_config.network_connections = self.network_connections.connections();
                self.persist_user_config_command()
            }
        }
    }

    fn accept_network_connection_mounted(
        &mut self,
        id: NetworkConnectionId,
        result: Result<desktop_linux::MountedNetworkConnection, String>,
    ) -> Task<Message> {
        match result {
            Ok(mounted) => {
                let Some(path) = self.network_connections.accept_mounted(mounted) else {
                    return Task::none();
                };
                Task::batch([
                    self.navigate_to(path, NavigationMode::RecordHistory),
                    self.refresh_network_mount_states(),
                ])
            }
            Err(error) => {
                self.network_connections
                    .accept_mount_error(&id, error.clone());
                self.error = Some(format!("Could not connect network location: {error}"));
                Task::none()
            }
        }
    }

    fn accept_network_connection_unmounted(
        &mut self,
        id: NetworkConnectionId,
        result: Result<(), String>,
    ) -> Task<Message> {
        match result {
            Ok(()) => {
                self.network_connections.accept_unmounted(&id);
                self.refresh_network_mount_states()
            }
            Err(error) => {
                self.network_connections.pending_action = None;
                self.error = Some(format!("Could not disconnect network location: {error}"));
                Task::none()
            }
        }
    }

    fn open_network_connection_editor(&mut self) -> Task<Message> {
        let command = self.prepare_network_connection_editor();
        self.network_connection_editor = Some(NetworkConnectionEditorState::add());
        command
    }

    fn edit_network_connection(&mut self, id: NetworkConnectionId) -> Task<Message> {
        let command = self.prepare_network_connection_editor();
        let Some(connection) = self.network_connections.connection(&id) else {
            return command;
        };
        self.network_connection_editor = Some(NetworkConnectionEditorState::edit(connection));
        command
    }

    fn connect_network_connection(&mut self, connection: NetworkConnection) -> Task<Message> {
        let command = self.commit_rename_if_active();
        self.open_network_connection_credentials_editor(&connection);
        command
    }

    fn open_network_connection_credentials_editor(&mut self, connection: &NetworkConnection) {
        self.prepare_network_connection_editor_surface();
        self.network_connection_editor = Some(NetworkConnectionEditorState::connect(connection));
    }

    fn prepare_network_connection_editor(&mut self) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        self.prepare_network_connection_editor_surface();
        rename_command
    }

    fn prepare_network_connection_editor_surface(&mut self) {
        self.context_menu = None;
        self.open_with = None;
        self.archive_creation = None;
        self.archive_extraction = None;
        self.operation_queue.close_panel();
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.shortcut_capture = None;
        self.clear_pointer_driven_interaction_state();
    }

    fn select_network_connection_editor_protocol(
        &mut self,
        protocol: NetworkProtocol,
    ) -> Task<Message> {
        if let Some(editor) = &mut self.network_connection_editor {
            editor.protocol = protocol;
            editor.error = None;
            if editor.uri == "smb://"
                || editor.uri == "dav://"
                || editor.uri == "davs://"
                || editor.uri == "https://"
            {
                editor.uri = match protocol {
                    NetworkProtocol::Smb => "smb://".to_owned(),
                    NetworkProtocol::WebDav => "https://".to_owned(),
                };
            }
        }
        Task::none()
    }

    fn save_network_connection_editor(&mut self) -> Task<Message> {
        let Some(editor) = self.network_connection_editor.clone() else {
            return Task::none();
        };
        let id = editor.id.clone().unwrap_or_else(|| {
            self.network_connections
                .unique_id_for(editor.label.trim(), editor.uri.trim())
        });
        let username = editor.trimmed_username();
        let connection = match build_network_connection_from_editor(&editor, id.clone(), username) {
            Ok(connection) => connection,
            Err(error) => {
                if let Some(editor) = &mut self.network_connection_editor {
                    editor.error = Some(error.to_string());
                }
                return Task::none();
            }
        };
        if editor.password_is_filled() && connection.username().is_none() {
            if let Some(editor) = &mut self.network_connection_editor {
                editor.error = Some("Username is required when a password is provided".to_owned());
            }
            return Task::none();
        }
        let credentials = network_mount_credentials_from_editor(&editor, &connection);
        let should_connect = editor.mode
            == crate::network_connections::NetworkConnectionEditorMode::Connect
            || credentials.is_some();

        self.network_connections.upsert(connection.clone());
        self.user_config.network_connections = self.network_connections.connections();
        self.network_connection_editor = None;
        if should_connect {
            self.network_connections.set_connecting(&connection.id);
            Task::batch([
                self.persist_user_config_command(),
                network_connection_mount_command(connection, credentials),
            ])
        } else {
            Task::batch([
                self.persist_user_config_command(),
                self.refresh_network_mount_states(),
            ])
        }
    }
}

fn build_network_connection_from_editor(
    editor: &NetworkConnectionEditorState,
    id: NetworkConnectionId,
    username: Option<String>,
) -> Result<NetworkConnection, desktop_linux::NetworkMountError> {
    if username.is_some() {
        NetworkConnection::new_with_username(
            id,
            editor.label.trim().to_owned(),
            editor.protocol,
            editor.uri.trim().to_owned(),
            username,
        )
    } else {
        NetworkConnection::new(
            id,
            editor.label.trim().to_owned(),
            editor.protocol,
            editor.uri.trim().to_owned(),
        )
    }
}

fn network_mount_credentials_from_editor(
    editor: &NetworkConnectionEditorState,
    connection: &NetworkConnection,
) -> Option<NetworkMountCredentials> {
    if editor.password_is_filled() {
        Some(NetworkMountCredentials::new(
            connection.username(),
            editor.password.clone(),
        ))
    } else {
        None
    }
}

fn network_connection_needs_credentials_editor(connection: &NetworkConnection) -> bool {
    connection.protocol == NetworkProtocol::WebDav || connection.username().is_some()
}

#[cfg(test)]
mod tests {
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
    fn pressing_mounted_network_connection_navigates_to_mount_path() {
        let (mut browser, id) = browser_with_connection();
        let mount_path = PathBuf::from("/run/user/1000/gvfs/smb-share:server=server,share=share");
        browser.network_connections.accept_loaded(vec![(
            id.clone(),
            NetworkMountState::Mounted(mount_path.clone()),
        )]);

        let command =
            browser.handle_network_connection_message(NetworkConnectionMessage::Pressed(id));
        drop(command);

        assert_eq!(browser.current_dir, mount_path);
        assert!(browser.network_connections.pending_action.is_none());
    }

    #[test]
    fn pressing_disconnected_network_connection_marks_it_connecting() {
        let (mut browser, id) = browser_with_connection();

        let command = browser
            .handle_network_connection_message(NetworkConnectionMessage::Pressed(id.clone()));
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

        let command = browser
            .handle_network_connection_message(NetworkConnectionMessage::Pressed(id.clone()));
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

        drop(
            browser
                .handle_network_connection_message(NetworkConnectionMessage::Pressed(id.clone())),
        );
        drop(browser.handle_network_connection_message(
            NetworkConnectionMessage::EditorPasswordChanged("secret-password".to_owned()),
        ));
        let command =
            browser.handle_network_connection_message(NetworkConnectionMessage::EditorSaved);
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

        let command = browser
            .handle_network_connection_message(NetworkConnectionMessage::Pressed(id.clone()));
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

        drop(
            browser
                .handle_network_connection_message(NetworkConnectionMessage::Pressed(id.clone())),
        );
        drop(browser.handle_network_connection_message(
            NetworkConnectionMessage::EditorUsernameChanged("user@example.com".to_owned()),
        ));
        drop(browser.handle_network_connection_message(
            NetworkConnectionMessage::EditorPasswordChanged("secret-password".to_owned()),
        ));
        let command =
            browser.handle_network_connection_message(NetworkConnectionMessage::EditorSaved);
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
}
