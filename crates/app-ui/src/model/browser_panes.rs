use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use file_core::{DirectoryEntry, TrashEntry};
use tokio_util::sync::CancellationToken;

use crate::operation_history::{
    completed_migrations_cross_directory_tree_boundary, completed_migrations_touch_directory_tree,
    path_after_completed_migrations, CompletedPathMigration,
};
use crate::thumbnail_cache::ColumnViewport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BrowserPaneId(pub(crate) u64);

impl BrowserPaneId {
    pub(crate) const PRIMARY: Self = Self(0);

    pub(crate) fn key(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectoryLoadRequest {
    pub(crate) pane_id: BrowserPaneId,
    pub(crate) path: PathBuf,
    pub(crate) generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpandedDirectoryLoadRequest {
    pub(crate) pane_id: BrowserPaneId,
    pub(crate) path: PathBuf,
    pub(crate) generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitRegion {
    Left,
    Right,
    Top,
    Bottom,
}

impl SplitRegion {
    pub(crate) fn axis(self) -> SplitAxis {
        match self {
            Self::Left | Self::Right => SplitAxis::Horizontal,
            Self::Top | Self::Bottom => SplitAxis::Vertical,
        }
    }

    pub(crate) fn places_dragged_first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserPaneLayout {
    Single {
        active: BrowserPaneId,
    },
    Split {
        axis: SplitAxis,
        first: BrowserPaneId,
        second: BrowserPaneId,
        active: BrowserPaneId,
    },
}

impl BrowserPaneLayout {
    pub(crate) fn active(self) -> BrowserPaneId {
        match self {
            Self::Single { active } | Self::Split { active, .. } => active,
        }
    }

    pub(crate) fn visible_pane_ids(self) -> Vec<BrowserPaneId> {
        match self {
            Self::Single { active } => vec![active],
            Self::Split { first, second, .. } => vec![first, second],
        }
    }

    pub(crate) fn with_active(self, next_active: BrowserPaneId) -> Self {
        match self {
            Self::Single { .. } => Self::Single {
                active: next_active,
            },
            Self::Split {
                axis,
                first,
                second,
                ..
            } => Self::Split {
                axis,
                first,
                second,
                active: next_active,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserViewMode {
    Columns,
    List,
    Icons,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ColumnBrowserViewport {
    pub(crate) offset_x: f32,
    pub(crate) width: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct IconGridViewport {
    pub(crate) offset_y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct DirectoryLoadingPlaceholderEntry {
    pub(crate) entry: DirectoryEntry,
    pub(crate) depth: usize,
    pub(crate) animation_progress: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct BrowserPane {
    pub(crate) id: BrowserPaneId,
    pub(crate) current_dir: PathBuf,
    pub(crate) is_trash_view: bool,
    pub(crate) entries: Vec<DirectoryEntry>,
    pub(crate) directory_loading_placeholder_entries: Vec<DirectoryLoadingPlaceholderEntry>,
    pub(crate) trash_entries: Vec<TrashEntry>,
    pub(crate) selected: Option<PathBuf>,
    pub(crate) selected_paths: HashSet<PathBuf>,
    pub(crate) selection_anchor: Option<PathBuf>,
    pub(crate) deepest_open_column_directory: Option<PathBuf>,
    pub(crate) expanded_directories: HashMap<PathBuf, ExpandedDirectory>,
    pub(crate) view_mode: BrowserViewMode,
    pub(crate) column_browser_viewport: ColumnBrowserViewport,
    pub(crate) column_viewports: HashMap<PathBuf, ColumnViewport>,
    pub(crate) tabs: Vec<BrowserTab>,
    pub(crate) active_tab_id: usize,
    pub(crate) directory_load_generation: u64,
    pub(crate) directory_load_cancel: Option<CancellationToken>,
    pub(crate) back_stack: Vec<PathBuf>,
    pub(crate) forward_stack: Vec<PathBuf>,
    pub(crate) is_loading: bool,
}

impl BrowserPane {
    pub(crate) fn sync_active_tab_state(&mut self) {
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == self.active_tab_id)
        else {
            return;
        };

        tab.directory = self.current_dir.clone();
        tab.is_trash_view = self.is_trash_view;
        tab.entries = self.entries.clone();
        tab.trash_entries = self.trash_entries.clone();
        tab.selected = self.selected.clone();
        tab.selected_paths = self.selected_paths.clone();
        tab.selection_anchor = self.selection_anchor.clone();
        tab.deepest_open_column_directory = self.deepest_open_column_directory.clone();
        tab.expanded_directories = self.expanded_directories.clone();
        tab.view_mode = self.view_mode;
        tab.back_stack = self.back_stack.clone();
        tab.forward_stack = self.forward_stack.clone();
    }

    pub(crate) fn migrate_completed_paths(&mut self, migrations: &[CompletedPathMigration]) {
        let original_directory = self.current_dir.clone();
        let migrated_directory = path_after_completed_migrations(&original_directory, migrations);
        let directory_was_migrated = migrated_directory != original_directory;
        let directory_tree_was_touched =
            completed_migrations_touch_directory_tree(&original_directory, migrations);
        let migration_crossed_directory_tree =
            completed_migrations_cross_directory_tree_boundary(&original_directory, migrations);

        self.current_dir = migrated_directory;
        if directory_was_migrated || directory_tree_was_touched {
            self.cancel_current_directory_load();
        }
        if !directory_was_migrated && migration_crossed_directory_tree {
            self.invalidate_cached_directory_tree();
        } else {
            self.migrate_cached_directory_tree_paths(migrations);
        }
        migrate_path_list(&mut self.back_stack, migrations);
        migrate_path_list(&mut self.forward_stack, migrations);
        for tab in &mut self.tabs {
            tab.migrate_completed_paths(migrations);
        }
    }

    fn cancel_current_directory_load(&mut self) {
        if let Some(cancellation) = self.directory_load_cancel.take() {
            cancellation.cancel();
        }
        self.directory_load_generation = self.directory_load_generation.saturating_add(1);
        self.is_loading = false;
    }

    fn migrate_cached_directory_tree_paths(&mut self, migrations: &[CompletedPathMigration]) {
        migrate_directory_entries(&mut self.entries, migrations);
        for placeholder in &mut self.directory_loading_placeholder_entries {
            migrate_directory_entry(&mut placeholder.entry, migrations);
        }
        migrate_optional_path(&mut self.selected, migrations);
        migrate_path_set(&mut self.selected_paths, migrations);
        migrate_optional_path(&mut self.selection_anchor, migrations);
        migrate_optional_path(&mut self.deepest_open_column_directory, migrations);
        migrate_expanded_directories(&mut self.expanded_directories, migrations);
        migrate_path_map_keys(&mut self.column_viewports, migrations);
    }

    fn invalidate_cached_directory_tree(&mut self) {
        self.entries.clear();
        self.directory_loading_placeholder_entries.clear();
        self.selected = None;
        self.selected_paths.clear();
        self.selection_anchor = None;
        self.deepest_open_column_directory = None;
        cancel_and_clear_expanded_directories(&mut self.expanded_directories);
        self.column_browser_viewport = ColumnBrowserViewport::default();
        self.column_viewports.clear();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BrowserTab {
    pub(crate) id: usize,
    pub(crate) directory: PathBuf,
    pub(crate) is_trash_view: bool,
    pub(crate) entries: Vec<DirectoryEntry>,
    pub(crate) trash_entries: Vec<TrashEntry>,
    pub(crate) selected: Option<PathBuf>,
    pub(crate) selected_paths: HashSet<PathBuf>,
    pub(crate) selection_anchor: Option<PathBuf>,
    pub(crate) deepest_open_column_directory: Option<PathBuf>,
    pub(crate) expanded_directories: HashMap<PathBuf, ExpandedDirectory>,
    pub(crate) view_mode: BrowserViewMode,
    pub(crate) back_stack: Vec<PathBuf>,
    pub(crate) forward_stack: Vec<PathBuf>,
}

impl BrowserTab {
    pub(crate) fn directory(id: usize, directory: PathBuf) -> Self {
        Self {
            id,
            directory,
            is_trash_view: false,
            entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            selection_anchor: None,
            deepest_open_column_directory: None,
            expanded_directories: HashMap::new(),
            view_mode: BrowserViewMode::Columns,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
        }
    }

    pub(crate) fn trash(id: usize) -> Self {
        Self {
            id,
            directory: super::trash_location_path(),
            is_trash_view: true,
            entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            selection_anchor: None,
            deepest_open_column_directory: None,
            expanded_directories: HashMap::new(),
            view_mode: BrowserViewMode::Columns,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
        }
    }

    fn migrate_completed_paths(&mut self, migrations: &[CompletedPathMigration]) {
        let original_directory = self.directory.clone();
        let migrated_directory = path_after_completed_migrations(&original_directory, migrations);
        let directory_was_migrated = migrated_directory != original_directory;
        let migration_crossed_directory_tree =
            completed_migrations_cross_directory_tree_boundary(&original_directory, migrations);

        self.directory = migrated_directory;
        if !directory_was_migrated && migration_crossed_directory_tree {
            self.invalidate_cached_directory_tree();
        } else {
            self.migrate_cached_directory_tree_paths(migrations);
        }
        migrate_path_list(&mut self.back_stack, migrations);
        migrate_path_list(&mut self.forward_stack, migrations);
    }

    fn migrate_cached_directory_tree_paths(&mut self, migrations: &[CompletedPathMigration]) {
        migrate_directory_entries(&mut self.entries, migrations);
        migrate_optional_path(&mut self.selected, migrations);
        migrate_path_set(&mut self.selected_paths, migrations);
        migrate_optional_path(&mut self.selection_anchor, migrations);
        migrate_optional_path(&mut self.deepest_open_column_directory, migrations);
        migrate_expanded_directories(&mut self.expanded_directories, migrations);
    }

    fn invalidate_cached_directory_tree(&mut self) {
        self.entries.clear();
        self.selected = None;
        self.selected_paths.clear();
        self.selection_anchor = None;
        self.deepest_open_column_directory = None;
        cancel_and_clear_expanded_directories(&mut self.expanded_directories);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ExpandedDirectory {
    pub(crate) entries: Vec<DirectoryEntry>,
    pub(crate) status: ExpandedDirectoryStatus,
    pub(crate) is_expanded: bool,
    pub(crate) is_collapsing: bool,
    pub(crate) animation_progress: f32,
    pub(crate) load_generation: u64,
    pub(crate) load_cancel: Option<CancellationToken>,
}

#[derive(Debug, Clone)]
pub(crate) enum ExpandedDirectoryStatus {
    Loading,
    Loaded,
    Error,
}

pub(crate) fn retain_direct_entry_selection(
    entries: &[DirectoryEntry],
    selected: &mut Option<PathBuf>,
    selected_paths: &mut HashSet<PathBuf>,
    selection_anchor: &mut Option<PathBuf>,
) {
    let direct_paths = entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<HashSet<_>>();
    selected_paths.retain(|path| direct_paths.contains(path));
    if !selected
        .as_ref()
        .is_some_and(|path| direct_paths.contains(path))
    {
        *selected = entries
            .iter()
            .rev()
            .find(|entry| selected_paths.contains(&entry.path))
            .map(|entry| entry.path.clone());
    }
    if selection_anchor
        .as_ref()
        .is_some_and(|path| !direct_paths.contains(path))
    {
        *selection_anchor = None;
    }
}

fn migrate_directory_entries(
    entries: &mut [DirectoryEntry],
    migrations: &[CompletedPathMigration],
) {
    for entry in entries {
        migrate_directory_entry(entry, migrations);
    }
}

fn migrate_directory_entry(entry: &mut DirectoryEntry, migrations: &[CompletedPathMigration]) {
    let migrated_path = path_after_completed_migrations(&entry.path, migrations);
    if migrated_path == entry.path {
        return;
    }
    entry.name = migrated_path
        .file_name()
        .unwrap_or_else(|| migrated_path.as_os_str())
        .to_os_string();
    entry.path = migrated_path;
}

fn migrate_optional_path(path: &mut Option<PathBuf>, migrations: &[CompletedPathMigration]) {
    if let Some(path) = path {
        *path = path_after_completed_migrations(path, migrations);
    }
}

fn migrate_path_list(paths: &mut [PathBuf], migrations: &[CompletedPathMigration]) {
    for path in paths {
        *path = path_after_completed_migrations(path, migrations);
    }
}

fn migrate_path_set(paths: &mut HashSet<PathBuf>, migrations: &[CompletedPathMigration]) {
    *paths = paths
        .drain()
        .map(|path| path_after_completed_migrations(&path, migrations))
        .collect();
}

fn migrate_path_map_keys<Value>(
    paths: &mut HashMap<PathBuf, Value>,
    migrations: &[CompletedPathMigration],
) {
    *paths = paths
        .drain()
        .map(|(path, value)| (path_after_completed_migrations(&path, migrations), value))
        .collect();
}

fn migrate_expanded_directories(
    directories: &mut HashMap<PathBuf, ExpandedDirectory>,
    migrations: &[CompletedPathMigration],
) {
    *directories = directories
        .drain()
        .filter_map(|(path, mut directory)| {
            if let Some(cancellation) = directory.load_cancel.take() {
                cancellation.cancel();
            }
            if matches!(directory.status, ExpandedDirectoryStatus::Loading) {
                return None;
            }
            migrate_directory_entries(&mut directory.entries, migrations);
            Some((
                path_after_completed_migrations(&path, migrations),
                directory,
            ))
        })
        .collect();
}

fn cancel_and_clear_expanded_directories(directories: &mut HashMap<PathBuf, ExpandedDirectory>) {
    for directory in directories.values_mut() {
        if let Some(cancellation) = directory.load_cancel.take() {
            cancellation.cancel();
        }
    }
    directories.clear();
}
