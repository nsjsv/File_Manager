use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use zip::result::ZipError;

use crate::{ArchivePassword, FileError};

const SEVEN_ZIP_COMMAND_NAMES: [&str; 3] = ["7z", "7zz", "7za"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveExtractionFormat {
    Zip,
    Tar,
    TarGz,
    SevenZip,
    Rar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveExtractionRequest {
    pub archive: PathBuf,
    pub destination: PathBuf,
    pub password: Option<ArchivePassword>,
}

impl ArchiveExtractionRequest {
    pub fn from_archive_path(
        archive: impl Into<PathBuf>,
        password: Option<ArchivePassword>,
    ) -> Result<Self, FileError> {
        let archive = archive.into();
        let destination = default_archive_extraction_directory(&archive)?;
        Ok(Self {
            archive,
            destination,
            password,
        })
    }

    pub fn with_password(&self, password: Option<ArchivePassword>) -> Self {
        Self {
            archive: self.archive.clone(),
            destination: self.destination.clone(),
            password,
        }
    }

    pub fn without_password(&self) -> Self {
        self.with_password(None)
    }
}

pub fn archive_extraction_format_for_path(
    path: impl AsRef<Path>,
) -> Option<ArchiveExtractionFormat> {
    let path = path.as_ref();
    let file_name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        return Some(ArchiveExtractionFormat::TarGz);
    }

    match path.extension().and_then(OsStr::to_str) {
        Some(extension) if extension.eq_ignore_ascii_case("zip") => {
            Some(ArchiveExtractionFormat::Zip)
        }
        Some(extension) if extension.eq_ignore_ascii_case("tar") => {
            Some(ArchiveExtractionFormat::Tar)
        }
        Some(extension) if extension.eq_ignore_ascii_case("7z") => {
            Some(ArchiveExtractionFormat::SevenZip)
        }
        Some(extension) if extension.eq_ignore_ascii_case("rar") => {
            Some(ArchiveExtractionFormat::Rar)
        }
        _ => None,
    }
}

pub fn is_supported_archive_path(path: impl AsRef<Path>) -> bool {
    archive_extraction_format_for_path(path).is_some()
}

pub async fn inspect_archive_extraction(
    request: ArchiveExtractionRequest,
    cancel: CancellationToken,
) -> Result<(), FileError> {
    let format = archive_extraction_format_for_path(&request.archive).ok_or(
        FileError::Unsupported("archive format is not supported for extraction"),
    )?;

    match format {
        ArchiveExtractionFormat::Zip => inspect_zip_archive(request, cancel).await,
        ArchiveExtractionFormat::Tar | ArchiveExtractionFormat::TarGz => {
            reject_tar_password(&request)?;
            Ok(())
        }
        ArchiveExtractionFormat::SevenZip | ArchiveExtractionFormat::Rar => {
            inspect_archive_with_seven_zip(request, cancel).await
        }
    }
}

pub async fn extract_archive(
    request: ArchiveExtractionRequest,
    cancel: CancellationToken,
) -> Result<PathBuf, FileError> {
    let format = archive_extraction_format_for_path(&request.archive).ok_or(
        FileError::Unsupported("archive format is not supported for extraction"),
    )?;

    match format {
        ArchiveExtractionFormat::Zip => extract_zip_archive(request, cancel).await,
        ArchiveExtractionFormat::Tar => {
            reject_tar_password(&request)?;
            extract_tar_archive(request, TarCompression::Plain, cancel).await
        }
        ArchiveExtractionFormat::TarGz => {
            reject_tar_password(&request)?;
            extract_tar_archive(request, TarCompression::Gzip, cancel).await
        }
        ArchiveExtractionFormat::SevenZip | ArchiveExtractionFormat::Rar => {
            extract_archive_with_seven_zip(request, cancel).await
        }
    }
}

async fn inspect_zip_archive(
    request: ArchiveExtractionRequest,
    cancel: CancellationToken,
) -> Result<(), FileError> {
    let archive = request.archive.clone();
    tokio::task::spawn_blocking(move || inspect_zip_entries(&request, &cancel))
        .await
        .map_err(|error| FileError::Archive {
            path: archive,
            message: error.to_string(),
        })?
}

async fn extract_zip_archive(
    request: ArchiveExtractionRequest,
    cancel: CancellationToken,
) -> Result<PathBuf, FileError> {
    let destination = request.destination.clone();
    let join_destination = destination.clone();
    tokio::task::spawn_blocking(move || extract_zip_archive_blocking(request, cancel))
        .await
        .map_err(|error| FileError::Archive {
            path: join_destination,
            message: error.to_string(),
        })?
}

fn extract_zip_archive_blocking(
    request: ArchiveExtractionRequest,
    cancel: CancellationToken,
) -> Result<PathBuf, FileError> {
    inspect_zip_entries(&request, &cancel)?;
    create_destination_directory(&request.destination)?;
    let outcome = extract_zip_entries(&request, &cancel);
    if outcome.is_err() {
        let _ = fs::remove_dir_all(&request.destination);
    }
    outcome.map(|_| request.destination)
}

fn inspect_zip_entries(
    request: &ArchiveExtractionRequest,
    cancel: &CancellationToken,
) -> Result<(), FileError> {
    let file = open_archive_file(&request.archive)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| zip_error(&request.archive, error))?;

    for index in 0..archive.len() {
        cancel_if_requested(cancel)?;
        let entry = open_zip_entry(
            &mut archive,
            index,
            request.password.as_ref(),
            &request.archive,
        )?;
        validate_zip_entry_path(&request.archive, &entry)?;
    }
    Ok(())
}

