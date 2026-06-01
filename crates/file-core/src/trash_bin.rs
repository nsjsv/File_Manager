use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use tokio::fs;

use crate::ops::TransferConflictStrategy;
use crate::scan::{entry_from_path, is_hidden_name, FileError, ScanOptions, ScanWarning};
use crate::{compare_entries, DirectoryEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashScan {
    pub entries: Vec<TrashEntry>,
    pub skipped: Vec<ScanWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashEntry {
    pub trash_path: PathBuf,
    pub info_path: PathBuf,
    pub original_path: PathBuf,
    pub deletion_date: Option<String>,
    pub entry: DirectoryEntry,
}

impl TrashEntry {
    pub fn restore_entry(&self) -> TrashRestoreEntry {
        TrashRestoreEntry {
            trash_path: self.trash_path.clone(),
            info_path: self.info_path.clone(),
            original_path: self.original_path.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashRestoreEntry {
    pub trash_path: PathBuf,
    pub info_path: PathBuf,
    pub original_path: PathBuf,
}

struct TrashLayout {
    files_dir: PathBuf,
    info_dir: PathBuf,
}

struct TrashInfo {
    path: PathBuf,
    deletion_date: Option<String>,
}

pub async fn scan_trash(options: ScanOptions) -> Result<TrashScan, FileError> {
    let layout = trash_layout()?;
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    let mut reader = match fs::read_dir(&layout.info_dir).await {
        Ok(reader) => reader,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TrashScan { entries, skipped })
        }
        Err(source) => {
            return Err(FileError::ReadDirectory {
                path: layout.info_dir,
                source,
            })
        }
    };

    loop {
        let info_entry = match reader.next_entry().await {
            Ok(Some(info_entry)) => info_entry,
            Ok(None) => break,
            Err(source) => {
                return Err(FileError::ReadEntry {
                    path: layout.info_dir.clone(),
                    source,
                })
            }
        };

        let info_name = info_entry.file_name();
        let Some(item_name) = trash_item_name(&info_name) else {
            continue;
        };

        let info_path = info_entry.path();
        let trash_path = layout.files_dir.join(&item_name);
        let content = match fs::read_to_string(&info_path).await {
            Ok(content) => content,
            Err(source) => {
                skipped.push(ScanWarning {
                    path: info_path,
                    message: source.to_string(),
                });
                continue;
            }
        };
        let Some(info) = parse_trash_info(&content) else {
            skipped.push(ScanWarning {
                path: info_path,
                message: "invalid trashinfo".to_owned(),
            });
            continue;
        };

        let display_name = info
            .path
            .file_name()
            .map(OsStr::to_os_string)
            .unwrap_or_else(|| item_name.clone());
        let is_hidden = is_hidden_name(&display_name);
        if is_hidden && !options.include_hidden {
            continue;
        }

        match entry_from_path(trash_path.clone(), display_name, is_hidden).await {
            Ok(entry) => entries.push(TrashEntry {
                trash_path,
                info_path,
                original_path: info.path,
                deletion_date: info.deletion_date,
                entry,
            }),
            Err(FileError::Metadata { path, source }) => skipped.push(ScanWarning {
                path,
                message: source.to_string(),
            }),
            Err(error) => return Err(error),
        }
    }

    entries.sort_by(|left, right| compare_entries(&left.entry, &right.entry, &options));

    Ok(TrashScan { entries, skipped })
}

pub async fn restore_trash_entry(
    entry: TrashRestoreEntry,
    conflict_strategy: TransferConflictStrategy,
) -> Result<PathBuf, FileError> {
    let Some(target) = prepare_restore_target(&entry, conflict_strategy).await? else {
        return Ok(entry.original_path);
    };
    if let Some(parent) = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .await
            .map_err(|source| FileError::Move {
                from: entry.trash_path.clone(),
                to: target.clone(),
                source,
            })?;
    }

    fs::rename(&entry.trash_path, &target)
        .await
        .map_err(|source| FileError::Move {
            from: entry.trash_path.clone(),
            to: target.clone(),
            source,
        })?;
    remove_info_file(&entry.info_path).await?;
    Ok(target)
}

pub async fn delete_trash_entry(entry: TrashRestoreEntry) -> Result<(), FileError> {
    match symlink_metadata_if_exists(&entry.trash_path)
        .await
        .map_err(|source| FileError::Trash {
            path: entry.trash_path.clone(),
            message: source.to_string(),
        })? {
        Some(metadata) if metadata.is_dir() => fs::remove_dir_all(&entry.trash_path)
            .await
            .map_err(|source| FileError::Trash {
                path: entry.trash_path.clone(),
                message: source.to_string(),
            })?,
        Some(_) => fs::remove_file(&entry.trash_path)
            .await
            .map_err(|source| FileError::Trash {
                path: entry.trash_path.clone(),
                message: source.to_string(),
            })?,
        None => {}
    }
    remove_info_file(&entry.info_path).await
}

pub async fn empty_trash() -> Result<(), FileError> {
    let layout = trash_layout()?;
    remove_dir_if_exists(&layout.files_dir).await?;
    remove_dir_if_exists(&layout.info_dir).await?;
    ensure_trash_layout(&layout).await
}

async fn prepare_restore_target(
    entry: &TrashRestoreEntry,
    conflict_strategy: TransferConflictStrategy,
) -> Result<Option<PathBuf>, FileError> {
    let target = &entry.original_path;
    let Some(target_metadata) =
        metadata_if_exists(target)
            .await
            .map_err(|source| FileError::Move {
                from: entry.trash_path.clone(),
                to: target.clone(),
                source,
            })?
    else {
        return Ok(Some(target.clone()));
    };

    match conflict_strategy {
        TransferConflictStrategy::Fail => Err(FileError::Move {
            from: entry.trash_path.clone(),
            to: target.clone(),
            source: already_exists_error(),
        }),
        TransferConflictStrategy::Replace => {
            remove_restore_target(&entry.trash_path, target, &target_metadata).await?;
            Ok(Some(target.clone()))
        }
        TransferConflictStrategy::Skip => Ok(None),
        TransferConflictStrategy::KeepBoth => Ok(Some(unique_available_path(target))),
        TransferConflictStrategy::Merge => {
            let source_metadata =
                fs::metadata(&entry.trash_path)
                    .await
                    .map_err(|source| FileError::Metadata {
                        path: entry.trash_path.clone(),
                        source,
                    })?;
            if source_metadata.is_dir() && target_metadata.is_dir() {
                Ok(Some(target.clone()))
            } else {
                Ok(None)
            }
        }
    }
}

async fn remove_restore_target(
    from: &Path,
    to: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), FileError> {
    let result = if metadata.is_dir() {
        fs::remove_dir_all(to).await
    } else {
        fs::remove_file(to).await
    };

    result.map_err(|source| FileError::Move {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })
}

async fn metadata_if_exists(path: &Path) -> io::Result<Option<std::fs::Metadata>> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

async fn symlink_metadata_if_exists(path: &Path) -> io::Result<Option<std::fs::Metadata>> {
    match fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn already_exists_error() -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, "target already exists")
}

fn unique_available_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("item"));

    for index in 1..1000 {
        let mut next = name.clone();
        next.push(format!(".copy{index}"));
        let candidate = parent.join(next);
        if !candidate.exists() {
            return candidate;
        }
    }

    path.to_path_buf()
}

