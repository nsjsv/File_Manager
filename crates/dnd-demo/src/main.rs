use std::{env, fs, path::PathBuf};

use dnd_demo::payload::DragPayload;

mod wayland_dnd;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    log_startup_environment();

    let sample_path = create_sample_path()?;
    let sample_payload = DragPayload::new(std::slice::from_ref(&sample_path));
    eprintln!("[dnd-demo] startup: sample path {}", sample_path.display());

    wayland_dnd::run(sample_path, sample_payload)
}

fn log_startup_environment() {
    eprintln!(
        "[dnd-demo] startup: DISPLAY={:?} WAYLAND_DISPLAY={:?} WAYLAND_SOCKET={:?} XDG_SESSION_TYPE={:?}",
        env::var_os("DISPLAY"),
        env::var_os("WAYLAND_DISPLAY"),
        env::var_os("WAYLAND_SOCKET"),
        env::var_os("XDG_SESSION_TYPE")
    );
}

fn create_sample_path() -> std::io::Result<PathBuf> {
    let path = env::temp_dir().join(format!(
        "dnd-demo-wayland-sample-{}.txt",
        std::process::id()
    ));
    fs::write(
        &path,
        b"Created by dnd-demo. Drop this file from the Wayland demo into GNOME Files.\n",
    )?;
    Ok(path)
}
