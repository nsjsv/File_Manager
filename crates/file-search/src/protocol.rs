use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::daemon::SearchDaemonCore;
use crate::database::SearchDatabase;
use crate::error::{SearchError, SearchResult};
use crate::model::{
    daemon_build_id, IndexStatus, IndexedQueryAvailability, SearchProviderFailure, SearchQuery,
    SearchResultBatch, SearchServiceEvent, SearchServicePhase, SearchServiceRequest,
    SearchServiceStatus, PROTOCOL_VERSION,
};

const MAX_REQUEST_FRAME_BYTES: u32 = 64 * 1024;
const MAX_EVENT_FRAME_BYTES: u32 = 2_500_000;
const MAX_ACTIVE_CLIENTS: usize = 8;
const CLIENT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_QUERY_TERMS_BYTES: usize = 4_096;
const MAX_QUERY_PATH_BYTES: usize = 4_096;
const MAX_QUERY_MIME_BYTES: usize = 255;
const MAX_QUERY_OFFSET: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SocketIdentity {
    device: u64,
    inode: u64,
}

impl SocketIdentity {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

/// 同时持有进程锁、listener 和路径身份，确保只有真实 owner 能删除 socket。
pub struct BoundSearchSocket {
    listener: Option<StdUnixListener>,
    socket_path: PathBuf,
    socket_identity: SocketIdentity,
    _instance_lock: File,
}

impl BoundSearchSocket {
    pub fn bind(socket_path: PathBuf) -> SearchResult<Self> {
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| SearchError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let instance_lock = acquire_socket_lock(&socket_path)?;
        reclaim_stale_socket(&socket_path)?;
        let listener = StdUnixListener::bind(&socket_path).map_err(|source| SearchError::Io {
            path: socket_path.clone(),
            source,
        })?;
        let metadata = match (|| -> SearchResult<std::fs::Metadata> {
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
                .map_err(|source| SearchError::Io {
                    path: socket_path.clone(),
                    source,
                })?;
            std::fs::symlink_metadata(&socket_path).map_err(|source| SearchError::Io {
                path: socket_path.clone(),
                source,
            })
        })() {
            Ok(metadata) => metadata,
            Err(error) => {
                drop(listener);
                let _ = std::fs::remove_file(&socket_path);
                return Err(error);
            }
        };

        Ok(Self {
            listener: Some(listener),
            socket_path,
            socket_identity: SocketIdentity::from_metadata(&metadata),
            _instance_lock: instance_lock,
        })
    }

    pub fn path(&self) -> &Path {
        &self.socket_path
    }

    fn take_tokio_listener(&mut self) -> SearchResult<UnixListener> {
        let listener = self.listener.take().expect("bound listener already taken");
        listener
            .set_nonblocking(true)
            .map_err(|source| SearchError::Io {
                path: self.socket_path.clone(),
                source,
            })?;
        UnixListener::from_std(listener).map_err(|source| SearchError::Io {
            path: self.socket_path.clone(),
            source,
        })
    }

    fn remove_owned_socket(&self) -> SearchResult<()> {
        let metadata = match std::fs::symlink_metadata(&self.socket_path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(SearchError::Io {
                    path: self.socket_path.clone(),
                    source,
                });
            }
        };
        if SocketIdentity::from_metadata(&metadata) != self.socket_identity {
            return Ok(());
        }
        std::fs::remove_file(&self.socket_path).map_err(|source| SearchError::Io {
            path: self.socket_path.clone(),
            source,
        })
    }
}

impl Drop for BoundSearchSocket {
    fn drop(&mut self) {
        let _ = self.remove_owned_socket();
    }
}

fn acquire_socket_lock(socket_path: &Path) -> SearchResult<File> {
    let lock_path = socket_path.with_extension("lock");
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&lock_path)
        .map_err(|source| SearchError::Io {
            path: lock_path.clone(),
            source,
        })?;
    lock_file
        .set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|source| SearchError::Io {
            path: lock_path.clone(),
            source,
        })?;

    // advisory lock 会在进程异常退出时由内核释放，不会留下无法回收的逻辑 owner。
    let lock_status = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if lock_status == 0 {
        return Ok(lock_file);
    }
    let source = io::Error::last_os_error();
    if source.kind() == io::ErrorKind::WouldBlock {
        return Err(SearchError::SocketAlreadyOwned {
            path: socket_path.to_path_buf(),
        });
    }
    Err(SearchError::Io {
        path: lock_path,
        source,
    })
}

