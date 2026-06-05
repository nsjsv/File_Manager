use std::path::{Path, PathBuf};

use file_core::{DirectoryEntry, ScanOptions};
use iced::Command;

use crate::app::FileBrowser;
use crate::commands::preview_directory_children_command;
use crate::model::{
    Message, PreviewContent, PreviewState, PreviewTreeDirectoryChildren, PreviewTreeEntry,
};

const PREVIEW_TREE_TOGGLE_ROTATION_STEP: f32 = 0.18;
const PREVIEW_TREE_TOGGLE_ROTATION_EPSILON: f32 = 0.001;

impl FileBrowser {
    pub(in crate::app) fn toggle_preview_tree_directory(
        &mut self,
        entry_id: usize,
    ) -> Command<Message> {
        let options = self.options.clone();
        match &mut self.preview {
            Some(PreviewState::Ready(PreviewContent::Directory { entries, .. })) => {
                toggle_directory_preview_tree_entry(entries, entry_id, options)
            }
            Some(PreviewState::Ready(PreviewContent::Archive { entries, .. })) => {
                toggle_loaded_preview_tree_entry(entries, entry_id)
            }
            _ => Command::none(),
        }
    }

    pub(in crate::app) fn accept_preview_directory_children(
        &mut self,
        parent_path: PathBuf,
        children_outcome: Result<Vec<DirectoryEntry>, String>,
    ) -> Command<Message> {
        let Some(entries) = directory_preview_entries_mut(self.preview.as_mut()) else {
            return Command::none();
        };
        match children_outcome {
            Ok(children) => {
                accept_loaded_preview_directory_children(entries, &parent_path, children)
            }
            Err(error) => accept_preview_directory_children_error(entries, &parent_path, error),
        }
        Command::none()
    }

    pub(in crate::app) fn preview_tree_animation_is_active(&self) -> bool {
        let Some(entries) = preview_tree_entries(self.preview.as_ref()) else {
            return false;
        };
        entries.iter().any(preview_tree_rotation_is_active)
    }

    pub(in crate::app) fn advance_preview_tree_animation(&mut self) -> Command<Message> {
        let Some(entries) = preview_tree_entries_mut(self.preview.as_mut()) else {
            return Command::none();
        };

        for entry in entries.iter_mut().filter(|entry| entry.is_directory()) {
            let target = preview_tree_rotation_target(entry);
            if entry.toggle_rotation_progress < target {
                entry.toggle_rotation_progress = (entry.toggle_rotation_progress
                    + PREVIEW_TREE_TOGGLE_ROTATION_STEP)
                    .min(target);
            } else if entry.toggle_rotation_progress > target {
                entry.toggle_rotation_progress = (entry.toggle_rotation_progress
                    - PREVIEW_TREE_TOGGLE_ROTATION_STEP)
                    .max(target);
            }
        }

        Command::none()
    }
}

fn toggle_directory_preview_tree_entry(
    entries: &mut [PreviewTreeEntry],
    entry_id: usize,
    options: ScanOptions,
) -> Command<Message> {
    let Some(entry) = entries.get_mut(entry_id) else {
        return Command::none();
    };
    if !entry.is_directory() {
        return Command::none();
    }

    entry.is_expanded = !entry.is_expanded;
    if !entry.is_expanded || !directory_children_can_load(entry) {
        return Command::none();
    }

    let Some(path) = entry.filesystem_path.clone() else {
        return Command::none();
    };
    entry.directory_children = Some(PreviewTreeDirectoryChildren::Loading);
    preview_directory_children_command(path, options)
}

fn toggle_loaded_preview_tree_entry(
    entries: &mut [PreviewTreeEntry],
    entry_id: usize,
) -> Command<Message> {
    let Some(entry) = entries.get_mut(entry_id) else {
        return Command::none();
    };
    if entry.is_directory() {
        entry.is_expanded = !entry.is_expanded;
    }

    Command::none()
}

fn directory_children_can_load(entry: &PreviewTreeEntry) -> bool {
    matches!(
        entry.directory_children.as_ref(),
        Some(PreviewTreeDirectoryChildren::Pending | PreviewTreeDirectoryChildren::Error(_))
    )
}

