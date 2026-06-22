use std::path::{Path, PathBuf};

use desktop_linux::{
    MountedNetworkConnection, NetworkConnection, NetworkConnectionId, NetworkMountState,
    NetworkProtocol,
};
use iced::Point;

#[derive(Debug, Clone)]
pub(crate) enum NetworkConnectionMessage {
    AddRequested,
    StatusRefreshRequested,
    MountStatesLoaded(Result<Vec<(NetworkConnectionId, NetworkMountState)>, String>),
    Hovered(NetworkConnectionId),
    HoverCleared(NetworkConnectionId),
    Pressed(NetworkConnectionId),
    MiddlePressed(crate::model::BrowserPaneId, NetworkConnectionId),
    RightClicked(NetworkConnectionId),
    ActionSelected(NetworkConnectionId, SidebarNetworkConnectionAction),
    MountFinished(
        NetworkConnectionId,
        Result<MountedNetworkConnection, String>,
    ),
    UnmountFinished(NetworkConnectionId, Result<(), String>),
    EditorProtocolSelected(NetworkProtocol),
    EditorLabelChanged(String),
    EditorUriChanged(String),
    EditorUsernameChanged(String),
    EditorPasswordChanged(String),
    EditorSaved,
    EditorCanceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarNetworkConnectionAction {
    Connect,
    Disconnect,
    Edit,
    Remove,
}

impl SidebarNetworkConnectionAction {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Connect => "Connect",
            Self::Disconnect => "Disconnect",
            Self::Edit => "Edit",
            Self::Remove => "Remove",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarNetworkConnectionEntry {
    pub(crate) connection: NetworkConnection,
    pub(crate) state: NetworkMountState,
}

impl SidebarNetworkConnectionEntry {
    fn new(connection: NetworkConnection) -> Self {
        Self {
            connection,
            state: NetworkMountState::Disconnected,
        }
    }

    pub(crate) fn id(&self) -> &NetworkConnectionId {
        &self.connection.id
    }

    pub(crate) fn label(&self) -> String {
        self.connection.label_or_default()
    }

    pub(crate) fn mount_path(&self) -> Option<&Path> {
        match &self.state {
            NetworkMountState::Mounted(path) => Some(path.as_path()),
            _ => None,
        }
    }

    pub(crate) fn available_actions(&self) -> Vec<SidebarNetworkConnectionAction> {
        match &self.state {
            NetworkMountState::Mounted(_) => vec![
                SidebarNetworkConnectionAction::Disconnect,
                SidebarNetworkConnectionAction::Edit,
                SidebarNetworkConnectionAction::Remove,
            ],
            NetworkMountState::Disconnected | NetworkMountState::Error(_) => [
                SidebarNetworkConnectionAction::Connect,
                SidebarNetworkConnectionAction::Edit,
                SidebarNetworkConnectionAction::Remove,
            ]
            .to_vec(),
            NetworkMountState::Connecting => vec![
                SidebarNetworkConnectionAction::Edit,
                SidebarNetworkConnectionAction::Remove,
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarNetworkConnectionContextMenuState {
    pub(crate) connection: SidebarNetworkConnectionEntry,
    pub(crate) position: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkConnectionEditorMode {
    Add,
    Edit,
    Connect,
}

#[derive(Debug, Clone)]
pub(crate) struct NetworkConnectionEditorState {
    pub(crate) mode: NetworkConnectionEditorMode,
    pub(crate) id: Option<NetworkConnectionId>,
    pub(crate) protocol: NetworkProtocol,
    pub(crate) label: String,
    pub(crate) uri: String,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) error: Option<String>,
}

impl NetworkConnectionEditorState {
    pub(crate) fn add() -> Self {
        Self {
            mode: NetworkConnectionEditorMode::Add,
            id: None,
            protocol: NetworkProtocol::Smb,
            label: String::new(),
            uri: String::new(),
            username: String::new(),
            password: String::new(),
            error: None,
        }
    }

    pub(crate) fn edit(connection: &NetworkConnection) -> Self {
        Self {
            mode: NetworkConnectionEditorMode::Edit,
            id: Some(connection.id.clone()),
            protocol: connection.protocol,
            label: connection.label.clone(),
            uri: connection.uri_without_username(),
            username: connection.username().unwrap_or_default(),
            password: String::new(),
            error: None,
        }
    }

    pub(crate) fn connect(connection: &NetworkConnection) -> Self {
        Self {
            mode: NetworkConnectionEditorMode::Connect,
            id: Some(connection.id.clone()),
            protocol: connection.protocol,
            label: connection.label.clone(),
            uri: connection.uri_without_username(),
            username: connection.username().unwrap_or_default(),
            password: String::new(),
            error: None,
        }
    }

    pub(crate) fn password_is_filled(&self) -> bool {
        !self.password.is_empty()
    }

    pub(crate) fn trimmed_username(&self) -> Option<String> {
        let username = self.username.trim();
        if username.is_empty() {
            None
        } else {
            Some(username.to_owned())
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct NetworkConnectionState {
    pub(crate) entries: Vec<SidebarNetworkConnectionEntry>,
    pub(crate) unavailable: Option<String>,
    pub(crate) pending_action: Option<NetworkConnectionId>,
}

impl NetworkConnectionState {
    pub(crate) fn from_connections(connections: Vec<NetworkConnection>) -> Self {
        Self {
            entries: connections
                .into_iter()
                .map(SidebarNetworkConnectionEntry::new)
                .collect(),
            unavailable: None,
            pending_action: None,
        }
    }

    pub(crate) fn connections(&self) -> Vec<NetworkConnection> {
        self.entries
            .iter()
            .map(|entry| entry.connection.clone())
            .collect()
    }

    pub(crate) fn connection(&self, id: &NetworkConnectionId) -> Option<&NetworkConnection> {
        self.entries
            .iter()
            .find(|entry| entry.id() == id)
            .map(|entry| &entry.connection)
    }

    pub(crate) fn entry(&self, id: &NetworkConnectionId) -> Option<&SidebarNetworkConnectionEntry> {
        self.entries.iter().find(|entry| entry.id() == id)
    }

    pub(crate) fn accept_loaded(
        &mut self,
        statuses: Vec<(NetworkConnectionId, NetworkMountState)>,
    ) {
        self.unavailable = None;
        for (id, state) in statuses {
            if self.pending_action.as_ref() == Some(&id) {
                continue;
            }
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id() == &id) {
                entry.state = state;
            }
        }
    }

    pub(crate) fn accept_unavailable(&mut self, error: String) {
        self.unavailable = Some(error);
    }

    pub(crate) fn set_connecting(&mut self, id: &NetworkConnectionId) {
        self.pending_action = Some(id.clone());
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id() == id) {
            entry.state = NetworkMountState::Connecting;
        }
    }

    pub(crate) fn accept_mounted(&mut self, mounted: MountedNetworkConnection) -> Option<PathBuf> {
        self.pending_action = None;
        let mount_path = mounted.mount_path;
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.id() == &mounted.connection.id)
        {
            entry.connection = mounted.connection;
            entry.state = NetworkMountState::Mounted(mount_path.clone());
        }
        Some(mount_path)
    }

    pub(crate) fn accept_mount_error(&mut self, id: &NetworkConnectionId, error: String) {
        self.pending_action = None;
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id() == id) {
            entry.state = NetworkMountState::Error(error);
        }
    }

    pub(crate) fn accept_unmounted(&mut self, id: &NetworkConnectionId) {
        self.pending_action = None;
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id() == id) {
            entry.state = NetworkMountState::Disconnected;
        }
    }

    pub(crate) fn upsert(&mut self, connection: NetworkConnection) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.id() == &connection.id)
        {
            entry.connection = connection;
            entry.state = NetworkMountState::Disconnected;
        } else {
            self.entries
                .push(SidebarNetworkConnectionEntry::new(connection));
        }
    }