fn reclaim_stale_socket(socket_path: &Path) -> SearchResult<()> {
    let stale_metadata = match std::fs::symlink_metadata(socket_path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(SearchError::Io {
                path: socket_path.to_path_buf(),
                source,
            });
        }
    };
    if !stale_metadata.file_type().is_socket() {
        return Err(SearchError::Io {
            path: socket_path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "search socket path is not a Unix socket",
            ),
        });
    }

    match std::os::unix::net::UnixStream::connect(socket_path) {
        Ok(_) => {
            return Err(SearchError::SocketAlreadyOwned {
                path: socket_path.to_path_buf(),
            });
        }
        Err(source)
            if matches!(
                source.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) => {}
        Err(source) => {
            return Err(SearchError::Io {
                path: socket_path.to_path_buf(),
                source,
            });
        }
    }

    // connect 与 unlink 之间再次核对身份，避免删除刚替换到同一路径的新 peer。
    let current_metadata = match std::fs::symlink_metadata(socket_path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(SearchError::Io {
                path: socket_path.to_path_buf(),
                source,
            });
        }
    };
    if SocketIdentity::from_metadata(&current_metadata)
        != SocketIdentity::from_metadata(&stale_metadata)
    {
        return Err(SearchError::SocketAlreadyOwned {
            path: socket_path.to_path_buf(),
        });
    }
    std::fs::remove_file(socket_path).map_err(|source| SearchError::Io {
        path: socket_path.to_path_buf(),
        source,
    })
}

pub trait SearchSocketService: Send + Sync + 'static {
    fn status(&self) -> SearchServiceStatus;

    fn search(&self, query: &SearchQuery) -> Result<SearchResultBatch, SearchProviderFailure>;
}

#[derive(Clone)]
enum SearchServiceBackend {
    DirectDatabase {
        database_path: PathBuf,
        status: Arc<Mutex<IndexStatus>>,
    },
    DaemonCore(Arc<SearchDaemonCore>),
    Runtime(Arc<dyn SearchSocketService>),
}

pub fn default_socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("file-manager-search.sock")
}

pub async fn search_via_socket(
    socket_path: &Path,
    query: SearchQuery,
) -> SearchResult<SearchResultBatch> {
    let mut stream = UnixStream::connect(socket_path).await?;
    write_service_request(&mut stream, &SearchServiceRequest::Search(query.clone())).await?;
    loop {
        match read_service_event(&mut stream).await? {
            SearchServiceEvent::Results(batch) if batch.query_id == query.query_id => {
                return Ok(batch);
            }
            SearchServiceEvent::SearchFailed { query_id, failure }
                if query_id == query.query_id =>
            {
                return Err(SearchError::SearchFailed { query_id, failure });
            }
            _ => {}
        }
    }
}

pub async fn status_via_socket(socket_path: &Path) -> SearchResult<SearchServiceStatus> {
    let mut stream = UnixStream::connect(socket_path).await?;
    write_service_request(&mut stream, &SearchServiceRequest::Status).await?;
    loop {
        match read_service_event(&mut stream).await? {
            SearchServiceEvent::Status(status) => return Ok(status),
            _ => {}
        }
    }
}

/// 客户端用版本握手识别并退休旧应用遗留的 daemon。
pub async fn version_via_socket(socket_path: &Path) -> SearchResult<(u32, String)> {
    let mut stream = UnixStream::connect(socket_path).await?;
    write_service_request(&mut stream, &SearchServiceRequest::Version).await?;
    loop {
        match read_service_event(&mut stream).await? {
            SearchServiceEvent::Version { protocol, build } => return Ok((protocol, build)),
            _ => {}
        }
    }
}

/// socket 不存在或拒绝连接表示没有 daemon 正在监听；其它连接错误必须保留。
pub async fn shutdown_via_socket(socket_path: &Path) -> SearchResult<()> {
    let stream = match UnixStream::connect(socket_path).await {
        Ok(stream) => stream,
        Err(source)
            if matches!(
                source.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(());
        }
        Err(source) => return Err(source.into()),
    };
    shutdown_connected_service(stream).await
}

pub async fn shutdown_connected_service(mut stream: UnixStream) -> SearchResult<()> {
    write_service_request(&mut stream, &SearchServiceRequest::Shutdown).await?;
    // server 在自有资源清理完成后才关闭连接，EOF 因而是可等待的完成边界。
    let mut buffer = [0_u8; 1];
    let _ = stream.read(&mut buffer).await;
    Ok(())
}

