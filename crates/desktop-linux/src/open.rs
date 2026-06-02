use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use thiserror::Error;
use tokio::process::Command;

const XDG_OPEN: &str = "xdg-open";
const XDG_MIME: &str = "xdg-mime";
const DEFAULT_XDG_DATA_DIRS: &str = "/usr/local/share:/usr/share";

const TERMINAL_LAUNCHERS: &[TerminalLauncher] = &[
    TerminalLauncher {
        command: "xdg-terminal-exec",
        before_open_command: &[],
    },
    TerminalLauncher {
        command: "foot",
        before_open_command: &["-e"],
    },
    TerminalLauncher {
        command: "alacritty",
        before_open_command: &["-e"],
    },
    TerminalLauncher {
        command: "kitty",
        before_open_command: &["-e"],
    },
    TerminalLauncher {
        command: "wezterm",
        before_open_command: &["start", "--"],
    },
    TerminalLauncher {
        command: "ghostty",
        before_open_command: &["-e"],
    },
    TerminalLauncher {
        command: "gnome-terminal",
        before_open_command: &["--"],
    },
    TerminalLauncher {
        command: "konsole",
        before_open_command: &["-e"],
    },
    TerminalLauncher {
        command: "xfce4-terminal",
        before_open_command: &["-e"],
    },
    TerminalLauncher {
        command: "xterm",
        before_open_command: &["-e"],
    },
];

struct TerminalLauncher {
    command: &'static str,
    before_open_command: &'static [&'static str],
}

#[derive(Debug, Error)]
pub enum OpenError {
    #[error("could not start xdg-open for {path:?}: {source}")]
    Spawn {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("xdg-open failed for {path:?} with status {status}")]
    Failed { path: PathBuf, status: ExitStatus },
    #[error("could not start terminal opener for {path:?}: {source}")]
    TerminalSpawn {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not find a terminal emulator to open {path:?} with {desktop_id}")]
    TerminalUnavailable { path: PathBuf, desktop_id: String },
}

pub async fn open_path(path: impl AsRef<Path>) -> Result<(), OpenError> {
    let path = path.as_ref().to_path_buf();
    if let Some(desktop_id) = terminal_default_desktop_id(&path).await {
        return open_terminal_default_application(&path, desktop_id).await;
    }

    open_path_with_opener(path.as_ref(), OsStr::new(XDG_OPEN)).await
}

async fn terminal_default_desktop_id(path: &Path) -> Option<String> {
    let mime_type = query_file_mime_type(path).await?;
    let desktop_id = query_default_desktop_id(&mime_type).await?;
    let desktop_entry = read_default_desktop_entry(&desktop_id).await?;
    desktop_entry_requires_terminal(&desktop_entry).then_some(desktop_id)
}

async fn query_file_mime_type(path: &Path) -> Option<String> {
    let output = Command::new(XDG_MIME)
        .arg("query")
        .arg("filetype")
        .arg(path.as_os_str())
        .output()
        .await
        .ok()?;
    successful_command_output(output)
}

async fn query_default_desktop_id(mime_type: &str) -> Option<String> {
    let output = Command::new(XDG_MIME)
        .arg("query")
        .arg("default")
        .arg(mime_type)
        .output()
        .await
        .ok()?;
    successful_command_output(output)
}

fn successful_command_output(output: std::process::Output) -> Option<String> {
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

async fn read_default_desktop_entry(desktop_id: &str) -> Option<String> {
    for path in desktop_entry_search_paths(desktop_id) {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            return Some(content);
        }
    }
    None
}

fn desktop_entry_search_paths(desktop_id: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(data_home) = xdg_data_home() {
        paths.push(data_home.join("applications").join(desktop_id));
    }

    let data_dirs = env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from(DEFAULT_XDG_DATA_DIRS));
    paths.extend(
        env::split_paths(&data_dirs).map(|data_dir| data_dir.join("applications").join(desktop_id)),
    );

    paths
}

fn xdg_data_home() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
}

fn desktop_entry_requires_terminal(desktop_entry: &str) -> bool {
    let mut in_desktop_entry_group = false;
    for raw_line in desktop_entry.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "Terminal" {
            return value.trim().eq_ignore_ascii_case("true");
        }
    }

    false
}

async fn open_terminal_default_application(
    path: &Path,
    desktop_id: String,
) -> Result<(), OpenError> {
    let launcher = terminal_launcher().ok_or_else(|| OpenError::TerminalUnavailable {
        path: path.to_path_buf(),
        desktop_id,
    })?;
    let mut command = terminal_open_command(launcher, path);
    command.spawn().map_err(|source| OpenError::TerminalSpawn {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn terminal_launcher() -> Option<&'static TerminalLauncher> {
    TERMINAL_LAUNCHERS
        .iter()
        .find(|launcher| command_exists(launcher.command))
}

fn terminal_open_command(launcher: &TerminalLauncher, path: &Path) -> Command {
    let mut command = Command::new(launcher.command);
    command
        .args(launcher.before_open_command)
        // Terminal=true 的默认程序（例如 vim.desktop）需要真实终端承载；
        // 在终端里继续交给 xdg-open，保留系统 MIME 关联选择。
        .arg(XDG_OPEN)
        .arg(path.as_os_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn command_exists(command_name: &str) -> bool {
    let Some(search_path) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&search_path).any(|directory| directory.join(command_name).is_file())
}

async fn open_path_with_opener(path: &Path, opener: &OsStr) -> Result<(), OpenError> {
    let path = path.to_path_buf();
    let status = open_command(opener, &path)
        .status()
        .await
        .map_err(|source| OpenError::Spawn {
            path: path.clone(),
            source,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(OpenError::Failed { path, status })
    }
}

fn open_command(opener: &OsStr, path: &Path) -> Command {
    let mut command = Command::new(opener);
    command
        .arg(path.as_os_str())
        // xdg-open 可能回退到 vim/nvim 这类终端应用，不能继承启动器所在终端。
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn open_path_disconnects_stdio_from_launcher_terminal() {
        let dir = tempdir().unwrap();
        let opener = dir.path().join("fake-open");
        let report = dir.path().join("stdio-report");

        fs::write(
            &opener,
            r#"#!/bin/sh
exec 3>"$1"
readlink "/proc/$$/fd/0" >&3
readlink "/proc/$$/fd/1" >&3
readlink "/proc/$$/fd/2" >&3
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&opener).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&opener, permissions).unwrap();

        open_path_with_opener(&report, opener.as_os_str())
            .await
            .unwrap();

        let targets = fs::read_to_string(report).unwrap();
        assert_eq!(targets.lines().collect::<Vec<_>>(), ["/dev/null"; 3]);
    }

    #[test]
    fn desktop_entry_requires_terminal_reads_desktop_entry_group() {
        let desktop_entry = r#"
[Desktop Entry]
Name=Vim
Terminal=true
Exec=vim %F

[Desktop Action New]
Terminal=false
"#;

        assert!(desktop_entry_requires_terminal(desktop_entry));
    }

    #[test]
    fn desktop_entry_requires_terminal_ignores_other_groups() {
        let desktop_entry = r#"
[Desktop Action New]
Terminal=true

[Desktop Entry]
Name=Graphical Editor
Terminal=false
Exec=editor %F
"#;

        assert!(!desktop_entry_requires_terminal(desktop_entry));
    }
}
