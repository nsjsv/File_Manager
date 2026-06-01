use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use thiserror::Error;
use tokio::process::Command;

const XDG_OPEN: &str = "xdg-open";

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
}

pub async fn open_path(path: impl AsRef<Path>) -> Result<(), OpenError> {
    open_path_with_opener(path.as_ref(), OsStr::new(XDG_OPEN)).await
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
}
