use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio_util::sync::CancellationToken;

use crate::ipc::{
    read_frame, write_frame, IndexClientError, IndexRequest, IndexRequestCommand, IndexResponse,
    INDEX_PROTOCOL_VERSION,
};
use crate::{
    BuildSelectedPathsRequest, IndexError, IndexServiceCommand, IndexServiceCore,
    IndexServiceEvent, SearchQuery,
};

const FLOCK_EXCLUSIVE: i32 = 2;
const FLOCK_NONBLOCKING: i32 = 4;

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

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
}

#[derive(Debug)]
struct DaemonInstanceLock {
    _file: File,
}

pub async fn run(config: IndexDaemonConfig) -> Result<(), IndexDaemonError> {
    let _instance_lock = DaemonInstanceLock::acquire(&config.socket_path)?;
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
    match fs::remove_file(&config.socket_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

impl DaemonInstanceLock {
    fn acquire(socket_path: &Path) -> Result<Self, io::Error> {
        let lock_path = socket_path.with_extension("lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(lock_path)?;
        let lock_result = unsafe { flock(file.as_raw_fd(), FLOCK_EXCLUSIVE | FLOCK_NONBLOCKING) };
        if lock_result == 0 {
            return Ok(Self { _file: file });
        }

        let error = io::Error::last_os_error();
        Err(if error.kind() == io::ErrorKind::WouldBlock {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("index daemon is already running for socket {socket_path:?}"),
            )
        } else {
            error
        })
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
        fs::create_dir_all(parent)?;
    }
    match std::fs::symlink_metadata(socket_path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            if socket_has_listener(socket_path) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("index daemon is already running at socket {socket_path:?}"),
                ));
            }
            fs::remove_file(socket_path)?;
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

fn socket_has_listener(socket_path: &Path) -> bool {
    StdUnixStream::connect(socket_path).is_ok()
}

impl IndexDaemonState {
    async fn handle_connection(&self, mut stream: UnixStream) -> Result<(), IndexClientError> {
        let request: IndexRequest = read_frame(&mut stream).await?;
        let request_index_base_dir = request.index_base_dir();
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

        let core = self.core_for(request_index_base_dir).await?;
        match request.command {
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
                if let IndexServiceCommand::Query(query) = command {
                    return stream_query(core, query, stream).await;
                }
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
        Ok(Self {
            service: IndexServiceCore::open(index_base_dir.join("control.sqlite"), index_base_dir)?,
            manual_build_queue: AsyncMutex::new(()),
        })
    }

    async fn execute(&self, command: IndexServiceCommand) -> Result<IndexServiceEvent, IndexError> {
        match command {
            IndexServiceCommand::Rebuild { .. } => {
                let _manual_build = self.manual_build_queue.lock().await;
                self.service.execute(command).await
            }
            command => self.service.execute(command).await,
        }
    }

    async fn query_with_cancel(
        &self,
        query: SearchQuery,
        cancel: CancellationToken,
    ) -> Result<IndexServiceEvent, IndexError> {
        self.service.query_with_cancel(query, cancel).await
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
}

async fn stream_query(
    core: Arc<IndexDaemonCore>,
    query: SearchQuery,
    stream: UnixStream,
) -> Result<(), IndexClientError> {
    let cancel = CancellationToken::new();
    let query = core.query_with_cancel(query, cancel.clone());
    tokio::pin!(query);
    let (mut disconnect_reader, mut response_writer) = stream.into_split();
    let disconnect_cancel = cancel.clone();
    let disconnect_watch = tokio::spawn(async move {
        let mut byte = [0; 1];
        let _ = disconnect_reader.read(&mut byte).await;
        disconnect_cancel.cancel();
    });

    let response = match query.await {
        Ok(event) => IndexResponse::from_event(&event),
        Err(error) => IndexResponse::Error(error.to_string()),
    };
    disconnect_watch.abort();
    write_frame(&mut response_writer, &response).await
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

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener as StdUnixListener;

    use super::*;

    #[test]
    fn bind_socket_refuses_live_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("file-indexd.sock");
        let existing_listener = StdUnixListener::bind(&socket_path).unwrap();

        let error = match bind_socket(&socket_path) {
            Ok(_) => panic!("bind_socket replaced a live socket"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        drop(existing_listener);
    }

    #[tokio::test]
    async fn bind_socket_replaces_stale_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("file-indexd.sock");
        let stale_listener = StdUnixListener::bind(&socket_path).unwrap();
        drop(stale_listener);

        let listener = bind_socket(&socket_path).unwrap();
        let client = tokio::net::UnixStream::connect(&socket_path).await;

        assert!(client.is_ok(), "new listener should accept connections");
        drop(listener);
    }
}