fn extract_zip_entries(
    request: &ArchiveExtractionRequest,
    cancel: &CancellationToken,
) -> Result<(), FileError> {
    let file = open_archive_file(&request.archive)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| zip_error(&request.archive, error))?;

    for index in 0..archive.len() {
        cancel_if_requested(cancel)?;
        let mut entry = open_zip_entry(
            &mut archive,
            index,
            request.password.as_ref(),
            &request.archive,
        )?;
        let relative_path = validate_zip_entry_path(&request.archive, &entry)?;
        let target = request.destination.join(relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|source| FileError::CreateDirectory {
                path: target,
                source,
            })?;
            continue;
        }
        if entry.is_symlink() {
            return Err(FileError::Unsupported(
                "zip symlink extraction is not supported",
            ));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|source| FileError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|source| FileError::CreateFile {
                path: target.clone(),
                source,
            })?;
        io::copy(&mut entry, &mut output).map_err(|source| FileError::Archive {
            path: request.archive.clone(),
            message: source.to_string(),
        })?;
        output.flush().map_err(|source| FileError::Archive {
            path: target,
            message: source.to_string(),
        })?;
    }
    Ok(())
}

fn open_zip_entry<'a, R: io::Read + io::Seek>(
    archive: &'a mut zip::ZipArchive<R>,
    index: usize,
    password: Option<&ArchivePassword>,
    archive_path: &Path,
) -> Result<zip::read::ZipFile<'a, R>, FileError> {
    match password {
        Some(password) => archive
            .by_index_decrypt(index, password.as_str().as_bytes())
            .map_err(|error| zip_error(archive_path, error)),
        None => {
            let entry = archive
                .by_index(index)
                .map_err(|error| zip_error(archive_path, error))?;
            if entry.encrypted() {
                return Err(FileError::ArchivePasswordRequired {
                    path: archive_path.to_path_buf(),
                });
            }
            Ok(entry)
        }
    }
}

fn validate_zip_entry_path<R: io::Read>(
    archive_path: &Path,
    entry: &zip::read::ZipFile<'_, R>,
) -> Result<PathBuf, FileError> {
    entry
        .enclosed_name()
        .ok_or_else(|| FileError::InvalidInput {
            path: archive_path.to_path_buf(),
            message: format!("archive entry path is not safe: {}", entry.name()),
        })
}

