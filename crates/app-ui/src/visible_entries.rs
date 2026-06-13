use std::collections::HashMap;
use std::path::{Path, PathBuf};

use file_core::DirectoryEntry;

use crate::model::{ExpandedDirectory, ExpandedDirectoryStatus};

#[derive(Debug, Clone, Copy)]
pub(crate) struct VisibleEntry<'a> {
    pub(crate) entry: &'a DirectoryEntry,
    pub(crate) depth: usize,
    pub(crate) animation_progress: f32,
}

pub(crate) fn visible_entries<'a>(
    entries: &'a [DirectoryEntry],
    expanded_directories: &'a HashMap<PathBuf, ExpandedDirectory>,
) -> Vec<VisibleEntry<'a>> {
    let mut visible_entries = Vec::new();
    for entry in entries {
        push_visible_entry(entry, 0, expanded_directories, &mut visible_entries);
    }
    visible_entries
}

pub(crate) fn visible_entry_paths(
    entries: &[DirectoryEntry],
    expanded_directories: &HashMap<PathBuf, ExpandedDirectory>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in entries {
        push_selectable_entry_path(entry, expanded_directories, &mut paths);
    }
    paths
}

pub(crate) fn visible_child_paths(
    directory: &Path,
    current_dir: &Path,
    entries: &[DirectoryEntry],
    expanded_directories: &HashMap<PathBuf, ExpandedDirectory>,
) -> Vec<PathBuf> {
    if directory == current_dir {
        return entries.iter().map(|entry| entry.path.clone()).collect();
    }

    expanded_directories
        .get(directory)
        .filter(|expanded| expanded.is_expanded)
        .filter(|expanded| matches!(expanded.status, ExpandedDirectoryStatus::Loaded))
        .map(|expanded| {
            expanded
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn entry_is_visible(
    path: &Path,
    entries: &[DirectoryEntry],
    expanded_directories: &HashMap<PathBuf, ExpandedDirectory>,
) -> bool {
    visible_entries(entries, expanded_directories)
        .iter()
        .any(|visible_entry| visible_entry.entry.path == path)
}

fn push_visible_entry<'a>(
    entry: &'a DirectoryEntry,
    depth: usize,
    expanded_directories: &'a HashMap<PathBuf, ExpandedDirectory>,
    visible_entries: &mut Vec<VisibleEntry<'a>>,
) {
    push_visible_entry_with_progress(entry, depth, 1.0, expanded_directories, visible_entries);
}

fn push_visible_entry_with_progress<'a>(
    entry: &'a DirectoryEntry,
    depth: usize,
    animation_progress: f32,
    expanded_directories: &'a HashMap<PathBuf, ExpandedDirectory>,
    visible_entries: &mut Vec<VisibleEntry<'a>>,
) {
    visible_entries.push(VisibleEntry {
        entry,
        depth,
        animation_progress,
    });
    let Some(expanded) = expanded_directories
        .get(&entry.path)
        .filter(|expanded| expanded.is_expanded || expanded.is_collapsing)
    else {
        return;
    };
    if matches!(expanded.status, ExpandedDirectoryStatus::Loaded) {
        let child_progress = animation_progress * expanded.animation_progress.clamp(0.0, 1.0);
        for child in &expanded.entries {
            push_visible_entry_with_progress(
                child,
                depth + 1,
                child_progress,
                expanded_directories,
                visible_entries,
            );
        }
    }
}

fn push_selectable_entry_path(
    entry: &DirectoryEntry,
    expanded_directories: &HashMap<PathBuf, ExpandedDirectory>,
    paths: &mut Vec<PathBuf>,
) {
    paths.push(entry.path.clone());
    let Some(expanded) = expanded_directories
        .get(&entry.path)
        .filter(|expanded| expanded.is_expanded && !expanded.is_collapsing)
    else {
        return;
    };
    if matches!(expanded.status, ExpandedDirectoryStatus::Loaded) {
        for child in &expanded.entries {
            push_selectable_entry_path(child, expanded_directories, paths);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use file_core::{DirectoryEntry, EntryMetadata, FileKind};

    use super::*;

    fn test_entry(path: PathBuf, kind: FileKind) -> DirectoryEntry {
        DirectoryEntry::new(
            path,
            kind,
            EntryMetadata {
                len: 0,
                modified: None,
                readonly: false,
            },
            false,
            false,
            false,
        )
    }

    fn expanded(entries: Vec<DirectoryEntry>, is_expanded: bool) -> ExpandedDirectory {
        ExpandedDirectory {
            entries,
            status: ExpandedDirectoryStatus::Loaded,
            is_expanded,
            is_collapsing: false,
            animation_progress: 1.0,
        }
    }

    #[test]
    fn visible_entries_include_root_rows() {
        let root = PathBuf::from("/workspace");
        let first = test_entry(root.join("a.txt"), FileKind::File);
        let second = test_entry(root.join("b.txt"), FileKind::File);
        let entries = vec![first.clone(), second.clone()];

        let paths = visible_entry_paths(&entries, &HashMap::new());

        assert_eq!(paths, vec![first.path, second.path]);
    }

    #[test]
    fn visible_entries_include_expanded_children() {
        let root = PathBuf::from("/workspace");
        let directory = test_entry(root.join("project"), FileKind::Directory);
        let child = test_entry(directory.path.join("main.rs"), FileKind::File);
        let entries = vec![directory.clone()];
        let expanded_directories =
            HashMap::from([(directory.path.clone(), expanded(vec![child.clone()], true))]);

        let visible = visible_entries(&entries, &expanded_directories);

        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].entry.path, directory.path);
        assert_eq!(visible[0].depth, 0);
        assert_eq!(visible[1].entry.path, child.path);
        assert_eq!(visible[1].depth, 1);
    }

    #[test]
    fn visible_entries_include_nested_expanded_children() {
        let root = PathBuf::from("/workspace");
        let parent = test_entry(root.join("parent"), FileKind::Directory);
        let child = test_entry(parent.path.join("child"), FileKind::Directory);
        let grandchild = test_entry(child.path.join("note.txt"), FileKind::File);
        let entries = vec![parent.clone()];
        let expanded_directories = HashMap::from([
            (parent.path.clone(), expanded(vec![child.clone()], true)),
            (child.path.clone(), expanded(vec![grandchild.clone()], true)),
        ]);

        let visible = visible_entries(&entries, &expanded_directories);

        assert_eq!(visible.len(), 3);
        assert_eq!(visible[2].entry.path, grandchild.path);
        assert_eq!(visible[2].depth, 2);
    }

    #[test]
    fn collapsed_directory_hides_children() {
        let root = PathBuf::from("/workspace");
        let directory = test_entry(root.join("project"), FileKind::Directory);
        let child = test_entry(directory.path.join("main.rs"), FileKind::File);
        let entries = vec![directory.clone()];
        let expanded_directories =
            HashMap::from([(directory.path.clone(), expanded(vec![child], false))]);

        let paths = visible_entry_paths(&entries, &expanded_directories);

        assert_eq!(paths, vec![directory.path]);
    }

    #[test]
    fn collapsed_directory_hides_children_even_when_animation_progress_remains() {
        let root = PathBuf::from("/workspace");
        let directory = test_entry(root.join("project"), FileKind::Directory);
        let child = test_entry(directory.path.join("main.rs"), FileKind::File);
        let entries = vec![directory.clone()];
        let expanded_directories = HashMap::from([(
            directory.path.clone(),
            ExpandedDirectory {
                entries: vec![child],
                status: ExpandedDirectoryStatus::Loaded,
                is_expanded: false,
                is_collapsing: false,
                animation_progress: 1.0,
            },
        )]);

        let visible = visible_entries(&entries, &expanded_directories);

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].entry.path, directory.path);
        assert_eq!(visible[0].depth, 0);
    }

    #[test]
    fn loading_directory_does_not_show_stale_children() {
        let root = PathBuf::from("/workspace");
        let directory = test_entry(root.join("project"), FileKind::Directory);
        let child = test_entry(directory.path.join("main.rs"), FileKind::File);
        let entries = vec![directory.clone()];
        let expanded_directories = HashMap::from([(
            directory.path.clone(),
            ExpandedDirectory {
                entries: vec![child],
                status: ExpandedDirectoryStatus::Loading,
                is_expanded: true,
                is_collapsing: false,
                animation_progress: 1.0,
            },
        )]);

        let paths = visible_entry_paths(&entries, &expanded_directories);

        assert_eq!(paths, vec![directory.path]);
    }

    #[test]
    fn loading_directory_does_not_expose_stale_visible_children() {
        let root = PathBuf::from("/workspace");
        let directory = test_entry(root.join("project"), FileKind::Directory);
        let child = test_entry(directory.path.join("main.rs"), FileKind::File);
        let entries = vec![directory.clone()];
        let expanded_directories = HashMap::from([(
            directory.path.clone(),
            ExpandedDirectory {
                entries: vec![child],
                status: ExpandedDirectoryStatus::Loading,
                is_expanded: true,
                is_collapsing: false,
                animation_progress: 1.0,
            },
        )]);

        let paths = visible_child_paths(&directory.path, &root, &entries, &expanded_directories);

        assert!(paths.is_empty());
    }

    #[test]
    fn collapsing_directory_keeps_children_for_animation_but_not_selection_paths() {
        let root = PathBuf::from("/workspace");
        let directory = test_entry(root.join("project"), FileKind::Directory);
        let child = test_entry(directory.path.join("main.rs"), FileKind::File);
        let entries = vec![directory.clone()];
        let expanded_directories = HashMap::from([(
            directory.path.clone(),
            ExpandedDirectory {
                entries: vec![child.clone()],
                status: ExpandedDirectoryStatus::Loaded,
                is_expanded: true,
                is_collapsing: true,
                animation_progress: 0.5,
            },
        )]);

        let visible = visible_entries(&entries, &expanded_directories);
        let selectable_paths = visible_entry_paths(&entries, &expanded_directories);

        assert_eq!(visible.len(), 2);
        assert_eq!(visible[1].entry.path, child.path);
        assert_eq!(visible[1].animation_progress, 0.5);
        assert_eq!(selectable_paths, vec![directory.path]);
    }
}
