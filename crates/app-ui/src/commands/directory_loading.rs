use std::path::PathBuf;

use file_core::{scan_directory_with_progress, DirectoryScanBatch, FileError, ScanOptions};
use iced::futures::channel::mpsc::Sender as IcedSender;
use iced::futures::SinkExt;
use iced::Task;
use tokio_util::sync::CancellationToken;

use crate::model::{
    DirectoryLoadFailure, DirectoryLoadRequest, ExpandedDirectoryLoadRequest, Message,
};
use crate::startup_trace;

const DIRECTORY_LOAD_CHANNEL_SIZE: usize = 8;

pub(crate) fn load_directory_command(
    request: DirectoryLoadRequest,
    options: ScanOptions,
    cancellation: CancellationToken,
) -> Task<Message> {
    Task::stream(iced::stream::channel(
        DIRECTORY_LOAD_CHANNEL_SIZE,
        async move |mut output| {
            let scan = load_directory_with_batches(
                request.path.clone(),
                options,
                cancellation,
                |batch| Message::DirectoryLoadBatch(request.clone(), batch),
                &mut output,
            )
            .await;
            let _ = output.send(Message::Loaded(request, scan)).await;
        },
    ))
}

pub(crate) fn load_expanded_directory_command(
    request: ExpandedDirectoryLoadRequest,
    options: ScanOptions,
    cancellation: CancellationToken,
) -> Task<Message> {
    Task::stream(iced::stream::channel(
        DIRECTORY_LOAD_CHANNEL_SIZE,
        async move |mut output| {
            let scan = load_directory_with_batches(
                request.path.clone(),
                options,
                cancellation,
                |batch| Message::ExpandedDirectoryLoadBatch(request.clone(), batch),
                &mut output,
            )
            .await;
            let _ = output
                .send(Message::ExpandedDirectoryLoaded(request, scan))
                .await;
        },
    ))
}

async fn load_directory_with_batches(
    path: PathBuf,
    options: ScanOptions,
    cancellation: CancellationToken,
    mut message_for_batch: impl FnMut(DirectoryScanBatch) -> Message,
    output: &mut IcedSender<Message>,
) -> Result<file_core::DirectoryScan, DirectoryLoadFailure> {
    startup_trace::mark_once("initial_directory_scan_started");
    let scan_outcome = scan_directory_with_progress(path, options, cancellation, |batch| {
        let _ = output.try_send(message_for_batch(batch));
    })
    .await
    .map_err(classify_directory_load_failure);
    startup_trace::mark_once("initial_directory_scan_finished");
    scan_outcome
}

fn classify_directory_load_failure(file_error: FileError) -> DirectoryLoadFailure {
    let directory_is_unavailable = matches!(
        &file_error,
        FileError::ReadDirectory { source, .. } | FileError::ReadEntry { source, .. }
            if matches!(
                source.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            )
    );
    let message = file_error.to_string();

    if directory_is_unavailable {
        DirectoryLoadFailure::DirectoryUnavailable { message }
    } else {
        DirectoryLoadFailure::ReadFailed { message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_directory_is_classified_as_unavailable() {
        let missing_path = PathBuf::from("/workspace/removed");
        let failure = classify_directory_load_failure(FileError::ReadDirectory {
            path: missing_path,
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        });

        assert!(matches!(
            failure,
            DirectoryLoadFailure::DirectoryUnavailable { .. }
        ));
    }

    #[test]
    fn permission_denial_remains_a_visible_read_failure() {
        let locked_path = PathBuf::from("/workspace/locked");
        let failure = classify_directory_load_failure(FileError::ReadDirectory {
            path: locked_path,
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        });

        assert!(matches!(failure, DirectoryLoadFailure::ReadFailed { .. }));
    }
}
