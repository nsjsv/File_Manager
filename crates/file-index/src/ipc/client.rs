use std::path::{Path, PathBuf};
use std::{env, io};

use tokio::net::UnixStream;
use tokio_util::sync::CancellationToken;

use crate::service::{IndexServiceCommand, IndexServiceEvent};
use crate::{FileSearchIndexProgress, IndexError};

use super::framing::{read_frame, write_frame};
use super::wire::{IndexRequest, IndexResponse};

const SOCKET_FILE_NAME: &str = "file-indexd.sock";

#[derive(Debug, Clone)]
pub struct IndexClient {
    socket_path: PathBuf,
    index_base_dir: PathBuf,
}

#[derive(Debug)]
pub struct IndexMaintenanceSubscription {
    stream: UnixStream,
}

#[derive(Debug, thiserror::Error)]
pub enum IndexClientError {
    #[error("could not connect to index daemon {path:?}: {source}")]
    Connect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("index IPC I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("index IPC codec failed: {0}")]
    Codec(#[from] bincode::Error),
    #[error("index IPC protocol error: {0}")]
    Protocol(String),
    #[error("index IPC protocol version mismatch: expected {expected}, got {actual}")]
    ProtocolMismatch { expected: u16, actual: u16 },
    #[error("{0}")]
    Service(String),
}

impl IndexClient {
    pub fn new(index_base_dir: impl Into<PathBuf>, socket_path: impl Into<PathBuf>) -> Self {
        Self {
            index_base_dir: index_base_dir.into(),
            socket_path: socket_path.into(),
        }
    }

    pub fn for_index_base_dir(index_base_dir: impl Into<PathBuf>) -> Self {
        Self::new(index_base_dir, default_socket_path())
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn index_base_dir(&self) -> &Path {
        &self.index_base_dir
    }

    pub async fn execute(
        &self,
        command: IndexServiceCommand,
    ) -> Result<IndexServiceEvent, IndexClientError> {
        self.execute_with_progress(command, |_| {}).await
    }

    pub async fn execute_with_cancel(
        &self,
        command: IndexServiceCommand,
        cancel: CancellationToken,
    ) -> Result<IndexServiceEvent, IndexClientError> {
        self.execute_with_progress_and_cancel(command, cancel, |_| {})
            .await
    }

    pub async fn subscribe_maintenance(
        &self,
        profile_id: impl Into<String>,
    ) -> Result<IndexMaintenanceSubscription, IndexClientError> {
        let mut stream = self.connect().await?;
        write_frame(
            &mut stream,
            &IndexRequest::subscribe_maintenance(&self.index_base_dir, profile_id),
        )
        .await?;
        Ok(IndexMaintenanceSubscription { stream })
    }

    pub async fn build_selected_paths_with_progress(
        &self,
        request: crate::BuildSelectedPathsRequest,
        progress: impl FnMut(FileSearchIndexProgress) + Send,
    ) -> Result<IndexServiceEvent, IndexClientError> {
        self.execute_with_progress(IndexServiceCommand::BuildSelectedPaths(request), progress)
            .await
    }

    pub async fn build_selected_paths_with_progress_and_cancel(
        &self,
        request: crate::BuildSelectedPathsRequest,
        cancel: CancellationToken,
        progress: impl FnMut(FileSearchIndexProgress) + Send,
    ) -> Result<IndexServiceEvent, IndexClientError> {
        self.execute_with_progress_and_cancel(
            IndexServiceCommand::BuildSelectedPaths(request),
            cancel,
            progress,
        )
        .await
    }

    async fn execute_with_progress(
        &self,
        command: IndexServiceCommand,
        mut progress: impl FnMut(FileSearchIndexProgress) + Send,
    ) -> Result<IndexServiceEvent, IndexClientError> {
        let mut stream = self.connect().await?;
        write_frame(
            &mut stream,
            &IndexRequest::from_command(&self.index_base_dir, command),
        )
        .await?;
        loop {
            match read_frame(&mut stream).await? {
                IndexResponse::Event(event) => return Ok(event.into_domain()),
                IndexResponse::Progress(update) => progress(update.into()),
                IndexResponse::Error(message) => return Err(IndexClientError::Service(message)),
                IndexResponse::ProtocolMismatch { expected, actual } => {
                    return Err(IndexClientError::ProtocolMismatch { expected, actual });
                }
            }
        }
    }

    async fn execute_with_progress_and_cancel(
        &self,
        command: IndexServiceCommand,
        cancel: CancellationToken,
        mut progress: impl FnMut(FileSearchIndexProgress) + Send,
    ) -> Result<IndexServiceEvent, IndexClientError> {
        let mut stream = self.connect().await?;
        write_frame(
            &mut stream,
            &IndexRequest::from_command(&self.index_base_dir, command),
        )
        .await?;
        loop {
            let response = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(IndexClientError::Service(IndexError::Cancelled.to_string()));
                }
                response = read_frame(&mut stream) => response?,
            };
            match response {
                IndexResponse::Event(event) => return Ok(event.into_domain()),
                IndexResponse::Progress(update) => progress(update.into()),
                IndexResponse::Error(message) => return Err(IndexClientError::Service(message)),
                IndexResponse::ProtocolMismatch { expected, actual } => {
                    return Err(IndexClientError::ProtocolMismatch { expected, actual });
                }
            }
        }
    }

    async fn connect(&self) -> Result<UnixStream, IndexClientError> {
        UnixStream::connect(&self.socket_path)
            .await
            .map_err(|source| IndexClientError::Connect {
                path: self.socket_path.clone(),
                source,
            })
    }
}

impl IndexMaintenanceSubscription {
    pub async fn next_event(&mut self) -> Result<Option<IndexServiceEvent>, IndexClientError> {
        let response = match read_frame(&mut self.stream).await {
            Ok(response) => response,
            Err(IndexClientError::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        match response {
            IndexResponse::Event(event) => Ok(Some(event.into_domain())),
            IndexResponse::Error(message) => Err(IndexClientError::Service(message)),
            IndexResponse::ProtocolMismatch { expected, actual } => {
                Err(IndexClientError::ProtocolMismatch { expected, actual })
            }
            IndexResponse::Progress(_) => Err(IndexClientError::Protocol(
                "index daemon sent progress on a maintenance stream".to_owned(),
            )),
        }
    }
}

pub fn default_socket_path() -> PathBuf {
    match env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime_dir) => PathBuf::from(runtime_dir).join(SOCKET_FILE_NAME),
        None => env::temp_dir().join(SOCKET_FILE_NAME),
    }
}

impl From<IndexError> for IndexClientError {
    fn from(error: IndexError) -> Self {
        Self::Service(error.to_string())
    }
}
