use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

use crate::{
    archive_extraction_format_for_path, ArchiveExtractionFormat, FileError, FileKind,
    SEVEN_ZIP_COMMAND_NAMES,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveListingEntry {
    pub path: String,
    pub kind: FileKind,
}

pub async fn list_archive_members(
    archive: impl Into<PathBuf>,
) -> Result<Vec<ArchiveListingEntry>, FileError> {
    let archive = archive.into();
    let format = archive_extraction_format_for_path(&archive).ok_or(FileError::Unsupported(
        "archive format is not supported for listing",
    ))?;

    match format {
        ArchiveExtractionFormat::Zip => {
            spawn_archive_listing(archive, |archive| read_zip_members(&archive)).await
        }
        ArchiveExtractionFormat::Tar => {
            spawn_archive_listing(archive, |archive| {
                read_tar_members(&archive, TarCompression::Plain)
            })
            .await
        }
        ArchiveExtractionFormat::TarGz => {
            spawn_archive_listing(archive, |archive| {
                read_tar_members(&archive, TarCompression::Gzip)
            })
            .await
        }
        ArchiveExtractionFormat::SevenZip | ArchiveExtractionFormat::Rar => {
            read_seven_zip_members(&archive, archive_format_label(format)).await
        }
    }
}

async fn spawn_archive_listing(
    archive: PathBuf,
    read_members: impl FnOnce(PathBuf) -> Result<Vec<ArchiveListingEntry>, FileError> + Send + 'static,
) -> Result<Vec<ArchiveListingEntry>, FileError> {
    let join_path = archive.clone();
    tokio::task::spawn_blocking(move || read_members(archive))
        .await
        .map_err(|error| FileError::Archive {
            path: join_path,
            message: error.to_string(),
        })?
}

fn read_zip_members(path: &Path) -> Result<Vec<ArchiveListingEntry>, FileError> {
    let file = open_archive_file(path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|source| FileError::Archive {
        path: path.to_path_buf(),
        message: source.to_string(),
    })?;
    let mut members = Vec::with_capacity(archive.len());

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|source| FileError::Archive {
                path: path.to_path_buf(),
                message: source.to_string(),
            })?;
        let kind = if entry.is_dir() {
            FileKind::Directory
        } else if entry.is_symlink() {
            FileKind::Symlink
        } else {
            FileKind::File
        };
        members.push(ArchiveListingEntry {
            path: entry.name().to_owned(),
            kind,
        });
    }

    Ok(members)
}

fn read_tar_members(
    path: &Path,
    compression: TarCompression,
) -> Result<Vec<ArchiveListingEntry>, FileError> {
    let file = open_archive_file(path)?;
    match compression {
        TarCompression::Plain => read_tar_members_from(path, file),
        TarCompression::Gzip => read_tar_members_from(path, flate2::read::GzDecoder::new(file)),
    }
}

fn read_tar_members_from<R: io::Read>(
    archive_path: &Path,
    reader: R,
) -> Result<Vec<ArchiveListingEntry>, FileError> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries().map_err(|source| FileError::Archive {
        path: archive_path.to_path_buf(),
        message: source.to_string(),
    })?;
    let mut members = Vec::new();

    for entry_outcome in entries {
        let entry = entry_outcome.map_err(|source| FileError::Archive {
            path: archive_path.to_path_buf(),
            message: source.to_string(),
        })?;
        let entry_type = entry.header().entry_type();
        let kind = if entry_type.is_dir() {
            FileKind::Directory
        } else if entry_type.is_symlink() {
            FileKind::Symlink
        } else if entry_type.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        };
        let path = entry
            .path()
            .map_err(|source| FileError::Archive {
                path: archive_path.to_path_buf(),
                message: source.to_string(),
            })?
            .to_string_lossy()
            .into_owned();
        members.push(ArchiveListingEntry { path, kind });
    }

    Ok(members)
}

