use iced::Command;

use crate::app::FileBrowser;
use crate::model::{Message, PreviewContent, PreviewState, PreviewTreeEntry};

const PREVIEW_TREE_TOGGLE_ROTATION_STEP: f32 = 0.18;
const PREVIEW_TREE_TOGGLE_ROTATION_EPSILON: f32 = 0.001;

impl FileBrowser {
    pub(in crate::app) fn toggle_preview_tree_directory(
        &mut self,
        entry_id: usize,
    ) -> Command<Message> {
        let Some(PreviewState::Ready(
            PreviewContent::Directory { entries, .. } | PreviewContent::Archive { entries, .. },
        )) = &mut self.preview
        else {
            return Command::none();
        };
        let Some(entry) = entries.get_mut(entry_id) else {
            return Command::none();
        };
        if entry.is_directory() {
            entry.is_expanded = !entry.is_expanded;
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
