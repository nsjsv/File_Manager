use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use super::super::FileOperationControls;
use super::RecoverableTransferError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ObjectFingerprint(pub [u8; 32]);

pub async fn fingerprint_object(
    root: &Path,
) -> Result<ObjectFingerprint, RecoverableTransferError> {
    fingerprint_object_with_checkpoint(root, || Ok(())).await
}

pub async fn fingerprint_object_with_controls(
    root: &Path,
    controls: &FileOperationControls,
) -> Result<ObjectFingerprint, RecoverableTransferError> {
    let controls = controls.clone();
    fingerprint_object_with_checkpoint(root, move || controls.checkpoint_now().map_err(Into::into))
        .await
}

async fn fingerprint_object_with_checkpoint<C>(
    root: &Path,
    checkpoint: C,
) -> Result<ObjectFingerprint, RecoverableTransferError>
where
    C: FnMut() -> Result<(), RecoverableTransferError> + Send + 'static,
{
    let root = root.to_path_buf();
    let error_path = root.clone();
    tokio::task::spawn_blocking(move || fingerprint_object_blocking(&root, checkpoint))
        .await
        .map_err(|join_error| {
            RecoverableTransferError::file_system(
                "join object fingerprint task for",
                &error_path,
                std::io::Error::other(join_error),
            )
        })?
}

fn fingerprint_object_blocking<C>(
    root: &Path,
    mut checkpoint: C,
) -> Result<ObjectFingerprint, RecoverableTransferError>
where
    C: FnMut() -> Result<(), RecoverableTransferError>,
{
    let mut hasher = blake3::Hasher::new();
    let mut pending = vec![(PathBuf::new(), root.to_path_buf())];
    let mut buffer = vec![0; 1024 * 1024];

    while let Some((relative_path, path)) = pending.pop() {
        checkpoint()?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|source| {
            RecoverableTransferError::file_system("read fingerprint metadata for", &path, source)
        })?;
        update_component(&mut hasher, relative_path.as_os_str());
        let file_type = metadata.file_type();
        if file_type.is_file() {
            hasher.update(b"file\0");
            hasher.update(&metadata.len().to_le_bytes());
            let mut file = std::fs::File::open(&path).map_err(|source| {
                RecoverableTransferError::file_system("open fingerprint file", &path, source)
            })?;
            loop {
                checkpoint()?;
                let read = file.read(&mut buffer).map_err(|source| {
                    RecoverableTransferError::file_system("read fingerprint file", &path, source)
                })?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            continue;
        }
        if file_type.is_symlink() {
            hasher.update(b"symlink\0");
            let target = std::fs::read_link(&path).map_err(|source| {
                RecoverableTransferError::file_system(
                    "read fingerprint symbolic link",
                    &path,
                    source,
                )
            })?;
            update_component(&mut hasher, target.as_os_str());
            continue;
        }
        if !file_type.is_dir() {
            return Err(RecoverableTransferError::UnsupportedObject { path });
        }

        hasher.update(b"directory\0");
        let entries = std::fs::read_dir(&path).map_err(|source| {
            RecoverableTransferError::file_system("read fingerprint directory", &path, source)
        })?;
        let mut children = Vec::new();
        for entry in entries {
            checkpoint()?;
            let entry = entry.map_err(|source| {
                RecoverableTransferError::file_system(
                    "read fingerprint directory entry",
                    &path,
                    source,
                )
            })?;
            children.push((entry.file_name(), entry.path()));
        }
        children.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (name, child_path) in children.into_iter().rev() {
            pending.push((relative_path.join(name), child_path));
        }
    }

    Ok(ObjectFingerprint(*hasher.finalize().as_bytes()))
}

fn update_component(hasher: &mut blake3::Hasher, value: &OsStr) {
    #[cfg(unix)]
    {
        let bytes = value.as_bytes();
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    #[cfg(not(unix))]
    {
        let encoded = value.to_string_lossy();
        let bytes = encoded.as_bytes();
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::FileOperationRunState;
    use tokio::sync::watch;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn controlled_fingerprint_stops_before_reading_content() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("content.bin");
        std::fs::write(&path, vec![7_u8; 1024 * 1024]).unwrap();
        let (_sender, receiver) = watch::channel(FileOperationRunState::ApplicationStopping);
        let controls = FileOperationControls::new(CancellationToken::new(), receiver);

        let error = fingerprint_object_with_controls(&path, &controls)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RecoverableTransferError::FileOperation(crate::FileError::ApplicationStopping)
        ));
    }
}