async fn remove_info_file(info_path: &Path) -> Result<(), FileError> {
    match fs::remove_file(info_path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(FileError::Trash {
            path: info_path.to_path_buf(),
            message: error.to_string(),
        }),
    }
}

async fn remove_dir_if_exists(path: &Path) -> Result<(), FileError> {
    match fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(FileError::Trash {
            path: path.to_path_buf(),
            message: error.to_string(),
        }),
    }
}

async fn ensure_trash_layout(layout: &TrashLayout) -> Result<(), FileError> {
    fs::create_dir_all(&layout.files_dir)
        .await
        .map_err(|source| FileError::CreateDirectory {
            path: layout.files_dir.clone(),
            source,
        })?;
    fs::create_dir_all(&layout.info_dir)
        .await
        .map_err(|source| FileError::CreateDirectory {
            path: layout.info_dir.clone(),
            source,
        })?;
    Ok(())
}

fn trash_layout() -> Result<TrashLayout, FileError> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or(FileError::Unsupported(
            "trash requires HOME or XDG_DATA_HOME",
        ))?;
    let trash_dir = data_home.join("Trash");
    Ok(TrashLayout {
        files_dir: trash_dir.join("files"),
        info_dir: trash_dir.join("info"),
    })
}

fn parse_trash_info(content: &str) -> Option<TrashInfo> {
    let mut path = None;
    let mut deletion_date = None;
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("Path=") {
            path = Some(percent_decode_path(value));
        } else if let Some(value) = line.strip_prefix("DeletionDate=") {
            deletion_date = Some(value.to_owned());
        }
    }

    Some(TrashInfo {
        path: path?,
        deletion_date,
    })
}

fn trash_item_name(info_name: &OsStr) -> Option<OsString> {
    const SUFFIX: &[u8] = b".trashinfo";
    #[cfg(unix)]
    {
        let bytes = info_name.as_bytes();
        bytes
            .strip_suffix(SUFFIX)
            .map(|name| OsString::from_vec(name.to_vec()))
    }
    #[cfg(not(unix))]
    {
        let name = info_name.to_string_lossy();
        name.strip_suffix(".trashinfo").map(OsString::from)
    }
}

fn percent_decode_path(value: &str) -> PathBuf {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                output.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }

    #[cfg(unix)]
    {
        PathBuf::from(OsString::from_vec(output))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(&output).into_owned())
    }
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
