use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use file_core::FileKind;
use iced::futures::channel::mpsc::Sender as IcedSender;
use iced::futures::SinkExt;
use iced::Task;
use tokio_util::sync::CancellationToken;

use crate::model::{
    FilePropertiesDirectoryContents, FilePropertiesDirectoryContentsState,
    FilePropertiesPermissions, FilePropertiesRequest, FilePropertiesSnapshot, Message,
};

const FILE_PROPERTIES_CHANNEL_SIZE: usize = 8;
const DIRECTORY_CONTENTS_PROGRESS_INTERVAL: usize = 128;

pub(crate) fn file_properties_command(
    request: FilePropertiesRequest,
    cancellation: CancellationToken,
) -> Task<Message> {
    Task::stream(iced::stream::channel(
        FILE_PROPERTIES_CHANNEL_SIZE,
        async move |mut output| {
            let properties_outcome = load_file_properties(request.path.clone()).await;
            let directory_contents_required = properties_outcome.as_ref().is_ok_and(|snapshot| {
                matches!(
                    snapshot.directory_contents,
                    FilePropertiesDirectoryContentsState::Loading(_)
                )
            });

            if output
                .send(Message::FilePropertiesLoaded(
                    request.clone(),
                    properties_outcome,
                ))
                .await
                .is_err()
            {
                return;
            }

            if !directory_contents_required || cancellation.is_cancelled() {
                return;
            }

            let contents_outcome =
                load_directory_properties_contents(request.clone(), cancellation, &mut output)
                    .await;
            let _ = output
                .send(Message::FilePropertiesDirectoryContentsLoaded(
                    request,
                    contents_outcome,
                ))
                .await;
        },
    ))
}

pub(crate) fn set_file_properties_permissions_command(
    path: PathBuf,
    permissions: FilePropertiesPermissions,
) -> Task<Message> {
    let requested_path = path.clone();
    Task::perform(
        set_file_properties_permissions(path, permissions),
        move |permissions_outcome| {
            Message::FilePropertiesPermissionsUpdated(requested_path.clone(), permissions_outcome)
        },
    )
}

async fn load_file_properties(path: PathBuf) -> Result<FilePropertiesSnapshot, String> {
    tokio::task::spawn_blocking(move || read_file_properties(path))
        .await
        .map_err(|error| error.to_string())?
}

async fn set_file_properties_permissions(
    path: PathBuf,
    permissions: FilePropertiesPermissions,
) -> Result<FilePropertiesPermissions, String> {
    tokio::task::spawn_blocking(move || write_file_properties_permissions(path, permissions))
        .await
        .map_err(|error| error.to_string())?
}

fn read_file_properties(path: PathBuf) -> Result<FilePropertiesSnapshot, String> {
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_file() {
        FileKind::File
    } else if file_type.is_symlink() {
        FileKind::Symlink
    } else {
        FileKind::Other
    };
    let type_label = if file_type.is_symlink() {
        "Symbolic Link".to_owned()
    } else if file_type.is_dir() {
        "Folder".to_owned()
    } else if file_type.is_file() {
        "File".to_owned()
    } else {
        "Other".to_owned()
    };
    let name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| path.as_os_str().to_os_string());
    let location = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));

    let directory_contents = if file_type.is_dir() {
        FilePropertiesDirectoryContentsState::Loading(None)
    } else {
        FilePropertiesDirectoryContentsState::NotDirectory
    };
    let size_bytes = metadata.len();
    let disk_size_bytes = metadata_disk_size(&metadata);

    Ok(FilePropertiesSnapshot {
        name,
        kind,
        type_label,
        location,
        created: metadata.created().ok(),
        modified: metadata.modified().ok(),
        accessed: metadata.accessed().ok(),
        size_bytes,
        disk_size_bytes,
        directory_contents,
        permissions: metadata_properties_permissions(&metadata, file_type.is_symlink()),
    })
}