fn directory_preview_entries_mut(
    preview: Option<&mut PreviewState>,
) -> Option<&mut Vec<PreviewTreeEntry>> {
    match preview? {
        PreviewState::Ready(PreviewContent::Directory { entries, .. }) => Some(entries),
        _ => None,
    }
}

fn accept_loaded_preview_directory_children(
    entries: &mut Vec<PreviewTreeEntry>,
    parent_path: &Path,
    children: Vec<DirectoryEntry>,
) {
    let Some(parent_id) = preview_tree_entry_index_for_path(entries, parent_path) else {
        return;
    };
    if !matches!(
        entries[parent_id].directory_children.as_ref(),
        Some(PreviewTreeDirectoryChildren::Loading)
    ) {
        return;
    }

    let insert_at = preview_tree_subtree_end(entries, parent_id);
    let child_count = children.len();
    shift_parent_ids_after_insertion(entries, insert_at, child_count);
    let child_depth = entries[parent_id].depth + 1;
    let child_entries = children
        .into_iter()
        .enumerate()
        .map(|(offset, entry)| {
            PreviewTreeEntry::from_directory_entry(
                insert_at + offset,
                entry,
                child_depth,
                Some(parent_id),
            )
        })
        .collect::<Vec<_>>();
    entries.splice(insert_at..insert_at, child_entries);
    entries[parent_id].directory_children = Some(PreviewTreeDirectoryChildren::Loaded);
    renumber_preview_tree_entries(entries);
}

fn accept_preview_directory_children_error(
    entries: &mut [PreviewTreeEntry],
    parent_path: &Path,
    error: String,
) {
    let Some(parent_id) = preview_tree_entry_index_for_path(entries, parent_path) else {
        return;
    };
    if matches!(
        entries[parent_id].directory_children.as_ref(),
        Some(PreviewTreeDirectoryChildren::Loading)
    ) {
        entries[parent_id].directory_children = Some(PreviewTreeDirectoryChildren::Error(error));
    }
}

fn preview_tree_entry_index_for_path(entries: &[PreviewTreeEntry], path: &Path) -> Option<usize> {
    entries
        .iter()
        .position(|entry| entry.filesystem_path.as_deref() == Some(path))
}

fn preview_tree_subtree_end(entries: &[PreviewTreeEntry], parent_id: usize) -> usize {
    let parent_depth = entries[parent_id].depth;
    let mut index = parent_id + 1;
    while entries
        .get(index)
        .is_some_and(|entry| entry.depth > parent_depth)
    {
        index += 1;
    }
    index
}

fn shift_parent_ids_after_insertion(
    entries: &mut [PreviewTreeEntry],
    insert_at: usize,
    child_count: usize,
) {
    if child_count == 0 {
        return;
    }

    for entry in entries.iter_mut().skip(insert_at) {
        if let Some(parent) = entry.parent.as_mut().filter(|parent| **parent >= insert_at) {
            *parent += child_count;
        }
    }
}

fn renumber_preview_tree_entries(entries: &mut [PreviewTreeEntry]) {
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.id = index;
    }
}

fn preview_tree_entries(preview: Option<&PreviewState>) -> Option<&[PreviewTreeEntry]> {
    match preview? {
        PreviewState::Ready(
            PreviewContent::Directory { entries, .. } | PreviewContent::Archive { entries, .. },
        ) => Some(entries),
        _ => None,
    }
}

fn preview_tree_entries_mut(preview: Option<&mut PreviewState>) -> Option<&mut [PreviewTreeEntry]> {
    match preview? {
        PreviewState::Ready(
            PreviewContent::Directory { entries, .. } | PreviewContent::Archive { entries, .. },
        ) => Some(entries),
        _ => None,
    }
}

fn preview_tree_rotation_is_active(entry: &PreviewTreeEntry) -> bool {
    entry.is_directory()
        && (entry.toggle_rotation_progress - preview_tree_rotation_target(entry)).abs()
            > PREVIEW_TREE_TOGGLE_ROTATION_EPSILON
}

fn preview_tree_rotation_target(entry: &PreviewTreeEntry) -> f32 {
    if entry.is_expanded {
        1.0
    } else {
        0.0
    }
}
