use std::collections::HashSet;
use std::path::{Path, PathBuf};

use file_core::{DirectoryEntry, FileKind};

#[derive(Debug, Clone)]
pub(crate) struct StartupIndexSetupState {
    pub(crate) entries: Vec<StartupIndexTreeEntry>,
    pub(crate) show_hidden_entries: bool,
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
    pub(crate) fn from_roots(roots: Vec<StartupIndexRootSeed>) -> Option<Self> {
        let entries = roots
            .into_iter()
            .enumerate()
            .map(|(id, root)| StartupIndexTreeEntry::from_root(id, root))
            .collect::<Vec<_>>();
        (!entries.is_empty()).then_some(Self {
            entries,
            show_hidden_entries: false,
            directory_load_generation: 0,
        })
    }

    pub(crate) fn merge_roots(&mut self, roots: Vec<StartupIndexRootSeed>) {
        let mut existing_paths = self
            .entries
            .iter()
            .filter(|entry| entry.parent.is_none())
            .map(|entry| entry.path.clone())
            .collect::<HashSet<_>>();

        for root in roots {
            if !existing_paths.insert(root.path.clone()) {
                continue;
            }
            let id = self.entries.len();
            self.entries
                .push(StartupIndexTreeEntry::from_root(id, root));
        }
    }

    pub(crate) fn expand_roots_waiting_for_children(&mut self) -> Vec<PathBuf> {
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

    pub(crate) fn selected_index_requests(&self) -> Vec<StartupIndexBuildRequest> {
        self.entries
            .iter()
            .filter(|entry| entry.parent.is_none())
            .filter_map(|root| self.selected_index_request_for_root(root.id))
            .collect()
    }

    fn selected_index_request_for_root(&self, root_id: usize) -> Option<StartupIndexBuildRequest> {
        let root = self.entries.get(root_id)?;
        let subtree_end = self.subtree_end(root_id);
        let selected_paths = (root_id..subtree_end)
            .filter(|entry_id| self.entry_is_selected_without_selected_ancestor(root_id, *entry_id))
            .map(|entry_id| self.entries[entry_id].path.clone())
            .collect::<Vec<_>>();

        (!selected_paths.is_empty()).then_some(StartupIndexBuildRequest {
            root: root.path.clone(),
            selected_paths,
        })
    }

    fn entry_is_selected_without_selected_ancestor(&self, root_id: usize, entry_id: usize) -> bool {
        let Some(entry) = self.entries.get(entry_id) else {
            return false;
        };
        if entry.selection != StartupIndexEntrySelection::Selected {
            return false;
        }

        let mut parent = entry.parent;
        while let Some(parent_id) = parent {
            if parent_id < root_id {
                return true;
            }
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
}

fn startup_index_directory_children(kind: FileKind) -> Option<StartupIndexDirectoryChildren> {
    (kind == FileKind::Directory).then_some(StartupIndexDirectoryChildren::Pending)
}