async fn read_seven_zip_members(
    path: &Path,
    format_label: &str,
) -> Result<Vec<ArchiveListingEntry>, FileError> {
    for command_name in SEVEN_ZIP_COMMAND_NAMES {
        let output = Command::new(command_name)
            .arg("l")
            .arg("-slt")
            .arg("-bd")
            .arg("-bsp0")
            .arg("--")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output()
            .await;
        let output = match output {
            Ok(output) => output,
            Err(source) if source.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(FileError::Archive {
                    path: path.to_path_buf(),
                    message: format!("could not run {command_name} for {format_label}: {source}"),
                });
            }
        };

        if output.status.success() {
            let listing = String::from_utf8_lossy(&output.stdout);
            return Ok(parse_seven_zip_listing(&listing));
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let message = if stderr.is_empty() {
            format!(
                "could not list {format_label} archive with {command_name}: {}",
                output.status
            )
        } else {
            format!("could not list {format_label} archive with {command_name}: {stderr}")
        };
        return Err(FileError::Archive {
            path: path.to_path_buf(),
            message,
        });
    }

    Err(FileError::Unsupported(
        "7z, 7zz or 7za command is required to preview this archive",
    ))
}

fn parse_seven_zip_listing(technical_listing: &str) -> Vec<ArchiveListingEntry> {
    let mut members = Vec::new();
    let mut in_archive_entries = false;
    let mut current_entry: Option<SevenZipListedEntry> = None;

    for line in technical_listing.lines() {
        if line.trim() == "----------" {
            in_archive_entries = true;
            continue;
        }
        if !in_archive_entries {
            continue;
        }
        if line.trim().is_empty() {
            push_listed_archive_member(current_entry.take(), &mut members);
            continue;
        }

        if let Some(path) = line.strip_prefix("Path = ") {
            push_listed_archive_member(current_entry.take(), &mut members);
            current_entry = Some(SevenZipListedEntry {
                path: path.to_owned(),
                is_directory: path.ends_with('/') || path.ends_with('\\'),
            });
            continue;
        }

        let Some(entry) = current_entry.as_mut() else {
            continue;
        };
        if let Some(folder) = line.strip_prefix("Folder = ") {
            entry.is_directory |= seven_zip_folder_field_is_directory(folder);
        } else if let Some(attributes) = line.strip_prefix("Attributes = ") {
            entry.is_directory |= seven_zip_attributes_field_is_directory(attributes);
        }
    }

    push_listed_archive_member(current_entry, &mut members);
    members
}

fn push_listed_archive_member(
    listed_entry: Option<SevenZipListedEntry>,
    members: &mut Vec<ArchiveListingEntry>,
) {
    let Some(listed_entry) = listed_entry else {
        return;
    };
    if listed_entry.path.is_empty() {
        return;
    }

    members.push(ArchiveListingEntry {
        path: listed_entry.path,
        kind: if listed_entry.is_directory {
            FileKind::Directory
        } else {
            FileKind::File
        },
    });
}

fn seven_zip_folder_field_is_directory(folder: &str) -> bool {
    let folder = folder.trim();
    folder == "+" || folder.eq_ignore_ascii_case("true")
}

fn seven_zip_attributes_field_is_directory(attributes: &str) -> bool {
    attributes.trim_start().starts_with('D')
}

fn archive_format_label(format: ArchiveExtractionFormat) -> &'static str {
    match format {
        ArchiveExtractionFormat::Zip => "zip",
        ArchiveExtractionFormat::Tar => "tar",
        ArchiveExtractionFormat::TarGz => "tar.gz",
        ArchiveExtractionFormat::SevenZip => "7z",
        ArchiveExtractionFormat::Rar => "rar",
    }
}

fn open_archive_file(path: &Path) -> Result<File, FileError> {
    File::open(path).map_err(|source| FileError::Archive {
        path: path.to_path_buf(),
        message: source.to_string(),
    })
}

#[derive(Debug, Clone, Copy)]
enum TarCompression {
    Plain,
    Gzip,
}

struct SevenZipListedEntry {
    path: String,
    is_directory: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_seven_zip_listing_reads_directory_markers() {
        let members = parse_seven_zip_listing(
            r#"
Path = archive.rar
Type = Rar

----------
Path = src
Folder = +
Attributes = D_ drwxr-xr-x

Path = src/main.rs
Folder = -
Attributes = A_ -rw-r--r--

Path = docs\guide.md
Folder = -
Attributes = A_ -rw-r--r--
"#,
        );

        assert_eq!(members.len(), 3);
        assert_eq!(members[0].path, "src");
        assert_eq!(members[0].kind, FileKind::Directory);
        assert_eq!(members[1].path, "src/main.rs");
        assert_eq!(members[1].kind, FileKind::File);
        assert_eq!(members[2].path, "docs\\guide.md");
        assert_eq!(members[2].kind, FileKind::File);
    }
}