async fn load_directory_properties_contents(
    request: FilePropertiesRequest,
    cancellation: CancellationToken,
    output: &mut IcedSender<Message>,
) -> Result<FilePropertiesDirectoryContents, String> {
    let progress = {
        let request = request.clone();
        let mut output = output.clone();
        move |contents: FilePropertiesDirectoryContents| {
            let _ = output.try_send(Message::FilePropertiesDirectoryContentsUpdated(
                request.clone(),
                contents,
            ));
        }
    };

    let join_path = request.path.clone();
    tokio::task::spawn_blocking(move || {
        read_directory_properties_contents(&request.path, cancellation, progress)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| match error {
        FilePropertiesReadError::Cancelled => "operation cancelled".to_owned(),
        FilePropertiesReadError::Io(error) => error.to_string(),
    })
    .map_err(|error| {
        if error.is_empty() {
            format!("could not read directory contents for {:?}", join_path)
        } else {
            error
        }
    })
}

#[cfg(unix)]
fn metadata_properties_permissions(
    metadata: &std::fs::Metadata,
    is_symlink: bool,
) -> Option<FilePropertiesPermissions> {
    (!is_symlink).then(|| FilePropertiesPermissions::from_mode(metadata.permissions().mode()))
}

#[cfg(not(unix))]
fn metadata_properties_permissions(
    _metadata: &std::fs::Metadata,
    _is_symlink: bool,
) -> Option<FilePropertiesPermissions> {
    None
}

#[cfg(unix)]
fn write_file_properties_permissions(
    path: PathBuf,
    permissions: FilePropertiesPermissions,
) -> Result<FilePropertiesPermissions, String> {
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("symbolic link permissions cannot be changed".to_owned());
    }

    let mut fs_permissions = metadata.permissions();
    fs_permissions.set_mode(permissions.mode());
    std::fs::set_permissions(&path, fs_permissions).map_err(|error| error.to_string())?;

    let refreshed = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    Ok(FilePropertiesPermissions::from_mode(
        refreshed.permissions().mode(),
    ))
}

#[cfg(not(unix))]
fn write_file_properties_permissions(
    _path: PathBuf,
    _permissions: FilePropertiesPermissions,
) -> Result<FilePropertiesPermissions, String> {
    Err("permission editing is only available on Unix filesystems".to_owned())
}

fn read_directory_properties_contents(
    path: &Path,
    cancellation: CancellationToken,
    mut progress: impl FnMut(FilePropertiesDirectoryContents),
) -> Result<FilePropertiesDirectoryContents, FilePropertiesReadError> {
    let mut contents = FilePropertiesDirectoryContents {
        file_count: 0,
        directory_count: 0,
        total_size_bytes: 0,
        total_disk_size_bytes: 0,
    };

    if cancellation.is_cancelled() {
        return Err(FilePropertiesReadError::Cancelled);
    }

    let mut processed_entries = 0usize;
    for entry in std::fs::read_dir(path).map_err(FilePropertiesReadError::Io)? {
        if cancellation.is_cancelled() {
            return Err(FilePropertiesReadError::Cancelled);
        }

        let entry = entry.map_err(FilePropertiesReadError::Io)?;
        let file_type = entry.file_type().map_err(FilePropertiesReadError::Io)?;
        let metadata = entry.metadata().map_err(FilePropertiesReadError::Io)?;
        if file_type.is_dir() {
            contents.directory_count += 1;
        } else {
            contents.file_count += 1;
        }
        contents.total_size_bytes = contents.total_size_bytes.saturating_add(metadata.len());
        contents.total_disk_size_bytes = contents
            .total_disk_size_bytes
            .saturating_add(metadata_disk_size(&metadata));
        processed_entries = processed_entries.saturating_add(1);
        if processed_entries % DIRECTORY_CONTENTS_PROGRESS_INTERVAL == 0 {
            progress(contents.clone());
        }
    }

    progress(contents.clone());
    Ok(contents)
}

enum FilePropertiesReadError {
    Cancelled,
    Io(std::io::Error),
}

#[cfg(unix)]
fn metadata_disk_size(metadata: &std::fs::Metadata) -> u64 {
    metadata.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn metadata_disk_size(metadata: &std::fs::Metadata) -> u64 {
    metadata.len()
}
