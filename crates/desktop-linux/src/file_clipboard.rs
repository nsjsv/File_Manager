use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::string::FromUtf8Error;

use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub const GNOME_COPIED_FILES_MIME: &str = "x-special/gnome-copied-files";
pub const URI_LIST_MIME: &str = "text/uri-list";

const IMAGE_MIME_EXTENSIONS: [(&str, &str); 5] = [
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/webp", "webp"),
    ("image/bmp", "bmp"),
    ("image/tiff", "tiff"),
];
const TEXT_MIME_CANDIDATES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "TEXT",
    "STRING",
];

const WL_COPY: &str = "wl-copy";
const WL_PASTE: &str = "wl-paste";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClipboardOperation {
    Copy,
    Move,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileClipboardSelection {
    pub operation: FileClipboardOperation,
    pub paths: Vec<PathBuf>,
}

impl FileClipboardSelection {
    pub fn new(operation: FileClipboardOperation, paths: Vec<PathBuf>) -> Self {
        Self { operation, paths }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopClipboardContent {
    Files(FileClipboardSelection),
    Text(String),
    Image(ClipboardImage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardImage {
    pub mime: String,
    pub extension: String,
    pub bytes: Vec<u8>,
}

impl ClipboardImage {
    pub fn new(mime: impl Into<String>, extension: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            mime: mime.into(),
            extension: extension.into(),
            bytes,
        }
    }
}

#[derive(Debug, Error)]
pub enum FileClipboardError {
    #[error("could not start {command}: {source}")]
    Spawn {
        command: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write clipboard payload to {command}: {source}")]
    Write {
        command: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{command} failed with status {status}")]
    Failed {
        command: &'static str,
        status: ExitStatus,
    },
    #[error("{command} returned non-UTF-8 output: {source}")]
    InvalidCommandOutput {
        command: &'static str,
        #[source]
        source: FromUtf8Error,
    },
    #[error("clipboard payload for {mime} is invalid: {source}")]
    InvalidPayload {
        mime: &'static str,
        #[source]
        source: FileClipboardPayloadError,
    },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FileClipboardPayloadError {
    #[error("unsupported clipboard operation {operation:?}")]
    UnsupportedOperation { operation: String },
    #[error("unsupported file URI {uri:?}")]
    UnsupportedUri { uri: String },
    #[error("invalid percent encoding {sequence:?}")]
    InvalidPercentEncoding { sequence: String },
    #[error("clipboard selection does not contain any file paths")]
    EmptySelection,
}

pub async fn write_file_clipboard(
    selection: FileClipboardSelection,
) -> Result<(), FileClipboardError> {
    let payload = serialize_gnome_copied_files(&selection);
    let mut child = Command::new(WL_COPY)
        .arg("--type")
        .arg(GNOME_COPIED_FILES_MIME)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| FileClipboardError::Spawn {
            command: WL_COPY,
            source,
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|source| FileClipboardError::Write {
                command: WL_COPY,
                source,
            })?;
    }

    let status = child
        .wait()
        .await
        .map_err(|source| FileClipboardError::Spawn {
            command: WL_COPY,
            source,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(FileClipboardError::Failed {
            command: WL_COPY,
            status,
        })
    }
}

pub async fn read_file_clipboard() -> Result<Option<FileClipboardSelection>, FileClipboardError> {
    let Some(mime_types) = read_clipboard_mime_types().await? else {
        return Ok(None);
    };

    read_file_clipboard_from_mime_types(&mime_types).await
}

pub async fn read_desktop_clipboard() -> Result<Option<DesktopClipboardContent>, FileClipboardError>
{
    let Some(mime_types) = read_clipboard_mime_types().await? else {
        return Ok(None);
    };

    if let Some(selection) = read_file_clipboard_from_mime_types(&mime_types).await? {
        return Ok(Some(DesktopClipboardContent::Files(selection)));
    }

    if let Some((mime, extension)) = select_image_mime(&mime_types) {
        let bytes = read_clipboard_bytes_payload(mime).await?;
        if !bytes.is_empty() {
            return Ok(Some(DesktopClipboardContent::Image(ClipboardImage::new(
                mime, extension, bytes,
            ))));
        }
    }

    if let Some(mime) = select_text_mime(&mime_types) {
        let text = read_clipboard_text_payload(mime).await?;
        if !text.is_empty() {
            return Ok(Some(DesktopClipboardContent::Text(text)));
        }
    }

    Ok(None)
}

async fn read_file_clipboard_from_mime_types(
    mime_types: &[String],
) -> Result<Option<FileClipboardSelection>, FileClipboardError> {
    if mime_types
        .iter()
        .any(|mime_type| mime_type == GNOME_COPIED_FILES_MIME)
    {
        let payload = read_clipboard_text_payload(GNOME_COPIED_FILES_MIME).await?;
        let selection = parse_gnome_copied_files(&payload).map_err(|source| {
            FileClipboardError::InvalidPayload {
                mime: GNOME_COPIED_FILES_MIME,
                source,
            }
        })?;
        return Ok(Some(selection));
    }

    if mime_types
        .iter()
        .any(|mime_type| mime_type == URI_LIST_MIME)
    {
        let payload = read_clipboard_text_payload(URI_LIST_MIME).await?;
        let paths =
            parse_file_uri_list(&payload).map_err(|source| FileClipboardError::InvalidPayload {
                mime: URI_LIST_MIME,
                source,
            })?;
        if paths.is_empty() {
            return Ok(None);
        }
        return Ok(Some(FileClipboardSelection::new(
            FileClipboardOperation::Copy,
            paths,
        )));
    }

    Ok(None)
}

fn select_image_mime(mime_types: &[String]) -> Option<(&str, &str)> {
    IMAGE_MIME_EXTENSIONS
        .iter()
        .find_map(|(candidate, extension)| {
            mime_types
                .iter()
                .any(|mime_type| mime_type.eq_ignore_ascii_case(candidate))
                .then_some((*candidate, *extension))
        })
}

fn select_text_mime(mime_types: &[String]) -> Option<&str> {
    for candidate in TEXT_MIME_CANDIDATES {
        if let Some(mime_type) = mime_types
            .iter()
            .find(|mime_type| mime_type.eq_ignore_ascii_case(candidate))
        {
            return Some(mime_type.as_str());
        }
    }

    mime_types
        .iter()
        .find(|mime_type| mime_type.to_ascii_lowercase().starts_with("text/plain"))
        .map(String::as_str)
}

pub fn serialize_gnome_copied_files(selection: &FileClipboardSelection) -> String {
    let mut payload = match selection.operation {
        FileClipboardOperation::Copy => String::from("copy\n"),
        FileClipboardOperation::Move => String::from("cut\n"),
    };
    payload.push_str(&serialize_file_uri_list(&selection.paths));
    payload
}

pub fn parse_gnome_copied_files(
    payload: &str,
) -> Result<FileClipboardSelection, FileClipboardPayloadError> {
    let mut lines = payload.lines();
    let operation = match lines.next().map(str::trim) {
        Some("copy") => FileClipboardOperation::Copy,
        Some("cut") => FileClipboardOperation::Move,
        Some(operation) => {
            return Err(FileClipboardPayloadError::UnsupportedOperation {
                operation: operation.to_owned(),
            })
        }
        None => return Err(FileClipboardPayloadError::EmptySelection),
    };

    let paths_payload = lines.collect::<Vec<_>>().join("\n");
    let paths = parse_file_uri_list(&paths_payload)?;
    if paths.is_empty() {
        return Err(FileClipboardPayloadError::EmptySelection);
    }

    Ok(FileClipboardSelection::new(operation, paths))
}

pub fn serialize_file_uri_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| file_uri(path))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_file_uri_list(payload: &str) -> Result<Vec<PathBuf>, FileClipboardPayloadError> {
    payload
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(parse_file_uri)
        .collect()
}

async fn read_clipboard_mime_types() -> Result<Option<Vec<String>>, FileClipboardError> {
    let output = Command::new(WL_PASTE)
        .arg("--list-types")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|source| FileClipboardError::Spawn {
            command: WL_PASTE,
            source,
        })?;

    if !output.status.success() {
        return Ok(None);
    }

    let output = String::from_utf8(output.stdout).map_err(|source| {
        FileClipboardError::InvalidCommandOutput {
            command: WL_PASTE,
            source,
        }
    })?;
    Ok(Some(
        output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect(),
    ))
}

async fn read_clipboard_text_payload(mime: &str) -> Result<String, FileClipboardError> {
    let output = Command::new(WL_PASTE)
        .arg("--no-newline")
        .arg("--type")
        .arg(mime)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|source| FileClipboardError::Spawn {
            command: WL_PASTE,
            source,
        })?;

    if !output.status.success() {
        return Err(FileClipboardError::Failed {
            command: WL_PASTE,
            status: output.status,
        });
    }

    String::from_utf8(output.stdout).map_err(|source| FileClipboardError::InvalidCommandOutput {
        command: WL_PASTE,
        source,
    })
}

async fn read_clipboard_bytes_payload(mime: &str) -> Result<Vec<u8>, FileClipboardError> {
    let output = Command::new(WL_PASTE)
        .arg("--type")
        .arg(mime)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|source| FileClipboardError::Spawn {
            command: WL_PASTE,
            source,
        })?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(FileClipboardError::Failed {
            command: WL_PASTE,
            status: output.status,
        })
    }
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        match *byte {
            b'/' | b'-' | b'.' | b'_' | b'~' => uri.push(*byte as char),
            b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' => uri.push(*byte as char),
            byte => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

fn parse_file_uri(uri: &str) -> Result<PathBuf, FileClipboardPayloadError> {
    let path = if let Some(rest) = uri.strip_prefix("file://localhost") {
        rest
    } else if let Some(rest) = uri.strip_prefix("file://") {
        if !rest.starts_with('/') {
            return Err(FileClipboardPayloadError::UnsupportedUri {
                uri: uri.to_owned(),
            });
        }
        rest
    } else if let Some(rest) = uri.strip_prefix("file:") {
        if !rest.starts_with('/') {
            return Err(FileClipboardPayloadError::UnsupportedUri {
                uri: uri.to_owned(),
            });
        }
        rest
    } else {
        return Err(FileClipboardPayloadError::UnsupportedUri {
            uri: uri.to_owned(),
        });
    };

    let bytes = percent_decode_path(path)?;
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

fn percent_decode_path(text: &str) -> Result<Vec<u8>, FileClipboardPayloadError> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }

        if index + 2 >= bytes.len() {
            return Err(FileClipboardPayloadError::InvalidPercentEncoding {
                sequence: String::from_utf8_lossy(&bytes[index..]).into_owned(),
            });
        }
        let Some(high) = hex_value(bytes[index + 1]) else {
            return Err(FileClipboardPayloadError::InvalidPercentEncoding {
                sequence: String::from_utf8_lossy(&bytes[index..index + 3]).into_owned(),
            });
        };
        let Some(low) = hex_value(bytes[index + 2]) else {
            return Err(FileClipboardPayloadError::InvalidPercentEncoding {
                sequence: String::from_utf8_lossy(&bytes[index..index + 3]).into_owned(),
            });
        };
        decoded.push((high << 4) | low);
        index += 3;
    }
    Ok(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_list_round_trips_spaces_and_non_utf8_bytes() {
        let paths = vec![
            PathBuf::from("/tmp/File Manager/a b.txt"),
            PathBuf::from(OsString::from_vec(b"/tmp/non-utf8-\xFF".to_vec())),
        ];

        let payload = serialize_file_uri_list(&paths);

        assert_eq!(
            payload,
            "file:///tmp/File%20Manager/a%20b.txt\nfile:///tmp/non-utf8-%FF"
        );
        assert_eq!(parse_file_uri_list(&payload).unwrap(), paths);
    }

    #[test]
    fn uri_list_ignores_empty_lines_and_comments() {
        let payload = "# comment\n\nfile:///tmp/a%20b\r\n";

        let paths = parse_file_uri_list(payload).unwrap();

        assert_eq!(paths, vec![PathBuf::from("/tmp/a b")]);
    }

    #[test]
    fn gnome_copied_files_parses_cut_operation() {
        let selection = parse_gnome_copied_files("cut\nfile:///tmp/a\nfile:///tmp/b").unwrap();

        assert_eq!(selection.operation, FileClipboardOperation::Move);
        assert_eq!(
            selection.paths,
            vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]
        );
    }

    #[test]
    fn gnome_copied_files_serializes_copy_operation() {
        let selection = FileClipboardSelection::new(
            FileClipboardOperation::Copy,
            vec![PathBuf::from("/tmp/a b")],
        );

        let payload = serialize_gnome_copied_files(&selection);

        assert_eq!(payload, "copy\nfile:///tmp/a%20b");
    }

    #[test]
    fn image_mime_selection_prefers_png() {
        let mime_types = vec!["text/plain".to_owned(), "image/png".to_owned()];

        let selected = select_image_mime(&mime_types);

        assert_eq!(selected, Some(("image/png", "png")));
    }

    #[test]
    fn text_mime_selection_accepts_plain_with_charset() {
        let mime_types = vec!["text/plain;charset=utf-8".to_owned()];

        let selected = select_text_mime(&mime_types);

        assert_eq!(selected, Some("text/plain;charset=utf-8"));
    }

    #[test]
    fn remote_file_uri_is_rejected() {
        let error = parse_file_uri_list("file://example.test/tmp/a").unwrap_err();

        assert!(matches!(
            error,
            FileClipboardPayloadError::UnsupportedUri { .. }
        ));
    }
}