async fn extract_tar_archive(
    request: ArchiveExtractionRequest,
    compression: TarCompression,
    cancel: CancellationToken,
) -> Result<PathBuf, FileError> {
    let destination = request.destination.clone();
    let join_destination = destination.clone();
    tokio::task::spawn_blocking(move || extract_tar_archive_blocking(request, compression, cancel))
        .await
        .map_err(|error| FileError::Archive {
            path: join_destination,
            message: error.to_string(),
        })?
}

fn extract_tar_archive_blocking(
    request: ArchiveExtractionRequest,
    compression: TarCompression,
    cancel: CancellationToken,
) -> Result<PathBuf, FileError> {
    create_destination_directory(&request.destination)?;
    let outcome = match compression {
        TarCompression::Plain => {
            let file = open_archive_file(&request.archive)?;
            extract_tar_entries(file, &request, &cancel)
        }
        TarCompression::Gzip => {
            let file = open_archive_file(&request.archive)?;
            extract_tar_entries(flate2::read::GzDecoder::new(file), &request, &cancel)
        }
    };
    if outcome.is_err() {
        let _ = fs::remove_dir_all(&request.destination);
    }
    outcome.map(|_| request.destination)
}

fn extract_tar_entries<R: io::Read>(
    reader: R,
    request: &ArchiveExtractionRequest,
    cancel: &CancellationToken,
) -> Result<(), FileError> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries().map_err(|source| FileError::Archive {
        path: request.archive.clone(),
        message: source.to_string(),
    })?;

    for entry in entries {
        cancel_if_requested(cancel)?;
        let mut entry = entry.map_err(|source| FileError::Archive {
            path: request.archive.clone(),
            message: source.to_string(),
        })?;
        let unpacked =
            entry
                .unpack_in(&request.destination)
                .map_err(|source| FileError::Archive {
                    path: request.archive.clone(),
                    message: source.to_string(),
                })?;
        if !unpacked {
            return Err(FileError::InvalidInput {
                path: request.archive.clone(),
                message: "archive entry path is not safe".to_owned(),
            });
        }
    }
    Ok(())
}

async fn extract_archive_with_seven_zip(
    request: ArchiveExtractionRequest,
    cancel: CancellationToken,
) -> Result<PathBuf, FileError> {
    for command_name in SEVEN_ZIP_COMMAND_NAMES {
        match test_seven_zip_archive(command_name, &request, &cancel).await {
            Ok(()) => {
                create_destination_directory(&request.destination)?;
                let outcome = run_seven_zip_extract(command_name, &request, &cancel).await;
                if outcome.is_err() {
                    let _ = fs::remove_dir_all(&request.destination);
                }
                return outcome.map(|_| request.destination);
            }
            Err(FileError::Archive { message, .. }) if message == command_not_found_marker() => {
                continue;
            }
            Err(error) => return Err(error),
        }
    }

    Err(FileError::Unsupported(
        "7z, 7zz or 7za command is required to extract this archive",
    ))
}

async fn inspect_archive_with_seven_zip(
    request: ArchiveExtractionRequest,
    cancel: CancellationToken,
) -> Result<(), FileError> {
    for command_name in SEVEN_ZIP_COMMAND_NAMES {
        match test_seven_zip_archive(command_name, &request, &cancel).await {
            Ok(()) => return Ok(()),
            Err(FileError::Archive { message, .. }) if message == command_not_found_marker() => {
                continue;
            }
            Err(error) => return Err(error),
        }
    }

    Err(FileError::Unsupported(
        "7z, 7zz or 7za command is required to extract this archive",
    ))
}

