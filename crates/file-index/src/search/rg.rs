use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;

use file_core::FileKind;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::path_encoding::path_from_bytes;
use super::types::{
    DirectoryErrorPolicy, FileSearchMatch, FileSearchOptions, FileSearchOutcome, SearchResultSource,
};
use crate::IndexError;

pub(super) async fn search_file_contents_with_cancel(
    root: PathBuf,
    query: String,
    options: FileSearchOptions,
    cancel: CancellationToken,
) -> Result<FileSearchOutcome, IndexError> {
    let mut command = Command::new("rg");
    command
        .arg("--json")
        .arg("--line-number")
        .arg("--smart-case")
        .arg(query)
        .arg(".")
        .current_dir(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if options.include_hidden {
        command.arg("--hidden");
    }
    if options.directory_error_policy == DirectoryErrorPolicy::SkipUnreadable {
        command.arg("--no-messages");
    }
    for glob in exclude_globs(&options.exclude_patterns) {
        command.arg("-g").arg(format!("!{glob}"));
    }

    let mut child = command
        .spawn()
        .map_err(|error| IndexError::store(&root, format!("could not start rg: {error}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| IndexError::store(&root, "rg stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| IndexError::store(&root, "rg stderr unavailable"))?;
    let stderr_task = tokio::spawn(async move {
        let mut stderr = stderr;
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes).await;
        String::from_utf8_lossy(&bytes).trim().to_owned()
    });
    let mut lines = BufReader::new(stdout).lines();
    let mut seen_paths = HashSet::new();
    let mut matches = Vec::new();
    let limit = options.limit.max(1);
    let mut stopped_early = false;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(IndexError::Cancelled);
            }
            line = lines.next_line() => {
                let Some(line) = line.map_err(|error| IndexError::store(&root, format!("could not read rg output: {error}")))? else {
                    break;
                };
                let Some(search_match) = parse_match_event(&root, &line, matches.len())? else {
                    continue;
                };
                if !seen_paths.insert(search_match.path.clone()) {
                    continue;
                }
                matches.push(search_match);
                if matches.len() >= limit {
                    stopped_early = true;
                    let _ = child.kill().await;
                    break;
                }
            }
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|error| IndexError::store(&root, format!("could not wait for rg: {error}")))?;
    let stderr = stderr_task
        .await
        .unwrap_or_else(|_| "could not read rg stderr".to_owned());
    if !stopped_early && !status.success() && status.code() != Some(1) {
        let message = if stderr.is_empty() {
            format!("rg exited with {status}")
        } else {
            stderr
        };
        return Err(IndexError::store(&root, message));
    }

    Ok(FileSearchOutcome {
        root,
        matches,
        skipped: Vec::new(),
    })
}

fn exclude_globs(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
        .map(|pattern| {
            if let Some(prefix) = pattern.strip_suffix('/') {
                format!("{prefix}/**")
            } else {
                pattern.to_owned()
            }
        })
        .collect()
}

fn parse_match_event(
    root: &PathBuf,
    line: &str,
    index: usize,
) -> Result<Option<FileSearchMatch>, IndexError> {
    let event: RgEvent = serde_json::from_str(line).map_err(|error| {
        IndexError::store(root, format!("could not parse rg json output: {error}"))
    })?;
    if event.kind != "match" {
        return Ok(None);
    }
    let data: RgMatchData = serde_json::from_value(event.data).map_err(|error| {
        IndexError::store(root, format!("could not parse rg match output: {error}"))
    })?;
    let Some(relative_path) = data.path.to_relative_path(root)? else {
        return Ok(None);
    };
    let path = root.join(&relative_path);
    let name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| OsString::from("match"));
    let snippet = data
        .lines
        .first_line(root)?
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned);
    Ok(Some(FileSearchMatch {
        path,
        relative_path,
        name,
        kind: FileKind::File,
        rank_score: u32::MAX.saturating_sub(index as u32),
        source: SearchResultSource::Contents,
        snippet,
        media: None,
    }))
}

#[derive(Deserialize)]
struct RgEvent {
    #[serde(rename = "type")]
    kind: String,
    data: Value,
}

#[derive(Deserialize)]
struct RgMatchData {
    path: RgTextField,
    lines: RgTextField,
}

#[derive(Deserialize)]
struct RgTextField {
    text: Option<String>,
    bytes: Option<String>,
}

impl RgTextField {
    fn to_bytes(&self, root: &PathBuf) -> Result<Option<Vec<u8>>, IndexError> {
        if let Some(text) = &self.text {
            return Ok(Some(text.as_bytes().to_vec()));
        }
        self.bytes
            .as_deref()
            .map(decode_base64)
            .transpose()
            .map_err(|error| IndexError::store(root, error))
    }

    fn to_relative_path(&self, root: &PathBuf) -> Result<Option<PathBuf>, IndexError> {
        self.to_bytes(root).map(|bytes| {
            bytes.map(|bytes| {
                let relative_path = path_from_bytes(bytes);
                relative_path
                    .strip_prefix(".")
                    .unwrap_or(relative_path.as_path())
                    .to_path_buf()
            })
        })
    }

    fn first_line(&self, root: &PathBuf) -> Result<Option<String>, IndexError> {
        self.to_bytes(root).map(|bytes| {
            bytes.map(|bytes| {
                let text = String::from_utf8_lossy(&bytes);
                text.lines().next().unwrap_or_default().to_owned()
            })
        })
    }
}

fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::with_capacity(text.len() * 3 / 4);
    let mut chunk = [0_u8; 4];
    let mut chunk_len = 0;

    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => 64,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => {
                return Err(format!(
                    "could not decode rg base64 field: invalid byte {byte}"
                ))
            }
        };
        chunk[chunk_len] = value;
        chunk_len += 1;
        if chunk_len == 4 {
            decode_base64_chunk(chunk, &mut decoded)?;
            chunk_len = 0;
        }
    }

    if chunk_len != 0 {
        return Err("could not decode rg base64 field: invalid length".to_owned());
    }

    Ok(decoded)
}

fn decode_base64_chunk(chunk: [u8; 4], decoded: &mut Vec<u8>) -> Result<(), String> {
    if chunk[0] == 64 || chunk[1] == 64 {
        return Err("could not decode rg base64 field: invalid padding".to_owned());
    }

    decoded.push((chunk[0] << 2) | (chunk[1] >> 4));
    if chunk[2] == 64 {
        if chunk[3] != 64 {
            return Err("could not decode rg base64 field: invalid padding".to_owned());
        }
        return Ok(());
    }

    decoded.push((chunk[1] << 4) | (chunk[2] >> 2));
    if chunk[3] == 64 {
        return Ok(());
    }

    decoded.push((chunk[2] << 6) | chunk[3]);
    Ok(())
}
