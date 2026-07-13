use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use file_search::{
    daemon_build_id, default_socket_path, read_service_event, serve_bound_search_socket,
    shutdown_connected_service, shutdown_via_socket, write_service_request, BoundSearchSocket,
    SearchIndexConfig, SearchServiceEvent, SearchServiceRequest, SearchServiceRuntime,
    SearchSocketService, PROTOCOL_VERSION,
};
use tokio::net::UnixStream;
use tokio::signal::unix::{signal, SignalKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonInvocation {
    Serve,
    ShutdownExisting,
}

impl DaemonInvocation {
    fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut arguments = arguments.into_iter();
        let invocation = match arguments.next() {
            None => Self::Serve,
            Some(argument) if argument == "--shutdown-existing" => Self::ShutdownExisting,
            Some(argument) => {
                return Err(format!(
                    "unknown file-searchd argument: {}",
                    argument.to_string_lossy()
                ));
            }
        };
        if let Some(argument) = arguments.next() {
            return Err(format!(
                "unexpected file-searchd argument: {}",
                argument.to_string_lossy()
            ));
        }
        Ok(invocation)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    match DaemonInvocation::parse(std::env::args_os().skip(1))? {
        DaemonInvocation::Serve => serve().await,
        DaemonInvocation::ShutdownExisting => shutdown_existing(&default_socket_path()).await,
    }
}

async fn shutdown_existing(socket_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut peer_stream = match UnixStream::connect(socket_path).await {
        Ok(peer_stream) => peer_stream,
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
    let peer_process_id = peer_stream
        .peer_cred()?
        .pid()
        .ok_or_else(|| io::Error::other("search socket peer has no process ID"))?;
    let executable_path = std::fs::read_link(
        Path::new("/proc")
            .join(peer_process_id.to_string())
            .join("exe"),
    )?;
    if !is_file_searchd_executable_path(&executable_path) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to shut down search socket owner {peer_process_id}: {}",
                executable_path.display()
            ),
        )
        .into());
    }

    write_service_request(&mut peer_stream, &SearchServiceRequest::Version).await?;
    let version_event = read_service_event(&mut peer_stream).await?;
    validate_shutdown_peer_version(version_event)?;

    shutdown_connected_service(peer_stream).await?;
    Ok(())
}

fn validate_shutdown_peer_version(
    version_event: SearchServiceEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    match version_event {
        SearchServiceEvent::Version { protocol, build }
            if protocol == PROTOCOL_VERSION && build == daemon_build_id() =>
        {
            Ok(())
        }
        SearchServiceEvent::Version { protocol, build } => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "refusing to shut down incompatible search service: protocol={protocol}, build={build}"
            ),
        )
        .into()),
        event => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("refusing to shut down search service with unexpected version response: {event:?}"),
        )
        .into()),
    }
}

fn is_file_searchd_executable_path(executable_path: &Path) -> bool {
    matches!(
        executable_path.file_name(),
        Some(file_name)
            if file_name == OsStr::new("file-searchd")
                || file_name == OsStr::new("file-searchd (deleted)")
    )
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminate_signal = signal(SignalKind::terminate())?;
    let database_path = database_path()?;
    let bound_socket = BoundSearchSocket::bind(default_socket_path())?;
    let shutdown_socket_path = bound_socket.path().to_path_buf();
    let service_runtime = Arc::new(SearchServiceRuntime::new());
    service_runtime.start_in_background(database_path, SearchIndexConfig::default());

    eprintln!("file-searchd {} endpoint ready", daemon_build_id());
    let socket_service: Arc<dyn SearchSocketService> = service_runtime.clone();
    let shutdown_runtime = Arc::clone(&service_runtime);
    let socket_server = serve_bound_search_socket(bound_socket, socket_service, move || {
        let shutdown_runtime = Arc::clone(&shutdown_runtime);
        async move { shutdown_runtime.shutdown() }
    });
    let terminate_shutdown = async move {
        terminate_signal.recv().await.ok_or_else(|| {
            std::io::Error::other("SIGTERM signal stream closed before receiving a signal")
        })?;
        shutdown_via_socket(&shutdown_socket_path).await
    };

    tokio::pin!(socket_server);
    tokio::pin!(terminate_shutdown);
    tokio::select! {
        server_outcome = &mut socket_server => server_outcome?,
        signal_outcome = &mut terminate_shutdown => {
            signal_outcome?;
            socket_server.await?;
        }
    }
    Ok(())
}

fn database_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base = dirs::data_dir()
        .or_else(dirs::cache_dir)
        .ok_or("could not find XDG data or cache directory")?;
    Ok(base.join("file-manager").join("search.sqlite"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_defaults_to_serve() {
        assert_eq!(
            DaemonInvocation::parse([]).unwrap(),
            DaemonInvocation::Serve
        );
    }

    #[test]
    fn invocation_accepts_shutdown_existing() {
        assert_eq!(
            DaemonInvocation::parse([OsString::from("--shutdown-existing")]).unwrap(),
            DaemonInvocation::ShutdownExisting
        );
    }

    #[test]
    fn invocation_rejects_unknown_or_extra_arguments() {
        assert!(DaemonInvocation::parse([OsString::from("--unknown")]).is_err());
        assert!(DaemonInvocation::parse([
            OsString::from("--shutdown-existing"),
            OsString::from("extra")
        ])
        .is_err());
    }

    #[test]
    fn owner_path_accepts_file_searchd_and_deleted_executable() {
        assert!(is_file_searchd_executable_path(Path::new(
            "/usr/lib/file-manager/file-searchd"
        )));
        assert!(is_file_searchd_executable_path(Path::new(
            "/usr/lib/file-manager/file-searchd (deleted)"
        )));
    }

    #[test]
    fn owner_path_rejects_unknown_executable() {
        assert!(!is_file_searchd_executable_path(Path::new("/usr/bin/bash")));
        assert!(!is_file_searchd_executable_path(Path::new(
            "/usr/lib/file-manager/file-searchd-old"
        )));
    }

    #[test]
    fn shutdown_requires_an_exact_protocol_and_build_handshake() {
        assert!(validate_shutdown_peer_version(SearchServiceEvent::Version {
            protocol: PROTOCOL_VERSION,
            build: daemon_build_id(),
        })
        .is_ok());
        assert!(validate_shutdown_peer_version(SearchServiceEvent::Version {
            protocol: PROTOCOL_VERSION + 1,
            build: daemon_build_id(),
        })
        .is_err());
        assert!(validate_shutdown_peer_version(SearchServiceEvent::Version {
            protocol: PROTOCOL_VERSION,
            build: "older-build".to_owned(),
        })
        .is_err());
        assert!(validate_shutdown_peer_version(SearchServiceEvent::Status(
            file_search::SearchServiceStatus {
                phase: file_search::SearchServicePhase::Starting,
                query_availability: file_search::IndexedQueryAvailability::Unavailable {
                    message: "starting".to_owned(),
                },
                index_status: None,
            }
        ))
        .is_err());
    }
}