async fn test_seven_zip_archive(
    command_name: &str,
    request: &ArchiveExtractionRequest,
    cancel: &CancellationToken,
) -> Result<(), FileError> {
    let mut command = seven_zip_command(command_name, "t", request);
    append_seven_zip_archive_operand(&mut command, &request.archive);
    run_seven_zip_command(&mut command, request, cancel).await
}

async fn run_seven_zip_extract(
    command_name: &str,
    request: &ArchiveExtractionRequest,
    cancel: &CancellationToken,
) -> Result<(), FileError> {
    let mut command = seven_zip_extract_command(command_name, request);
    run_seven_zip_command(&mut command, request, cancel).await
}

fn seven_zip_extract_command(command_name: &str, request: &ArchiveExtractionRequest) -> Command {
    let mut command = seven_zip_command(command_name, "x", request);
    command.arg(format!("-o{}", request.destination.to_string_lossy()));
    append_seven_zip_archive_operand(&mut command, &request.archive);
    command
}

fn seven_zip_command(
    command_name: &str,
    action: &str,
    request: &ArchiveExtractionRequest,
) -> Command {
    let mut command = Command::new(command_name);
    command
        .arg(action)
        .arg("-y")
        .arg("-bd")
        .arg("-bsp0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(password) = &request.password {
        command.arg(format!("-p{}", password.as_str()));
    }
    command
}

fn append_seven_zip_archive_operand(command: &mut Command, archive: &Path) {
    // 7z treats arguments after `--` as operands, so switches such as `-o` must be finalized first.
    command.arg("--").arg(archive);
}

async fn run_seven_zip_command(
    command: &mut Command,
    request: &ArchiveExtractionRequest,
    cancel: &CancellationToken,
) -> Result<(), FileError> {
    let output = tokio::select! {
        output = command.output() => output.map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                FileError::Archive {
                    path: request.archive.clone(),
                    message: command_not_found_marker().to_owned(),
                }
            } else {
                FileError::Archive {
                    path: request.archive.clone(),
                    message: source.to_string(),
                }
            }
        })?,
        _ = cancel.cancelled() => return Err(FileError::Cancelled),
    };
    if output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(seven_zip_error(request, output.status, stdout, stderr))
}

fn seven_zip_error(
    request: &ArchiveExtractionRequest,
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
) -> FileError {
    let combined_output = if stdout.is_empty() {
        stderr.clone()
    } else if stderr.is_empty() {
        stdout.clone()
    } else {
        format!("{stdout}\n{stderr}")
    };
    let lower = combined_output.to_ascii_lowercase();
    if lower.contains("password") || lower.contains("encrypted") {
        if request.password.is_some() {
            return FileError::ArchiveInvalidPassword {
                path: request.archive.clone(),
            };
        }
        return FileError::ArchivePasswordRequired {
            path: request.archive.clone(),
        };
    }
    let message = if combined_output.is_empty() {
        format!("7z exited with status {status}")
    } else {
        combined_output
    };
    FileError::Archive {
        path: request.archive.clone(),
        message,
    }
}

fn default_archive_extraction_directory(archive: &Path) -> Result<PathBuf, FileError> {
    let parent = archive
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let Some(parent) = parent else {
        return Err(FileError::InvalidInput {
            path: archive.to_path_buf(),
            message: "archive path has no parent directory".to_owned(),
        });
    };
    let Some(name) = archive_extraction_directory_name(archive) else {
        return Err(FileError::InvalidInput {
            path: archive.to_path_buf(),
            message: "archive name is not usable as a directory name".to_owned(),
        });
    };
    Ok(parent.join(name))
}

fn archive_extraction_directory_name(archive: &Path) -> Option<OsString> {
    let file_name = archive.file_name()?;
    if let Some(text) = file_name.to_str() {
        let lower = text.to_ascii_lowercase();
        for suffix in [".tar.gz", ".tgz", ".zip", ".tar", ".7z", ".rar"] {
            if lower.ends_with(suffix) {
                let stem = &text[..text.len() - suffix.len()];
                if !stem.is_empty() {
                    return Some(OsString::from(stem));
                }
            }
        }
    }
    archive.file_stem().map(OsStr::to_os_string)
}