pub async fn serve_search_socket(
    socket_path: PathBuf,
    database_path: PathBuf,
    status: IndexStatus,
) -> SearchResult<()> {
    serve_search_socket_with_status(
        socket_path,
        database_path,
        Arc::new(Mutex::new(status)),
        || async { Ok(()) },
    )
    .await
}

pub async fn serve_search_socket_with_status<ShutdownFuture, ShutdownHandler>(
    socket_path: PathBuf,
    database_path: PathBuf,
    status: Arc<Mutex<IndexStatus>>,
    on_shutdown: ShutdownHandler,
) -> SearchResult<()>
where
    ShutdownFuture: Future<Output = SearchResult<()>> + Send,
    ShutdownHandler: FnOnce() -> ShutdownFuture + Send,
{
    let bound_socket = BoundSearchSocket::bind(socket_path)?;
    serve_search_socket_with_backend(
        bound_socket,
        SearchServiceBackend::DirectDatabase {
            database_path,
            status,
        },
        on_shutdown,
    )
    .await
}

pub async fn serve_search_socket_with_core<ShutdownFuture, ShutdownHandler>(
    socket_path: PathBuf,
    daemon_core: Arc<SearchDaemonCore>,
    on_shutdown: ShutdownHandler,
) -> SearchResult<()>
where
    ShutdownFuture: Future<Output = SearchResult<()>> + Send,
    ShutdownHandler: FnOnce() -> ShutdownFuture + Send,
{
    let bound_socket = BoundSearchSocket::bind(socket_path)?;
    serve_search_socket_with_backend(
        bound_socket,
        SearchServiceBackend::DaemonCore(daemon_core),
        on_shutdown,
    )
    .await
}

pub async fn serve_bound_search_socket<ShutdownFuture, ShutdownHandler>(
    bound_socket: BoundSearchSocket,
    service: Arc<dyn SearchSocketService>,
    on_shutdown: ShutdownHandler,
) -> SearchResult<()>
where
    ShutdownFuture: Future<Output = SearchResult<()>> + Send,
    ShutdownHandler: FnOnce() -> ShutdownFuture + Send,
{
    serve_search_socket_with_backend(
        bound_socket,
        SearchServiceBackend::Runtime(service),
        on_shutdown,
    )
    .await
}

async fn serve_search_socket_with_backend<ShutdownFuture, ShutdownHandler>(
    mut bound_socket: BoundSearchSocket,
    backend: SearchServiceBackend,
    on_shutdown: ShutdownHandler,
) -> SearchResult<()>
where
    ShutdownFuture: Future<Output = SearchResult<()>> + Send,
    ShutdownHandler: FnOnce() -> ShutdownFuture + Send,
{
    let listener = bound_socket.take_tokio_listener()?;
    let (shutdown_requested_sender, mut shutdown_requested_receiver) = watch::channel(false);
    let (shutdown_finished_sender, _) = watch::channel(false);
    let mut client_tasks = JoinSet::new();

    let accept_outcome = loop {
        tokio::select! {
            biased;
            changed = shutdown_requested_receiver.changed() => {
                if changed.is_err() || *shutdown_requested_receiver.borrow_and_update() {
                    break Ok(());
                }
            }
            accepted = listener.accept(), if client_tasks.len() < MAX_ACTIVE_CLIENTS => {
                let (stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(source) => {
                        break Err(SearchError::Io {
                            path: bound_socket.socket_path.clone(),
                            source,
                        });
                    }
                };
                client_tasks.spawn(handle_client(
                    stream,
                    backend.clone(),
                    shutdown_requested_sender.clone(),
                    shutdown_requested_sender.subscribe(),
                    shutdown_finished_sender.subscribe(),
                ));
            }
            joined = client_tasks.join_next(), if !client_tasks.is_empty() => {
                let _ = joined;
            }
        };
    };

    shutdown_requested_sender.send_replace(true);
    drop(listener);
    let shutdown_outcome = on_shutdown().await;
    let socket_cleanup_outcome = bound_socket.remove_owned_socket();
    shutdown_finished_sender.send_replace(true);
    client_tasks.abort_all();
    while client_tasks.join_next().await.is_some() {}

    accept_outcome?;
    shutdown_outcome?;
    socket_cleanup_outcome
}

