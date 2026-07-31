use std::path::{Path, PathBuf};

use tokio::fs;

use super::{inspect_file_identity, FileIdentity, FileObjectKind, RecoverableTransferError};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceManifestEntry {
    #[serde(with = "super::path_codec")]
    pub relative_path: PathBuf,
    pub identity: FileIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceManifest {
    #[serde(with = "super::path_codec")]
    pub root: PathBuf,
    pub entries: Vec<SourceManifestEntry>,
}

struct PendingDirectory {
    path: PathBuf,
    relative_path: PathBuf,
    expected_identity: FileIdentity,
}

pub async fn build_source_manifest(
    root: &Path,
) -> Result<SourceManifest, RecoverableTransferError> {
    let root_identity = inspect_file_identity(root).await?;
    let mut entries = vec![SourceManifestEntry {
        relative_path: PathBuf::new(),
        identity: root_identity.clone(),
    }];
    let mut pending_directories = Vec::new();
    if root_identity.object_kind == FileObjectKind::Directory {
        pending_directories.push(PendingDirectory {
            path: root.to_path_buf(),
            relative_path: PathBuf::new(),
            expected_identity: root_identity,
        });
    }

    while let Some(directory) = pending_directories.pop() {
        let before = inspect_file_identity(&directory.path).await?;
        if before != directory.expected_identity {
            return Err(RecoverableTransferError::SourceChanged {
                path: directory.path,
            });
        }

        let mut reader = fs::read_dir(&directory.path).await.map_err(|source| {
            RecoverableTransferError::file_system("read directory", &directory.path, source)
        })?;
        let mut children = Vec::new();
        while let Some(child) = reader.next_entry().await.map_err(|source| {
            RecoverableTransferError::file_system(
                "read directory entry in",
                &directory.path,
                source,
            )
        })? {
            children.push((child.file_name(), child.path()));
        }
        children.sort_by(|(left, _), (right, _)| left.cmp(right));

        for (name, child_path) in children {
            let relative_path = directory.relative_path.join(name);
            let identity = inspect_file_identity(&child_path).await?;
            entries.push(SourceManifestEntry {
                relative_path: relative_path.clone(),
                identity: identity.clone(),
            });
            if identity.object_kind == FileObjectKind::Directory {
                pending_directories.push(PendingDirectory {
                    path: child_path,
                    relative_path,
                    expected_identity: identity,
                });
            }
        }

        let after = inspect_file_identity(&directory.path).await?;
        if after != before {
            return Err(RecoverableTransferError::SourceChanged {
                path: directory.path,
            });
        }
    }

    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(SourceManifest {
        root: root.to_path_buf(),
        entries,
    })
}

pub async fn verify_source_manifest(
    expected: &SourceManifest,
) -> Result<(), RecoverableTransferError> {
    let actual = build_source_manifest(&expected.root).await?;
    if actual == *expected {
        return Ok(());
    }
    Err(RecoverableTransferError::SourceChanged {
        path: expected.root.clone(),
    })
}
