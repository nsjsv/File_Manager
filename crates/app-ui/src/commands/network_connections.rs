use desktop_linux::{
    load_network_mount_states, mount_network_connection_with_credentials,
    unmount_network_connection, NetworkConnection, NetworkConnectionId, NetworkMountCredentials,
};
use iced::Task;

use crate::model::Message;
use crate::network_connections::NetworkConnectionMessage;

pub(crate) fn network_mount_states_command(connections: Vec<NetworkConnection>) -> Task<Message> {
    Task::perform(
        async move {
            load_network_mount_states(connections)
                .await
                .map_err(|error| error.to_string())
        },
        |outcome| Message::NetworkConnection(NetworkConnectionMessage::MountStatesLoaded(outcome)),
    )
}

pub(crate) fn network_connection_mount_command(
    connection: NetworkConnection,
    credentials: Option<NetworkMountCredentials>,
) -> Task<Message> {
    let id = connection.id.clone();
    Task::perform(
        async move {
            mount_network_connection_with_credentials(connection, credentials)
                .await
                .map_err(|error| error.to_string())
        },
        move |outcome| {
            Message::NetworkConnection(NetworkConnectionMessage::MountFinished(id.clone(), outcome))
        },
    )
}

pub(crate) fn network_connection_unmount_command(
    id: NetworkConnectionId,
    connection: NetworkConnection,
) -> Task<Message> {
    Task::perform(
        async move {
            unmount_network_connection(connection)
                .await
                .map_err(|error| error.to_string())
        },
        move |outcome| {
            Message::NetworkConnection(NetworkConnectionMessage::UnmountFinished(
                id.clone(),
                outcome,
            ))
        },
    )
}
