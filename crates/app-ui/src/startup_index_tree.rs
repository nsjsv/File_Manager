use std::collections::HashSet;
use std::path::{Path, PathBuf};

use file_core::{DirectoryEntry, FileKind};
use file_index::MediaMetadataScope;

#[derive(Debug, Clone)]
pub(crate) struct StartupIndexSetupState {
    pub(crate) common_roots: Vec<StartupIndexRootSeed>,
    pub(crate) custom_root: Option<StartupIndexRootSeed>,
    pub(crate) entries: Vec<StartupIndexTreeEntry>,
    pub(crate) target_mode: Option<StartupIndexTargetMode>,
    pub(crate) show_hidden_entries: bool,
    pub(crate) capability: Option<StartupIndexCapability>,
    selection_drag_last_entry: Option<usize>,
    selection_drag_action: Option<StartupIndexEntrySelection>,
    selection_range_anchor: Option<usize>,
    directory_load_generation: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct StartupIndexTreeEntry {
    pub(crate) id: usize,
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) kind: FileKind,
    pub(crate) depth: usize,
    pub(crate) parent: Option<usize>,
    pub(crate) directory_children: Option<StartupIndexDirectoryChildren>,
    pub(crate) is_expanded: bool,
    pub(crate) toggle_rotation_progress: f32,
    pub(crate) selection: StartupIndexEntrySelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupIndexEntrySelection {
    Selected,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupIndexTargetMode {
    Common,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupIndexCapability {
    Filenames,
    Text,
    TextAndImageMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartupIndexDirectoryChildren {
    Pending,
    Loading,
    Loaded,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupIndexRootSeed {
    pub(crate) label: String,
    pub(crate) path: PathBuf,
    pub(crate) selection: StartupIndexEntrySelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupIndexBuildRequest {
    pub(crate) root: PathBuf,
    pub(crate) selected_paths: Vec<PathBuf>,
}

impl StartupIndexSetupState {
    pub(crate) fn from_choices(
        common_roots: Vec<StartupIndexRootSeed>,
        custom_root: Option<StartupIndexRootSeed>,
    ) -> Option<Self> {
        (!common_roots.is_empty() || custom_root.is_some()).then_some(Self {
            common_roots,
            custom_root,
            entries: Vec::new(),
            target_mode: None,
            show_hidden_entries: false,
            capability: None,
            selection_drag_last_entry: None,
            selection_drag_action: None,
            selection_range_anchor: None,
            directory_load_generation: 0,
        })
    }

    pub(crate) fn merge_choices(
        &mut self,
        common_roots: Vec<StartupIndexRootSeed>,
        custom_root: Option<StartupIndexRootSeed>,
    ) {
        self.merge_common_roots(common_roots);
        if self.custom_root.is_none() {
            self.custom_root = custom_root;
        }
        if self.target_mode == Some(StartupIndexTargetMode::Custom) {
            self.ensure_custom_root_entry();
        }
    }

    fn merge_common_roots(&mut self, roots: Vec<StartupIndexRootSeed>) {
        let mut existing_paths = self
            .common_roots
            .iter()
            .map(|root| root.path.clone())
            .collect::<HashSet<_>>();

        for root in roots {
            if !existing_paths.insert(root.path.clone()) {
                continue;
            }
            self.common_roots.push(root);
        }
    }

    pub(crate) fn select_target_mode(&mut self, mode: StartupIndexTargetMode) -> Vec<PathBuf> {
        self.target_mode = Some(mode);
        if mode != StartupIndexTargetMode::Custom {
            return Vec::new();
        }
        self.ensure_custom_root_entry();
        self.expand_roots_waiting_for_children()
    }

    pub(crate) fn expand_roots_waiting_for_children(&mut self) -> Vec<PathBuf> {
        if self.target_mode != Some(StartupIndexTargetMode::Custom) {
            return Vec::new();
        }
        self.entries
            .iter_mut()
            .filter(|entry| entry.parent.is_none())
            .filter_map(|entry| {
                if !entry.is_directory() || !entry.children_can_load() {
                    return None;
                }

                entry.is_expanded = true;
                entry.directory_children = Some(StartupIndexDirectoryChildren::Loading);
                Some(entry.path.clone())
            })
            .collect()
    }

    pub(crate) fn toggle_hidden_content_visibility(&mut self) -> Vec<PathBuf> {
        self.show_hidden_entries = !self.show_hidden_entries;
        self.directory_load_generation = self.directory_load_generation.wrapping_add(1);
        self.reload_expanded_roots()
    }

    pub(crate) fn directory_load_generation(&self) -> u64 {
        self.directory_load_generation
    }

    pub(crate) fn toggle_entry_selection(&mut self, entry_id: usize) {
        self.toggle_single_entry_selection(entry_id);
        self.selection_range_anchor = Some(entry_id);
    }

    pub(crate) fn select_entry_range(&mut self, entry_id: usize, visible_entry_ids: &[usize]) {
        let Some(anchor) = self.selection_range_anchor else {
            self.toggle_entry_selection(entry_id);
            return;
        };
        let Some(anchor_position) = visible_entry_ids.iter().position(|id| *id == anchor) else {
            self.toggle_entry_selection(entry_id);
            return;
        };
        let Some(entry_position) = visible_entry_ids.iter().position(|id| *id == entry_id) else {
            self.toggle_entry_selection(entry_id);
            return;
        };

        let start = anchor_position.min(entry_position);
        let end = anchor_position.max(entry_position);
        let selection = self
            .entries
            .get(anchor)
            .map(|entry| entry.selection)
            .unwrap_or(StartupIndexEntrySelection::Selected);
        self.apply_entry_selection_to_visible_range(start, end, visible_entry_ids, selection);
        self.selection_range_anchor = Some(entry_id);
    }

    pub(crate) fn start_entry_selection_drag(&mut self, entry_id: usize) {
        let Some(selection) = self.entries.get(entry_id).map(|entry| entry.selection) else {
            return;
        };
        let action = selection.toggled();
        self.selection_drag_last_entry = Some(entry_id);
        self.selection_drag_action = Some(action);
        self.selection_range_anchor = Some(entry_id);
        self.apply_entry_selection_from_gesture(entry_id, action);
    }

    pub(crate) fn enter_entry_during_selection_drag(
        &mut self,
        entry_id: usize,
        visible_entry_ids: &[usize],
    ) {
        let Some(action) = self.selection_drag_action else {
            return;
        };
        if let Some((start, end)) = self.drag_selection_visible_range(entry_id, visible_entry_ids) {
            self.apply_entry_selection_to_visible_range(start, end, visible_entry_ids, action);
        } else {
            self.apply_entry_selection_from_gesture(entry_id, action);
        }
        self.selection_drag_last_entry = Some(entry_id);
        self.selection_range_anchor = Some(entry_id);
    }

    pub(crate) fn finish_entry_selection_drag(&mut self) {
        self.selection_drag_last_entry = None;
        self.selection_drag_action = None;
    }

    fn drag_selection_visible_range(
        &self,
        entry_id: usize,
        visible_entry_ids: &[usize],
    ) -> Option<(usize, usize)> {
        let previous_id = self.selection_drag_last_entry?;
        let previous_position = visible_entry_ids.iter().position(|id| *id == previous_id)?;
        let entry_position = visible_entry_ids.iter().position(|id| *id == entry_id)?;
        Some((
            previous_position.min(entry_position),
            previous_position.max(entry_position),
        ))
    }

    fn toggle_single_entry_selection(&mut self, entry_id: usize) {
        let Some(selection) = self.entries.get(entry_id).map(|entry| entry.selection) else {
            return;
        };

        match selection {
            StartupIndexEntrySelection::Selected => {
                self.set_entry_and_loaded_descendants(
                    entry_id,
                    StartupIndexEntrySelection::Skipped,
                );
                self.set_ancestors(entry_id, StartupIndexEntrySelection::Skipped);
            }
            StartupIndexEntrySelection::Skipped => {
                self.set_entry_and_loaded_descendants(
                    entry_id,
                    StartupIndexEntrySelection::Selected,
                );
            }
        }
    }

    fn apply_entry_selection_from_gesture(
        &mut self,
        entry_id: usize,
        selection: StartupIndexEntrySelection,
    ) {
        self.set_entry_and_loaded_descendants(entry_id, selection);
        if selection == StartupIndexEntrySelection::Skipped {
            self.set_ancestors(entry_id, selection);
        }
    }

    fn apply_entry_selection_to_visible_range(
        &mut self,
        start: usize,
        end: usize,
        visible_entry_ids: &[usize],
        selection: StartupIndexEntrySelection,
    ) {
        for selected_id in &visible_entry_ids[start..=end] {
            self.apply_entry_selection_from_gesture(*selected_id, selection);
        }
    }

    pub(crate) fn select_capability(&mut self, capability: StartupIndexCapability) {
        self.capability = Some(capability);
    }

    pub(crate) fn toggle_directory(&mut self, entry_id: usize) -> Option<PathBuf> {
        let entry = self.entries.get_mut(entry_id)?;
        if !entry.is_directory() {
            return None;
        }

        entry.is_expanded = !entry.is_expanded;
        if !entry.is_expanded || !entry.children_can_load() {
            return None;
        }

        entry.directory_children = Some(StartupIndexDirectoryChildren::Loading);
        Some(entry.path.clone())
    }

    pub(crate) fn accept_directory_children(
        &mut self,
        parent_path: &Path,
        children: Vec<DirectoryEntry>,
    ) {
        let Some(parent_id) = self.entry_index_for_path(parent_path) else {
            return;
        };
        if !matches!(
            self.entries[parent_id].directory_children.as_ref(),
            Some(StartupIndexDirectoryChildren::Loading)
        ) {
            return;
        }

        let insert_at = self.subtree_end(parent_id);
        let child_count = children.len();
        self.shift_parent_ids_after_insertion(insert_at, child_count);
        let child_depth = self.entries[parent_id].depth + 1;
        let child_selection = self.entries[parent_id].selection;
        let child_entries = children
            .into_iter()
            .enumerate()
            .map(|(offset, child)| {
                StartupIndexTreeEntry::from_directory_entry(
                    insert_at + offset,
                    child,
                    child_depth,
                    Some(parent_id),
                    child_selection,
                )
            })
            .collect::<Vec<_>>();

        self.entries.splice(insert_at..insert_at, child_entries);
        self.entries[parent_id].directory_children = Some(StartupIndexDirectoryChildren::Loaded);
        self.renumber_entries();
    }

    pub(crate) fn accept_directory_error(&mut self, parent_path: &Path, error: String) {
        let Some(parent_id) = self.entry_index_for_path(parent_path) else {
            return;
        };
        if matches!(
            self.entries[parent_id].directory_children.as_ref(),
            Some(StartupIndexDirectoryChildren::Loading)
        ) {
            self.entries[parent_id].directory_children =
                Some(StartupIndexDirectoryChildren::Error(error));
        }
    }

    pub(crate) fn has_selected_entries(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.selection == StartupIndexEntrySelection::Selected)
    }

    pub(crate) fn can_accept(&self) -> bool {
        self.capability.is_some()
            && match self.target_mode {
                Some(StartupIndexTargetMode::Common) => !self.common_roots.is_empty(),
                Some(StartupIndexTargetMode::Custom) => self.has_selected_entries(),
                None => false,
            }
    }

    pub(crate) fn selected_index_requests(&self) -> Vec<StartupIndexBuildRequest> {
        match self.target_mode {
            Some(StartupIndexTargetMode::Common) => self.common_index_requests(),
            Some(StartupIndexTargetMode::Custom) => self.custom_index_requests(),
            None => Vec::new(),
        }
    }

    fn common_index_requests(&self) -> Vec<StartupIndexBuildRequest> {
        self.common_roots
            .iter()
            .map(|root| StartupIndexBuildRequest {
                root: root.path.clone(),
                selected_paths: vec![root.path.clone()],
            })
            .collect()
    }

    fn custom_index_requests(&self) -> Vec<StartupIndexBuildRequest> {
        let mut requests = Vec::new();
        for entry_id in 0..self.entries.len() {
            if !self.entry_is_selected_without_selected_ancestor(entry_id) {
                continue;
            }
            let entry = &self.entries[entry_id];
            if entry.is_directory() {
                requests.push(StartupIndexBuildRequest {
                    root: entry.path.clone(),
                    selected_paths: vec![entry.path.clone()],
                });
            } else if let Some(parent) = entry.path.parent() {
                push_file_selected_index_request(
                    &mut requests,
                    parent.to_path_buf(),
                    entry.path.clone(),
                );
            }
        }
        requests
    }

    fn entry_is_selected_without_selected_ancestor(&self, entry_id: usize) -> bool {
        let Some(entry) = self.entries.get(entry_id) else {
            return false;
        };
        if entry.selection != StartupIndexEntrySelection::Selected {
            return false;
        }

        let mut parent = entry.parent;
        while let Some(parent_id) = parent {
            let Some(parent_entry) = self.entries.get(parent_id) else {
                return false;
            };
            if parent_entry.selection == StartupIndexEntrySelection::Selected {
                return false;
            }
            parent = parent_entry.parent;
        }
        true
    }

    fn ensure_custom_root_entry(&mut self) {
        if !self.entries.is_empty() {
            return;
        }
        let Some(root) = self.custom_root.clone() else {
            return;
        };
        self.entries.push(StartupIndexTreeEntry::from_root(0, root));
    }

    fn set_entry_and_loaded_descendants(
        &mut self,
        entry_id: usize,
        selection: StartupIndexEntrySelection,
    ) {
        let subtree_end = self.subtree_end(entry_id);
        for entry in self.entries.iter_mut().take(subtree_end).skip(entry_id) {
            entry.selection = selection;
        }
    }

    fn set_ancestors(&mut self, entry_id: usize, selection: StartupIndexEntrySelection) {
        let mut parent = self.entries.get(entry_id).and_then(|entry| entry.parent);
        while let Some(parent_id) = parent {
            let Some(parent_entry) = self.entries.get_mut(parent_id) else {
                return;
            };
            parent_entry.selection = selection;
            parent = parent_entry.parent;
        }
    }

    fn entry_index_for_path(&self, path: &Path) -> Option<usize> {
        self.entries.iter().position(|entry| entry.path == path)
    }

    fn subtree_end(&self, parent_id: usize) -> usize {
        let parent_depth = self.entries[parent_id].depth;
        let mut index = parent_id + 1;
        while self
            .entries
            .get(index)
            .is_some_and(|entry| entry.depth > parent_depth)
        {
            index += 1;
        }
        index
    }

    fn shift_parent_ids_after_insertion(&mut self, insert_at: usize, child_count: usize) {
        if child_count == 0 {
            return;
        }

        for entry in self.entries.iter_mut().skip(insert_at) {
            if let Some(parent) = entry.parent.as_mut().filter(|parent| **parent >= insert_at) {
                *parent += child_count;
            }
        }
    }

    fn renumber_entries(&mut self) {
        for (index, entry) in self.entries.iter_mut().enumerate() {
            entry.id = index;
        }
    }

    fn reload_expanded_roots(&mut self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let mut load_paths = Vec::new();

        for entry in self.entries.iter().filter(|entry| entry.parent.is_none()) {
            let mut root = entry.clone();
            root.id = roots.len();
            root.depth = 0;
            root.parent = None;
            if root.is_expanded {
                root.directory_children = Some(StartupIndexDirectoryChildren::Loading);
                load_paths.push(root.path.clone());
            } else {
                root.directory_children = Some(StartupIndexDirectoryChildren::Pending);
            }
            roots.push(root);
        }

        self.entries = roots;
        load_paths
    }
}

fn push_file_selected_index_request(
    requests: &mut Vec<StartupIndexBuildRequest>,
    root: PathBuf,
    selected_path: PathBuf,
) {
    if let Some(request) = requests.iter_mut().find(|request| request.root == root) {
        request.selected_paths.push(selected_path);
    } else {
        requests.push(StartupIndexBuildRequest {
            root,
            selected_paths: vec![selected_path],
        });
    }
}

impl StartupIndexTreeEntry {
    fn from_root(id: usize, root: StartupIndexRootSeed) -> Self {
        Self {
            id,
            name: root.label,
            path: root.path,
            kind: FileKind::Directory,
            depth: 0,
            parent: None,
            directory_children: Some(StartupIndexDirectoryChildren::Pending),
            is_expanded: false,
            toggle_rotation_progress: 0.0,
            selection: root.selection,
        }
    }

    fn from_directory_entry(
        id: usize,
        entry: DirectoryEntry,
        depth: usize,
        parent: Option<usize>,
        selection: StartupIndexEntrySelection,
    ) -> Self {
        let kind = entry.kind;
        Self {
            id,
            name: entry.name().to_string_lossy().into_owned(),
            path: entry.path,
            kind,
            depth,
            parent,
            directory_children: startup_index_directory_children(kind),
            is_expanded: false,
            toggle_rotation_progress: 0.0,
            selection,
        }
    }

    pub(crate) fn is_directory(&self) -> bool {
        self.kind == FileKind::Directory
    }

    fn children_can_load(&self) -> bool {
        matches!(
            self.directory_children.as_ref(),
            Some(StartupIndexDirectoryChildren::Pending | StartupIndexDirectoryChildren::Error(_))
        )
    }
}

impl StartupIndexEntrySelection {
    pub(crate) fn is_selected(self) -> bool {
        self == Self::Selected
    }

    fn toggled(self) -> Self {
        match self {
            Self::Selected => Self::Skipped,
            Self::Skipped => Self::Selected,
        }
    }
}

impl StartupIndexCapability {
    pub(crate) fn content_enabled(self) -> bool {
        matches!(self, Self::Text | Self::TextAndImageMetadata)
    }

    pub(crate) fn media_metadata_scope(self) -> MediaMetadataScope {
        match self {
            Self::Filenames | Self::Text => MediaMetadataScope::Off,
            Self::TextAndImageMetadata => MediaMetadataScope::Images,
        }
    }
}

fn startup_index_directory_children(kind: FileKind) -> Option<StartupIndexDirectoryChildren> {
    (kind == FileKind::Directory).then_some(StartupIndexDirectoryChildren::Pending)
}

#[cfg(test)]
mod tests;