    pub(crate) fn remove(&mut self, id: &NetworkConnectionId) {
        self.entries.retain(|entry| entry.id() != id);
        if self.pending_action.as_ref() == Some(id) {
            self.pending_action = None;
        }
    }

    pub(crate) fn mounted_path_for(&self, id: &NetworkConnectionId) -> Option<PathBuf> {
        self.entry(id)
            .and_then(SidebarNetworkConnectionEntry::mount_path)
            .map(Path::to_path_buf)
    }

    pub(crate) fn selected_connection_id(
        &self,
        current_dir: &Path,
    ) -> Option<&NetworkConnectionId> {
        self.entries
            .iter()
            .flat_map(|entry| {
                entry
                    .mount_path()
                    .filter(|mount_path| path_is_inside_mount(current_dir, mount_path))
                    .map(|mount_path| (entry.id(), mount_path.components().count()))
            })
            .max_by_key(|(_, depth)| *depth)
            .map(|(id, _)| id)
    }

    pub(crate) fn path_is_mounted_network(&self, path: &Path) -> bool {
        self.entries.iter().any(|entry| {
            entry
                .mount_path()
                .is_some_and(|mount_path| path_is_inside_mount(path, mount_path))
        })
    }

    pub(crate) fn unique_id_for(&self, label: &str, uri: &str) -> NetworkConnectionId {
        let base = network_connection_id_base(label, uri);
        let mut candidate = base.clone();
        let mut suffix = 2;
        while self
            .entries
            .iter()
            .any(|entry| entry.connection.id.as_str() == candidate)
        {
            candidate = format!("{base}-{suffix}");
            suffix += 1;
        }
        NetworkConnectionId::new(candidate)
    }
}

fn path_is_inside_mount(path: &Path, mount_point: &Path) -> bool {
    path == mount_point || path.starts_with(mount_point)
}

fn network_connection_id_base(label: &str, uri: &str) -> String {
    let source = if label.trim().is_empty() { uri } else { label };
    let mut output = String::new();
    for character in source.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            output.push(character);
        } else if !output.ends_with('-') {
            output.push('-');
        }
    }
    let trimmed = output.trim_matches('-');
    if trimmed.is_empty() {
        "network".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(id: &str, uri: &str) -> NetworkConnection {
        NetworkConnection::new(NetworkConnectionId::new(id), id, NetworkProtocol::Smb, uri).unwrap()
    }

    #[test]
    fn mounted_network_selection_uses_longest_prefix() {
        let outer = connection("outer", "smb://server/outer");
        let inner = connection("inner", "smb://server/inner");
        let mut state = NetworkConnectionState::from_connections(vec![outer, inner]);
        state.entries[0].state = NetworkMountState::Mounted(PathBuf::from("/run/gvfs/server"));
        state.entries[1].state =
            NetworkMountState::Mounted(PathBuf::from("/run/gvfs/server/photos"));

        let selected = state
            .selected_connection_id(Path::new("/run/gvfs/server/photos/raw"))
            .expect("selected network connection");

        assert_eq!(selected.as_str(), "inner");
    }

    #[test]
    fn unique_id_appends_suffix_for_duplicate_base() {
        let state =
            NetworkConnectionState::from_connections(vec![connection("nas", "smb://server/share")]);

        let id = state.unique_id_for("NAS", "smb://other/share");

        assert_eq!(id.as_str(), "nas-2");
    }
}