async fn handle_client(
    mut stream: UnixStream,
    backend: SearchServiceBackend,
    shutdown_requested_sender: watch::Sender<bool>,
    shutdown_requested_receiver: watch::Receiver<bool>,
    mut shutdown_finished_receiver: watch::Receiver<bool>,
) -> SearchResult<()> {
    let mut client_database: Option<Result<SearchDatabase, String>> = None;
    loop {
        let request_outcome = tokio::select! {
            request = read_service_request_before(&mut stream, CLIENT_IDLE_TIMEOUT) => request,
            _ = wait_for_signal(&mut shutdown_finished_receiver) => return Ok(()),
        };
        let request = match request_outcome {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(SearchError::ProtocolIo(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        match request {
            SearchServiceRequest::Status => {
                let status = service_status(&backend);
                write_service_event(&mut stream, &SearchServiceEvent::Status(status)).await?;
            }
            SearchServiceRequest::Search(query) => {
                let query_id = query.query_id;
                let search_outcome = if let Err(message) = validate_wire_query(&query) {
                    Err(SearchProviderFailure::InvalidQuery { message })
                } else if *shutdown_requested_receiver.borrow() {
                    Err(SearchProviderFailure::Unavailable {
                        message: "search service is shutting down".to_owned(),
                    })
                } else {
                    let (returned_database, search_outcome) = search_indexed_backend_off_reactor(
                        backend.clone(),
                        client_database.take(),
                        query,
                    )
                    .await?;
                    client_database = returned_database;
                    search_outcome
                };
                let event = match search_outcome {
                    Ok(batch) => SearchServiceEvent::Results(batch),
                    Err(failure) => SearchServiceEvent::SearchFailed { query_id, failure },
                };
                write_service_event(&mut stream, &event).await?;
            }
            SearchServiceRequest::Cancel { query_id } => {
                write_service_event(&mut stream, &SearchServiceEvent::Cancelled { query_id })
                    .await?;
            }
            SearchServiceRequest::Version => {
                write_service_event(
                    &mut stream,
                    &SearchServiceEvent::Version {
                        protocol: PROTOCOL_VERSION,
                        build: daemon_build_id(),
                    },
                )
                .await?;
            }
            SearchServiceRequest::Shutdown => {
                shutdown_requested_sender.send_replace(true);
                wait_for_signal(&mut shutdown_finished_receiver).await;
                return Ok(());
            }
        }
    }
}

fn validate_wire_query(query: &SearchQuery) -> Result<(), String> {
    if query.terms.len() > MAX_QUERY_TERMS_BYTES {
        return Err(format!(
            "search terms exceed the {MAX_QUERY_TERMS_BYTES} byte limit"
        ));
    }
    if let crate::model::SearchScope::Directory(path) = &query.scope {
        if path.as_os_str().as_encoded_bytes().len() > MAX_QUERY_PATH_BYTES {
            return Err(format!(
                "search scope exceeds the {MAX_QUERY_PATH_BYTES} byte path limit"
            ));
        }
    }
    if query
        .filters
        .mime_type
        .as_ref()
        .is_some_and(|mime_type| mime_type.len() > MAX_QUERY_MIME_BYTES)
    {
        return Err(format!(
            "search MIME filter exceeds the {MAX_QUERY_MIME_BYTES} byte limit"
        ));
    }
    if !(1..=200).contains(&query.limit) {
        return Err("search result limit must be between 1 and 200".to_owned());
    }
    if query
        .cursor
        .is_some_and(|cursor| cursor.offset > MAX_QUERY_OFFSET)
    {
        return Err(format!(
            "search cursor exceeds the {MAX_QUERY_OFFSET} row offset limit"
        ));
    }
    for range in [
        query.filters.modified,
        query.filters.accessed,
        query.filters.created,
    ]
    .into_iter()
    .flatten()
    {
        if range.start_ms > range.end_ms {
            return Err("search time range starts after it ends".to_owned());
        }
    }
    Ok(())
}

async fn read_service_request_before(
    reader: &mut (impl AsyncRead + Unpin),
    idle_timeout: Duration,
) -> SearchResult<Option<SearchServiceRequest>> {
    match tokio::time::timeout(idle_timeout, read_service_request(reader)).await {
        Ok(request) => request.map(Some),
        Err(_) => Ok(None),
    }
}

fn service_status(backend: &SearchServiceBackend) -> SearchServiceStatus {
    match backend {
        SearchServiceBackend::DirectDatabase { status, .. } => {
            available_service_status(status.lock().expect("search status mutex poisoned").clone())
        }
        SearchServiceBackend::DaemonCore(daemon_core) => {
            available_service_status(daemon_core.current_status())
        }
        SearchServiceBackend::Runtime(service) => service.status(),
    }
}

fn available_service_status(index_status: IndexStatus) -> SearchServiceStatus {
    SearchServiceStatus {
        phase: SearchServicePhase::Ready,
        query_availability: IndexedQueryAvailability::Available,
        index_status: Some(index_status),
    }
}

fn search_indexed_backend(
    backend: &SearchServiceBackend,
    client_database: &mut Option<Result<SearchDatabase, String>>,
    query: &SearchQuery,
) -> Result<SearchResultBatch, SearchProviderFailure> {
    match backend {
        SearchServiceBackend::DirectDatabase { database_path, .. } => {
            if client_database.is_none() {
                *client_database = Some(
                    SearchDatabase::open_read_only(database_path)
                        .map_err(|error| error.to_string()),
                );
            }
            match client_database.as_ref().expect("reader state initialized") {
                Ok(database) => database
                    .search(query)
                    .map_err(search_provider_failure_from_error),
                Err(message) => Err(SearchProviderFailure::Fatal {
                    message: message.clone(),
                }),
            }
        }
        SearchServiceBackend::DaemonCore(daemon_core) => daemon_core
            .search(query)
            .map_err(search_provider_failure_from_error),
        SearchServiceBackend::Runtime(service) => service.search(query),
    }
}

async fn search_indexed_backend_off_reactor(
    backend: SearchServiceBackend,
    mut client_database: Option<Result<SearchDatabase, String>>,
    query: SearchQuery,
) -> SearchResult<(
    Option<Result<SearchDatabase, String>>,
    Result<SearchResultBatch, SearchProviderFailure>,
)> {
    tokio::task::spawn_blocking(move || {
        let search_outcome = search_indexed_backend(&backend, &mut client_database, &query);
        (client_database, search_outcome)
    })
    .await
    .map_err(|error| SearchError::WorkerFailed(format!("search worker failed: {error}")))
}

fn search_provider_failure_from_error(error: SearchError) -> SearchProviderFailure {
    match error {
        SearchError::InvalidQuery(message) => SearchProviderFailure::InvalidQuery { message },
        SearchError::SearchFailed { failure, .. } => failure,
        error => SearchProviderFailure::Fatal {
            message: error.to_string(),
        },
    }
}

async fn wait_for_signal(receiver: &mut watch::Receiver<bool>) {
    loop {
        if *receiver.borrow_and_update() {
            return;
        }
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

pub async fn read_service_request(
    reader: &mut (impl AsyncRead + Unpin),
) -> SearchResult<SearchServiceRequest> {
    read_frame(reader, MAX_REQUEST_FRAME_BYTES).await
}

pub async fn write_service_request(
    writer: &mut (impl AsyncWrite + Unpin),
    request: &SearchServiceRequest,
) -> SearchResult<()> {
    write_frame(writer, request, MAX_REQUEST_FRAME_BYTES).await
}

pub async fn read_service_event(
    reader: &mut (impl AsyncRead + Unpin),
) -> SearchResult<SearchServiceEvent> {
    read_frame(reader, MAX_EVENT_FRAME_BYTES).await
}

pub async fn write_service_event(
    writer: &mut (impl AsyncWrite + Unpin),
    event: &SearchServiceEvent,
) -> SearchResult<()> {
    write_frame(writer, event, MAX_EVENT_FRAME_BYTES).await
}

async fn read_frame<T: DeserializeOwned>(
    reader: &mut (impl AsyncRead + Unpin),
    max_frame_bytes: u32,
) -> SearchResult<T> {
    let len = reader.read_u32().await?;
    if len > max_frame_bytes {
        return Err(SearchError::ProtocolFrameTooLarge(len));
    }
    let mut bytes = vec![0; len as usize];
    reader.read_exact(&mut bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn write_frame<T: Serialize>(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &T,
    max_frame_bytes: u32,
) -> SearchResult<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > max_frame_bytes as usize {
        return Err(SearchError::ProtocolFrameTooLarge(bytes.len() as u32));
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
