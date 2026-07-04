use std::ffi::OsString;
use std::path::PathBuf;

use file_index::daemon::{run, IndexDaemonConfig};
use file_index::ipc::default_socket_path;
use file_index::{IndexClient, IndexServiceCommand, IndexServiceEvent};

#[derive(Debug)]
enum FileIndexdCommand {
    Run { socket_path: PathBuf },
    Shutdown { socket_path: PathBuf },
    Help,
}

#[tokio::main]
async fn main() {
    let command = match parse_command(std::env::args_os().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            std::process::exit(2);
        }
    };

    match command {
        FileIndexdCommand::Run { socket_path } => {
            if let Err(error) = run(IndexDaemonConfig { socket_path }).await {
                eprintln!("file-indexd failed: {error}");
                std::process::exit(1);
            }
        }
        FileIndexdCommand::Shutdown { socket_path } => {
            if let Err(error) = shutdown_daemon(socket_path).await {
                eprintln!("file-indexd shutdown failed: {error}");
                std::process::exit(1);
            }
        }
        FileIndexdCommand::Help => print_usage(),
    }
}

fn parse_command(args: impl IntoIterator<Item = OsString>) -> Result<FileIndexdCommand, String> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(FileIndexdCommand::Run {
            socket_path: default_socket_path(),
        });
    };

    match first.to_string_lossy().as_ref() {
        "-h" | "--help" => {
            if args.next().is_some() {
                return Err("--help does not accept extra arguments".to_owned());
            }
            Ok(FileIndexdCommand::Help)
        }
        "--shutdown" => {
            let socket_path = args
                .next()
                .map(PathBuf::from)
                .unwrap_or_else(default_socket_path);
            if args.next().is_some() {
                return Err("--shutdown accepts at most one socket path".to_owned());
            }
            Ok(FileIndexdCommand::Shutdown { socket_path })
        }
        _ => {
            if args.next().is_some() {
                return Err("file-indexd accepts at most one socket path".to_owned());
            }
            Ok(FileIndexdCommand::Run {
                socket_path: PathBuf::from(first),
            })
        }
    }
}

async fn shutdown_daemon(socket_path: PathBuf) -> Result<(), file_index::IndexClientError> {
    let client = IndexClient::new(PathBuf::new(), socket_path);
    match client.execute(IndexServiceCommand::Shutdown).await? {
        IndexServiceEvent::Shutdown => Ok(()),
        event => Err(file_index::IndexClientError::Protocol(format!(
            "unexpected shutdown event: {event:?}"
        ))),
    }
}

fn print_usage() {
    eprintln!("Usage: file-indexd [socket-path]");
    eprintln!("       file-indexd --shutdown [socket-path]");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_runs_default_socket() {
        let command = parse_command(Vec::<OsString>::new()).unwrap();

        assert!(matches!(command, FileIndexdCommand::Run { .. }));
    }

    #[test]
    fn parse_shutdown_accepts_socket_path() {
        let command = parse_command([
            OsString::from("--shutdown"),
            OsString::from("/tmp/file-indexd.sock"),
        ])
        .unwrap();

        assert!(matches!(
            command,
            FileIndexdCommand::Shutdown { socket_path } if socket_path == PathBuf::from("/tmp/file-indexd.sock")
        ));
    }

    #[test]
    fn parse_rejects_extra_socket_paths() {
        let error = parse_command([OsString::from("one"), OsString::from("two")]).unwrap_err();

        assert_eq!(error, "file-indexd accepts at most one socket path");
    }
}
