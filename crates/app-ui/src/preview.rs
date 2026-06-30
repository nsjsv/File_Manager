use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use file_core::{
    archive_extraction_format_for_path, is_supported_audio_path, is_supported_video_path,
    list_archive_members, scan_directory, ArchiveListingEntry, DirectoryEntry, FileKind,
    ScanOptions,
};

use crate::audio_preview::inspect_audio_preview_metadata;
use crate::formatting::format_file_size;
use crate::model::{PreviewContent, PreviewTreeDirectoryChildren, PreviewTreeEntry};
use crate::text_preview_loading::load_initial_text_preview;

pub(crate) const PREVIEW_ARCHIVE_ENTRY_LIMIT: usize = 500;

const SUPPORTED_ARCHIVE_FORMAT_MESSAGE: &str =
    "Archive preview supports .zip, .tar, .tar.gz, .tgz, .7z, and .rar files";

pub(crate) async fn load_preview(
    path: PathBuf,
    kind: FileKind,
    options: ScanOptions,
    max_file_bytes: u64,
) -> Result<PreviewContent, String> {
    match kind {
        FileKind::Directory => load_directory_preview(path, options).await,
        FileKind::File => {
            reject_file_over_preview_limit(&path, max_file_bytes).await?;
            if archive_extraction_format_for_path(&path).is_some() {
                load_archive_preview(path).await
            } else if is_known_archive_path(&path) {
                Err(format!(
                    "This archive format is not supported yet. {SUPPORTED_ARCHIVE_FORMAT_MESSAGE}"
                ))
            } else if is_supported_audio_path(&path) {
                load_audio_preview(path).await
            } else if is_supported_video_path(&path) {
                load_video_preview(path).await
            } else {
                load_text_preview(path, max_file_bytes).await
            }
        }
        FileKind::Symlink | FileKind::Other => {
            Err("Preview is only available for directories, archives, audio files, images, and UTF-8 text files".to_owned())
        }
    }
}

async fn reject_file_over_preview_limit(path: &Path, max_file_bytes: u64) -> Result<(), String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| format!("could not inspect preview file: {error}"))?;
    if metadata.len() <= max_file_bytes {
        return Ok(());
    }

    Err(format!(
        "File is too large to preview ({}). Maximum preview size is {}.",
        format_file_size(metadata.len()),
        format_file_size(max_file_bytes)
    ))
}

async fn load_directory_preview(
    path: PathBuf,
    options: ScanOptions,
) -> Result<PreviewContent, String> {
    let directory_entries = load_directory_preview_children(path, options).await?;
    let entries = directory_entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| PreviewTreeEntry::from_directory_entry(index, entry, 0, None))
        .collect();

    Ok(PreviewContent::Directory { entries })
}

pub(crate) async fn load_directory_preview_children(
    path: PathBuf,
    options: ScanOptions,
) -> Result<Vec<DirectoryEntry>, String> {
    scan_directory(path, options)
        .await
        .map(|scan| scan.entries)
        .map_err(|error| error.to_string())
}

async fn load_archive_preview(path: PathBuf) -> Result<PreviewContent, String> {
    let members = list_archive_members(path)
        .await
        .map_err(|error| error.to_string())?;
    let mut tree_builder = ArchiveTreeBuilder::new();
    for member in members {
        tree_builder.insert_member(member);
    }
    let archive_preview = tree_builder.finish(PREVIEW_ARCHIVE_ENTRY_LIMIT);

    Ok(PreviewContent::Archive {
        entries: archive_preview.entries,
    })
}

fn is_known_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            [
                "zip", "tar", "gz", "tgz", "xz", "bz2", "7z", "rar", "zst", "deb", "rpm",
            ]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

async fn load_audio_preview(path: PathBuf) -> Result<PreviewContent, String> {
    let preview_path = path.clone();
    let metadata = inspect_audio_preview_metadata(path).await?;

    Ok(PreviewContent::Audio {
        path: preview_path,
        duration: metadata.duration,
        len: metadata.len,
    })
}

async fn load_video_preview(path: PathBuf) -> Result<PreviewContent, String> {
    Ok(PreviewContent::Video {
        path,
        frame: None,
        width: 0,
        height: 0,
        duration: None,
    })
}

struct ArchivePreview {
    entries: Vec<PreviewTreeEntry>,
}

struct ArchiveTreeBuilder {
    nodes: Vec<ArchiveTreeNode>,
    children_by_parent: Vec<BTreeMap<String, usize>>,
    root_children: BTreeMap<String, usize>,
}

