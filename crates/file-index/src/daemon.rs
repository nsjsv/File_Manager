use std::collections::HashMap;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::io::AsyncReadExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio_util::sync::CancellationToken;

use crate::ipc::{
    read_frame, write_frame, IndexClientError, IndexRequest, IndexRequestCommand, IndexResponse,
    INDEX_PROTOCOL_VERSION,
};
use crate::{
    BuildSelectedPathsRequest, IndexError, IndexMaintenanceHandle, IndexServiceCommand,
    IndexServiceCore, IndexServiceEvent,
};

#[derive(Debug, Clone)]
pub struct IndexDaemonConfig {
    pub socket_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum IndexDaemonError {
    #[error("index daemon I/O failed: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug)]
struct IndexDaemonState {
    cores: AsyncMutex<HashMap<PathBuf, Arc<IndexDaemonCore>>>,
    shutdown: CancellationToken,
}

#[derive(Debug)]
struct IndexDaemonCore {
    service: IndexServiceCore,
    manual_build_queue: AsyncMutex<()>,
    maintenance_handles: Mutex<HashMap<String, IndexMaintenanceHandle>>,
}

pub async fn run(config: IndexDaemonConfig) -> Result<(), IndexDaemonError> {
    let listener = bind_socket(&config.socket_path)?;
    let state = Arc::new(IndexDaemonState::default());

    loop {
        tokio::select! {
            _ = state.shutdown.cancelled() => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(error) = state.handle_connection(stream).await {
                        eprintln!("index daemon connection failed: {error}");
                    }
                });
            }
        }
    }

    drop(listener);
    match std::fs::remove_file(&config.socket_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

impl Default for IndexDaemonState {
    fn default() -> Self {
        Self {
            cores: AsyncMutex::new(HashMap::new()),
            shutdown: CancellationToken::new(),
        }
    }
}

fn bind_socket(socket_path: &Path) -> Result<UnixListener, io::Error> {
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::symlink_metadata(socket_path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            std::fs::remove_file(socket_path)?;
        }
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("socket path is not a Unix socket: {socket_path:?}"),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    UnixListener::bind(socket_path)
}

impl IndexDaemonState {
    async fn handle_connection(&self, mut stream: UnixStream) -> Result<(), IndexClientError> {
        let request: IndexRequest = read_frame(&mut stream).await?;
        if request.version != INDEX_PROTOCOL_VERSION {
            write_frame(
                &mut stream,
                &IndexResponse::ProtocolMismatch {
                    expected: INDEX_PROTOCOL_VERSION,
                    actual: request.version,
                },
            )
            .await?;
            return Ok(());
        }

        if matches!(request.command, IndexRequestCommand::Ping) {
            write_frame(
                &mut stream,
                &IndexResponse::from_event(&IndexServiceEvent::Pong {
                    daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
                }),
            )
            .await?;
            return Ok(());
        }

        if matches!(request.command, IndexRequestCommand::Shutdown) {
            write_frame(
                &mut stream,
                &IndexResponse::from_event(&IndexServiceEvent::Shutdown),
            )
            .await?;
            self.shutdown.cancel();
            return Ok(());
        }

        let core = self.core_for(request.index_base_dir()).await?;
        match request.command {
            IndexRequestCommand::SubscribeMaintenance { profile_id } => {
                let events = core.service.status_stream();
                stream_maintenance_events(profile_id, stream, events).await
            }
            IndexRequestCommand::BuildSelectedPaths(request) => {
                stream_selected_build(core, request.into_domain(), stream).await
            }
            command => {
                let Some(command) = command.into_service_command() else {
                    write_frame(
                        &mut stream,
                        &IndexResponse::Error("unsupported index daemon command".to_owned()),
                    )
                    .await?;
                    return Ok(());
                };
                let response = match core.execute(command).await {
                    Ok(event) => IndexResponse::from_event(&event),
                    Err(error) => IndexResponse::Error(error.to_string()),
                };
                write_frame(&mut stream, &response).await
            }
        }
    }

    async fn core_for(&self, index_base_dir: PathBuf) -> Result<Arc<IndexDaemonCore>, IndexError> {
        let mut cores = self.cores.lock().await;
        if let Some(core) = cores.get(&index_base_dir) {
            return Ok(Arc::clone(core));
        }

        let core = Arc::new(IndexDaemonCore::open(index_base_dir.clone())?);
        cores.insert(index_base_dir, Arc::clone(&core));
        Ok(core)
    }
}

impl IndexDaemonCore {
    fn open(index_base_dir: PathBuf) -> Result<Self, IndexError> {
        let service =
            IndexServiceCore::open(index_base_dir.join("control.sqlite"), index_base_dir)?;
        let core = Self {
            service,
            manual_build_queue: AsyncMutex::new(()),
            maintenance_handles: Mutex::new(HashMap::new()),
        };
        Ok(core)
    }

    async fn execute(&self, command: IndexServiceCommand) -> Result<IndexServiceEvent, IndexError> {
        match command {
            IndexServiceCommand::ConfigureProfile(profile) => {
                self.service
                    .execute(IndexServiceCommand::ConfigureProfile(profile))
                    .await
            }
            IndexServiceCommand::DeleteProfile(profile_id) => {
                let event = self
                    .service
                    .execute(IndexServiceCommand::DeleteProfile(profile_id.clone()))
                    .await?;
                self.stop_profile_maintenance(&profile_id);
                Ok(event)
            }
            IndexServiceCommand::Rebuild { .. } => {
                let _manual_build = self.manual_build_queue.lock().await;
                self.service.execute(command).await
            }
            IndexServiceCommand::StartMaintenance { profile_id } => {
                let event = self
                    .service
                    .execute(IndexServiceCommand::StartMaintenance {
                        profile_id: profile_id.clone(),
                    })
                    .await?;
                self.start_profile_maintenance(&profile_id);
                Ok(event)
            }
            command => self.service.execute(command).await,
        }
    }

    async fn build_selected_paths_with_cancel(
        &self,
        request: BuildSelectedPathsRequest,
        cancel: CancellationToken,
        progress: impl FnMut(crate::FileSearchIndexProgress) + Send + 'static,
    ) -> Result<IndexServiceEvent, IndexError> {
        let _manual_build = self.manual_build_queue.lock().await;
        self.service
            .build_selected_paths_with_cancel(request, cancel, progress)
            .await
    }

    fn start_profile_maintenance(&self, profile_id: &str) {
        let handle = self.service.maintain_profile(profile_id.to_owned());
        let mut handles = self
            .maintenance_handles
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        handles.insert(profile_id.to_owned(), handle);
    }

    fn stop_profile_maintenance(&self, profile_id: &str) {
        let mut handles = self
            .maintenance_handles
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        handles.remove(profile_id);
    }
}

async fn stream_selected_build(
    core: Arc<IndexDaemonCore>,
    request: BuildSelectedPathsRequest,
    stream: UnixStream,
) -> Result<(), IndexClientError> {
    let (progress_sender, mut progress_receiver) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let build = core.build_selected_paths_with_cancel(request, cancel.clone(), move |progress| {
        let _ = progress_sender.send(progress);
    });
    tokio::pin!(build);
    let (mut disconnect_reader, mut response_writer) = stream.into_split();
    let disconnect_cancel = cancel.clone();
    let disconnect_watch = tokio::spawn(async move {
        let mut byte = [0; 1];
        let _ = disconnect_reader.read(&mut byte).await;
        disconnect_cancel.cancel();
    });

    loop {
        tokio::select! {
            progress = progress_receiver.recv() => {
                if let Some(progress) = progress {
                    if let Err(error) = write_frame(&mut response_writer, &IndexResponse::from_progress(progress)).await {
                        disconnect_watch.abort();
                        cancel.cancel();
                        let _ = (&mut build).await;
                        return Err(error);
                    }
                }
            }
            outcome = &mut build => {
                disconnect_watch.abort();
                let response = match outcome {
                    Ok(event) => IndexResponse::from_event(&event),
                    Err(error) => IndexResponse::Error(error.to_string()),
                };
                write_frame(&mut response_writer, &response).await?;
                return Ok(());
            }
        }
    }
}

async fn stream_maintenance_events(
    profile_id: String,
    mut stream: UnixStream,
    mut events: tokio::sync::broadcast::Receiver<IndexServiceEvent>,
) -> Result<(), IndexClientError> {
    loop {
        match events.recv().await {
            Ok(event) if maintenance_event_matches(&event, &profile_id) => {
                write_frame(&mut stream, &IndexResponse::from_event(&event)).await?;
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}

fn maintenance_event_matches(event: &IndexServiceEvent, profile_id: &str) -> bool {
    matches!(
        event,
            IndexServiceEvent::WatchStarted {
                profile_id: event_profile_id,
                ..
            }
            | IndexServiceEvent::MaintenanceStarted {
                profile_id: event_profile_id,
            }
            | IndexServiceEvent::WatchFailed {
                profile_id: event_profile_id,
                ..
            }
            | IndexServiceEvent::IncrementalUpdateStarted {
                profile_id: event_profile_id,
                ..
            }
            | IndexServiceEvent::IncrementalUpdateFinished {
                profile_id: event_profile_id,
                ..
            }
            | IndexServiceEvent::IncrementalUpdateFailed {
                profile_id: event_profile_id,
                ..
            } if event_profile_id == profile_id
    )
}
