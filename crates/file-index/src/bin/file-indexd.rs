use std::path::PathBuf;

use file_index::daemon::{run, IndexDaemonConfig};
use file_index::ipc::default_socket_path;

#[tokio::main]
async fn main() {
    let socket_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_socket_path);
    if let Err(error) = run(IndexDaemonConfig { socket_path }).await {
        eprintln!("file-indexd failed: {error}");
        std::process::exit(1);
    }
}