impl ArchiveTreeBuilder {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            children_by_parent: Vec::new(),
            root_children: BTreeMap::new(),
        }
    }

    fn insert_member(&mut self, member: ArchiveListingEntry) {
        let segments = archive_path_segments(&member.path);
        if segments.is_empty() {
            return;
        }

        let mut parent = None;
        for (depth, segment) in segments.iter().enumerate() {
            let is_leaf = depth + 1 == segments.len();
            let kind = if is_leaf {
                member.kind
            } else {
                FileKind::Directory
            };
            parent = Some(self.insert_segment(parent, segment, kind));
        }
    }

    fn insert_segment(&mut self, parent: Option<usize>, name: &str, kind: FileKind) -> usize {
        if let Some(existing_index) = self.child_index(parent, name) {
            if kind == FileKind::Directory {
                self.nodes[existing_index].kind = FileKind::Directory;
            }
            return existing_index;
        }

        let node_index = self.nodes.len();
        self.nodes.push(ArchiveTreeNode {
            name: name.to_owned(),
            kind,
        });
        self.children_by_parent.push(BTreeMap::new());
        self.children_mut(parent)
            .insert(name.to_owned(), node_index);
        node_index
    }

    fn child_index(&self, parent: Option<usize>, name: &str) -> Option<usize> {
        match parent {
            Some(parent_index) => self.children_by_parent[parent_index].get(name).copied(),
            None => self.root_children.get(name).copied(),
        }
    }

    fn children_mut(&mut self, parent: Option<usize>) -> &mut BTreeMap<String, usize> {
        match parent {
            Some(parent_index) => &mut self.children_by_parent[parent_index],
            None => &mut self.root_children,
        }
    }

    fn finish(self, limit: usize) -> ArchivePreview {
        let mut entries = Vec::new();
        let root_children = self.sorted_child_indices(None);
        for node_index in root_children {
            self.append_flattened_entry(node_index, None, 0, limit, &mut entries);
            if entries.len() >= limit {
                break;
            }
        }

        ArchivePreview { entries }
    }

    fn append_flattened_entry(
        &self,
        node_index: usize,
        parent: Option<usize>,
        depth: usize,
        limit: usize,
        entries: &mut Vec<PreviewTreeEntry>,
    ) {
        if entries.len() >= limit {
            return;
        }

        let node = &self.nodes[node_index];
        let entry_id = entries.len();
        entries.push(PreviewTreeEntry {
            id: entry_id,
            name: node.name.clone(),
            kind: node.kind,
            depth,
            parent,
            filesystem_path: None,
            directory_children: preview_archive_directory_children(node.kind),
            is_expanded: true,
            toggle_rotation_progress: if node.kind == FileKind::Directory {
                1.0
            } else {
                0.0
            },
        });

        if node.kind != FileKind::Directory {
            return;
        }

        for child_index in self.sorted_child_indices(Some(node_index)) {
            self.append_flattened_entry(child_index, Some(entry_id), depth + 1, limit, entries);
            if entries.len() >= limit {
                break;
            }
        }
    }

    fn sorted_child_indices(&self, parent: Option<usize>) -> Vec<usize> {
        let children = match parent {
            Some(parent_index) => &self.children_by_parent[parent_index],
            None => &self.root_children,
        };
        let mut indices = children.values().copied().collect::<Vec<_>>();
        indices.sort_by(|left, right| archive_node_order(&self.nodes[*left], &self.nodes[*right]));
        indices
    }
}

fn preview_archive_directory_children(kind: FileKind) -> Option<PreviewTreeDirectoryChildren> {
    (kind == FileKind::Directory).then_some(PreviewTreeDirectoryChildren::Loaded)
}

struct ArchiveTreeNode {
    name: String,
    kind: FileKind,
}

fn archive_node_order(left: &ArchiveTreeNode, right: &ArchiveTreeNode) -> std::cmp::Ordering {
    archive_kind_rank(left.kind)
        .cmp(&archive_kind_rank(right.kind))
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.name.cmp(&right.name))
}

fn archive_kind_rank(kind: FileKind) -> u8 {
    match kind {
        FileKind::Directory => 0,
        FileKind::File => 1,
        FileKind::Symlink => 2,
        FileKind::Other => 3,
    }
}

fn archive_path_segments(path: &str) -> Vec<&str> {
    path.split(['/', '\\'])
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect()
}

async fn load_text_preview(path: PathBuf, max_file_bytes: u64) -> Result<PreviewContent, String> {
    load_initial_text_preview(path, max_file_bytes).await
}

#[cfg(test)]
mod tests;