fn create_destination_directory(destination: &Path) -> Result<(), FileError> {
    fs::create_dir(destination).map_err(|source| FileError::CreateDirectory {
        path: destination.to_path_buf(),
        source,
    })
}

fn reject_tar_password(request: &ArchiveExtractionRequest) -> Result<(), FileError> {
    if request.password.is_some() {
        return Err(FileError::InvalidInput {
            path: request.archive.clone(),
            message: "tar archives do not support passwords".to_owned(),
        });
    }
    Ok(())
}

fn open_archive_file(path: &Path) -> Result<File, FileError> {
    File::open(path).map_err(|source| FileError::Archive {
        path: path.to_path_buf(),
        message: source.to_string(),
    })
}

fn zip_error(path: &Path, error: ZipError) -> FileError {
    match error {
        ZipError::UnsupportedArchive(message) if message == ZipError::PASSWORD_REQUIRED => {
            FileError::ArchivePasswordRequired {
                path: path.to_path_buf(),
            }
        }
        ZipError::InvalidPassword => FileError::ArchiveInvalidPassword {
            path: path.to_path_buf(),
        },
        error => FileError::Archive {
            path: path.to_path_buf(),
            message: error.to_string(),
        },
    }
}

fn cancel_if_requested(cancel: &CancellationToken) -> Result<(), FileError> {
    if cancel.is_cancelled() {
        Err(FileError::Cancelled)
    } else {
        Ok(())
    }
}

fn command_not_found_marker() -> &'static str {
    "__7z_command_not_found__"
}

#[derive(Debug, Clone, Copy)]
enum TarCompression {
    Plain,
    Gzip,
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use super::*;

    fn seven_zip_test_request(password: Option<ArchivePassword>) -> ArchiveExtractionRequest {
        ArchiveExtractionRequest {
            archive: PathBuf::from("/tmp/locked.7z"),
            destination: PathBuf::from("/tmp/locked"),
            password,
        }
    }

    fn seven_zip_exit_status(code: i32) -> std::process::ExitStatus {
        std::process::ExitStatus::from_raw(code << 8)
    }

    #[test]
    fn seven_zip_extract_output_switch_precedes_archive_operand() {
        let request = seven_zip_test_request(None);
        let command = seven_zip_extract_command("7z", &request);
        let arguments = command
            .as_std()
            .get_args()
            .map(OsStr::to_os_string)
            .collect::<Vec<_>>();
        let output_switch = OsString::from(format!("-o{}", request.destination.to_string_lossy()));
        let output_index = arguments
            .iter()
            .position(|argument| argument == &output_switch)
            .unwrap();
        let archive_separator_index = arguments
            .iter()
            .position(|argument| argument == OsStr::new("--"))
            .unwrap();

        assert!(output_index < archive_separator_index);
        assert_eq!(
            arguments.get(archive_separator_index + 1),
            Some(&request.archive.as_os_str().to_os_string())
        );
    }

    #[test]
    fn seven_zip_stdout_password_prompt_requires_password() {
        let request = seven_zip_test_request(None);
        let error = seven_zip_error(
            &request,
            seven_zip_exit_status(255),
            "Enter password:".to_owned(),
            "Break signaled".to_owned(),
        );

        assert!(matches!(
            error,
            FileError::ArchivePasswordRequired { path } if path == request.archive
        ));
    }

    #[test]
    fn seven_zip_wrong_password_reports_invalid_password() {
        let request = seven_zip_test_request(ArchivePassword::new("wrong"));
        let error = seven_zip_error(
            &request,
            seven_zip_exit_status(2),
            String::new(),
            "Cannot open encrypted archive. Wrong password?".to_owned(),
        );

        assert!(matches!(
            error,
            FileError::ArchiveInvalidPassword { path } if path == request.archive
        ));
    }
}
