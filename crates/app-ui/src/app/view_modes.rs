use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use file_core::FileKind;
use iced::Task;

use super::FileBrowser;
use crate::commands::load_expanded_directory_command;
use crate::model::{
    BrowserPaneId, BrowserViewMode, DirectoryExpansionLoadContext, ExpandedDirectory,
    ExpandedDirectoryLoadRequest, ExpandedDirectoryStatus, ListExpansionFollowSessionId, Message,
};

const LIST_DIRECTORY_ANIMATION_STEP: f32 = 0.18;

#[derive(Debug, Clone)]
pub(super) struct ListExpansionFollowPlan {
    pane_id: BrowserPaneId,
    tab_id: usize,
    current_dir: PathBuf,
    session_id: ListExpansionFollowSessionId,
    remaining_directories: VecDeque<PathBuf>,
    waiting_for: PathBuf,
    target_selection: PathBuf,
}

struct DirectoryExpansionTransfer {
    chain: Vec<PathBuf>,
    target_selection: PathBuf,
}

impl FileBrowser {
    pub(super) fn select_browser_view_mode(
        &mut self,
        pane_id: BrowserPaneId,
        view_mode: BrowserViewMode,
    ) -> Task<Message> {
        self.activate_pane(pane_id);
        if self.view_mode == view_mode {
            return Task::none();
        }

        let previous_mode = self.view_mode;
        let list_to_icons = (previous_mode == BrowserViewMode::List
            && view_mode == BrowserViewMode::Icons)
            .then(|| self.list_expansion_transfer())
            .flatten();
        let icons_to_list = (previous_mode == BrowserViewMode::Icons
            && view_mode == BrowserViewMode::List)
            .then(|| self.icon_grid_expansion_transfer())
            .flatten();

        self.clear_icon_grid_expansion_for_context_change();
        if previous_mode == BrowserViewMode::Icons || view_mode == BrowserViewMode::Icons {
            self.retain_direct_entry_selection();
        }
        let mut transition_command = match (previous_mode, view_mode) {
            (BrowserViewMode::Columns, BrowserViewMode::List) => {
                self.sync_expanded_directories_to_open_columns();
                Task::none()
            }
            (BrowserViewMode::List, BrowserViewMode::Columns) => {
                self.sync_open_column_directory_to_list_selection()
            }
            (BrowserViewMode::Icons, BrowserViewMode::Columns)
                if self.selected_is_direct_entry() =>
            {
                self.sync_open_column_directory_to_list_selection()
            }
            _ => Task::none(),
        };
        self.view_mode = view_mode;
        if let Some(transfer) = list_to_icons {
            transition_command =
                self.start_icon_grid_expansion_follow(transfer.chain, transfer.target_selection);
        } else if let Some(transfer) = icons_to_list {
            transition_command =
                self.start_list_expansion_follow(transfer.chain, transfer.target_selection);
        } else if previous_mode == BrowserViewMode::Icons && view_mode == BrowserViewMode::List {
            self.clear_list_expansion_for_follow();
        }
        self.hovered_entry = None;
        self.clear_list_header_hover_in_pane(pane_id);
        self.cursor_paste_directory = None;
        self.selection_marquee = None;
        self.drag_selection_anchor = None;
        self.cancel_file_drag_interaction();
        self.pending_keyboard_column_focus = None;
        self.column_resize_drag = None;
        self.file_entry_bounds.clear();
        self.user_config.browser_view_mode = view_mode;
        self.sync_active_tab_state();
        let list_directory_summary_command = if view_mode == BrowserViewMode::List {
            self.schedule_visible_list_directory_summaries_for_pane(pane_id)
        } else {
            Task::none()
        };
        Task::batch([
            transition_command,
            list_directory_summary_command,
            self.persist_user_preferences_command(),
            self.schedule_thumbnail_refresh(),
            self.request_browser_session_save(),
        ])
    }

    pub(super) fn retain_direct_entry_selection(&mut self) {
        crate::model::retain_direct_entry_selection(
            &self.entries,
            &mut self.selected,
            &mut self.selected_paths,
            &mut self.selection_anchor,
        );
    }

    pub(super) fn cancel_expansion_follow_plans(&mut self) {
        self.cancel_list_expansion_follow();
        if let Some(state) = self.icon_grid_expansion.as_mut() {
            state.cancel_follow_plan();
        }
    }

    pub(super) fn cancel_list_expansion_follow(&mut self) {
        let Some(plan) = self.list_expansion_follow.take() else {
            return;
        };
        if let Some(mut expanded) = self.expanded_directories.remove(&plan.waiting_for) {
            Self::cancel_expanded_directory_load(&mut expanded);
        }
    }

    fn list_expansion_transfer(&self) -> Option<DirectoryExpansionTransfer> {
        let target_selection = self.selected.clone()?;
        if target_selection == self.current_dir || !target_selection.starts_with(&self.current_dir)
        {
            return None;
        }

        let selected_is_expanded_directory = self
            .expanded_directories
            .get(&target_selection)
            .is_some_and(list_directory_is_followable);
        let mut current = if selected_is_expanded_directory {
            target_selection.clone()
        } else {
            target_selection.parent()?.to_path_buf()
        };
        let mut chain = Vec::new();
        while current != self.current_dir {
            chain.push(current.clone());
            current = current.parent()?.to_path_buf();
        }
        chain.reverse();
        if chain.is_empty() {
            return None;
        }

        let mut parent = self.current_dir.as_path();
        for directory_path in &chain {
            let entries = if parent == self.current_dir {
                self.entries.as_slice()
            } else {
                self.expanded_directories
                    .get(parent)
                    .filter(|expanded| list_directory_is_followable(expanded))?
                    .entries
                    .as_slice()
            };
            if !entries.iter().any(|entry| {
                entry.path == *directory_path
                    && entry.kind == FileKind::Directory
                    && entry.path.parent() == Some(parent)
            }) || !self
                .expanded_directories
                .get(directory_path)
                .is_some_and(list_directory_is_followable)
            {
                return None;
            }
            parent = directory_path;
        }

        let target_parent = target_selection.parent()?;
        let target_entries = if target_parent == self.current_dir {
            self.entries.as_slice()
        } else {
            self.expanded_directories
                .get(target_parent)
                .filter(|expanded| list_directory_is_followable(expanded))?
                .entries
                .as_slice()
        };
        if !target_entries.iter().any(|entry| {
            entry.path == target_selection && entry.path.parent() == Some(target_parent)
        }) {
            return None;
        }

        Some(DirectoryExpansionTransfer {
            chain,
            target_selection,
        })
    }

    fn icon_grid_expansion_transfer(&self) -> Option<DirectoryExpansionTransfer> {
        let target_selection = self.selected.clone()?;
        let chain = self
            .icon_grid_expansion
            .as_ref()?
            .interactive_expansion_chain_for_selection(&target_selection)?;
        (!chain.is_empty()).then_some(DirectoryExpansionTransfer {
            chain,
            target_selection,
        })
    }

    fn start_list_expansion_follow(
        &mut self,
        mut chain: Vec<PathBuf>,
        target_selection: PathBuf,
    ) -> Task<Message> {
        let Some(root_path) = chain.first().cloned() else {
            return Task::none();
        };
        self.clear_list_expansion_for_follow();
        if !self.entries.iter().any(|entry| {
            entry.path == root_path
                && entry.kind == FileKind::Directory
                && entry.path.parent() == Some(self.current_dir.as_path())
        }) {
            return Task::none();
        }

        let mut expanded = loading_list_directory();
        let session_id = self.next_list_expansion_follow_session_id();
        let pane_id = self.active_pane_id();
        let tab_id = self.active_tab_id;
        let current_dir = self.current_dir.clone();
        let (request, cancellation) = Self::next_expanded_directory_load_request(
            DirectoryExpansionLoadContext::ListFollow {
                pane_id,
                tab_id,
                current_dir: current_dir.clone(),
                session_id,
            },
            root_path.clone(),
            &mut expanded,
        );
        self.expanded_directories
            .insert(root_path.clone(), expanded);
        chain.remove(0);
        self.list_expansion_follow = Some(ListExpansionFollowPlan {
            pane_id,
            tab_id,
            current_dir,
            session_id,
            remaining_directories: chain.into(),
            waiting_for: root_path,
            target_selection,
        });
        load_expanded_directory_command(request, self.options.clone(), cancellation)
    }

    fn clear_list_expansion_for_follow(&mut self) {
        self.list_expansion_follow = None;
        for expanded in self.expanded_directories.values_mut() {
            Self::cancel_expanded_directory_load(expanded);
        }
        self.expanded_directories.clear();
    }

    pub(super) fn expanded_directory_load_context(
        &self,
        pane_id: BrowserPaneId,
        path: &Path,
    ) -> DirectoryExpansionLoadContext {
        let Some(plan) = self.list_expansion_follow.as_ref() else {
            return DirectoryExpansionLoadContext::BrowserTree { pane_id };
        };
        if self.view_mode == BrowserViewMode::List
            && pane_id == plan.pane_id
            && self.active_pane_id() == plan.pane_id
            && self.active_tab_id == plan.tab_id
            && self.current_dir == plan.current_dir
            && path == plan.waiting_for
        {
            DirectoryExpansionLoadContext::ListFollow {
                pane_id: plan.pane_id,
                tab_id: plan.tab_id,
                current_dir: plan.current_dir.clone(),
                session_id: plan.session_id,
            }
        } else {
            DirectoryExpansionLoadContext::BrowserTree { pane_id }
        }
    }

    pub(super) fn list_expansion_follow_request_is_current(
        &self,
        request: &ExpandedDirectoryLoadRequest,
    ) -> bool {
        let Some(plan) = self.list_expansion_follow.as_ref() else {
            return false;
        };
        matches!(
            &request.context,
            DirectoryExpansionLoadContext::ListFollow {
                pane_id,
                tab_id,
                current_dir,
                session_id,
            } if *pane_id == plan.pane_id
                && *tab_id == plan.tab_id
                && current_dir == &plan.current_dir
                && *session_id == plan.session_id
        ) && self.view_mode == BrowserViewMode::List
            && self.active_pane_id() == plan.pane_id
            && self.active_tab_id == plan.tab_id
            && self.current_dir == plan.current_dir
            && request.path == plan.waiting_for
    }

    #[cfg(test)]
    pub(in crate::app) fn pending_list_expansion_follow_request(
        &self,
    ) -> Option<ExpandedDirectoryLoadRequest> {
        let plan = self.list_expansion_follow.as_ref()?;
        let expanded = self.expanded_directories.get(&plan.waiting_for)?;
        Some(ExpandedDirectoryLoadRequest {
            context: DirectoryExpansionLoadContext::ListFollow {
                pane_id: plan.pane_id,
                tab_id: plan.tab_id,
                current_dir: plan.current_dir.clone(),
                session_id: plan.session_id,
            },
            path: plan.waiting_for.clone(),
            generation: expanded.load_generation,
        })
    }

    pub(super) fn advance_list_expansion_follow(
        &mut self,
        loaded_successfully: bool,
    ) -> Task<Message> {
        let Some(plan) = self.list_expansion_follow.as_ref() else {
            return Task::none();
        };
        if !loaded_successfully {
            self.list_expansion_follow = None;
            return Task::none();
        }

        if let Some(next_path) = plan.remaining_directories.front().cloned() {
            let waiting_for = plan.waiting_for.clone();
            let next_is_direct_directory = self
                .expanded_directories
                .get(&waiting_for)
                .filter(|expanded| list_directory_is_followable(expanded))
                .is_some_and(|expanded| {
                    expanded.entries.iter().any(|entry| {
                        entry.path == next_path
                            && entry.kind == FileKind::Directory
                            && entry.path.parent() == Some(waiting_for.as_path())
                    })
                });
            if !next_is_direct_directory {
                self.list_expansion_follow = None;
                return Task::none();
            }

            let context = DirectoryExpansionLoadContext::ListFollow {
                pane_id: plan.pane_id,
                tab_id: plan.tab_id,
                current_dir: plan.current_dir.clone(),
                session_id: plan.session_id,
            };
            let mut expanded = loading_list_directory();
            let (next_request, cancellation) = Self::next_expanded_directory_load_request(
                context,
                next_path.clone(),
                &mut expanded,
            );
            self.expanded_directories
                .insert(next_path.clone(), expanded);
            let plan = self
                .list_expansion_follow
                .as_mut()
                .expect("list follow plan remains active");
            plan.remaining_directories.pop_front();
            plan.waiting_for = next_path;
            return load_expanded_directory_command(
                next_request,
                self.options.clone(),
                cancellation,
            );
        }

        let target_selection = plan.target_selection.clone();
        self.list_expansion_follow = None;
        if crate::visible_entries::entry_is_visible(
            &target_selection,
            &self.entries,
            &self.expanded_directories,
        ) {
            self.select_path(target_selection);
        }
        Task::none()
    }

    fn next_list_expansion_follow_session_id(&mut self) -> ListExpansionFollowSessionId {
        let session_id =
            ListExpansionFollowSessionId::new(self.next_list_expansion_follow_session_id);
        self.next_list_expansion_follow_session_id =
            self.next_list_expansion_follow_session_id.wrapping_add(1);
        session_id
    }

    fn selected_is_direct_entry(&self) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|selected| self.entries.iter().any(|entry| entry.path == *selected))
    }

    pub(super) fn list_directory_animation_is_active(&self) -> bool {
        self.expanded_directories
            .values()
            .any(expanded_directory_is_animating)
            || self.panes.iter().any(|pane| {
                pane.expanded_directories
                    .values()
                    .any(expanded_directory_is_animating)
            })
    }

    pub(super) fn advance_list_directory_animations(&mut self) -> Task<Message> {
        let active_changed = advance_expanded_directories(&mut self.expanded_directories);
        for pane in &mut self.panes {
            let pane_changed = advance_expanded_directories(&mut pane.expanded_directories);
            if pane_changed {
                pane.sync_active_tab_state();
            }
        }
        if active_changed {
            self.sync_active_tab_state();
        }
        Task::none()
    }

    pub(super) fn toggle_list_directory(
        &mut self,
        pane_id: BrowserPaneId,
        path: PathBuf,
    ) -> Task<Message> {
        self.cancel_expansion_follow_plans();
        self.activate_pane(pane_id);
        self.toggle_list_directory_for_path(path)
    }

    pub(super) fn expand_selected_list_directory(&mut self) -> Task<Message> {
        self.cancel_expansion_follow_plans();
        let Some(selected) = self.selected.clone() else {
            return Task::none();
        };
        if self.entry_kind(&selected) != Some(FileKind::Directory) {
            return Task::none();
        }

        if self
            .expanded_directories
            .get(&selected)
            .is_some_and(|expanded| expanded.is_expanded && !expanded.is_collapsing)
        {
            if let Some(child) = self.first_visible_child_path(&selected) {
                self.select_path_from_keyboard(child);
            }
            return Task::none();
        }

        self.open_list_directory(selected)
    }

    pub(super) fn collapse_selected_list_directory_or_select_parent(&mut self) -> Task<Message> {
        self.cancel_expansion_follow_plans();
        let Some(selected) = self.selected.clone() else {
            return Task::none();
        };

        if self
            .expanded_directories
            .get(&selected)
            .is_some_and(|expanded| expanded.is_expanded && !expanded.is_collapsing)
        {
            return self.collapse_list_directory(selected);
        }

        let Some(parent) = selected.parent().map(Path::to_path_buf) else {
            return Task::none();
        };
        if parent == self.current_dir {
            return Task::none();
        }
        if !crate::visible_entries::entry_is_visible(
            &parent,
            &self.entries,
            &self.expanded_directories,
        ) {
            return Task::none();
        }

        self.select_path_from_keyboard(parent);
        self.sync_active_tab_state();
        self.schedule_thumbnail_refresh()
    }

    fn toggle_list_directory_for_path(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_trash_view || self.entry_kind(&path) != Some(FileKind::Directory) {
            return Task::none();
        }

        if self
            .expanded_directories
            .get(&path)
            .is_some_and(|expanded| expanded.is_expanded && !expanded.is_collapsing)
        {
            return self.collapse_list_directory(path);
        }

        self.open_list_directory(path)
    }

    fn collapse_list_directory(&mut self, path: PathBuf) -> Task<Message> {
        if let Some(expanded) = self.expanded_directories.get_mut(&path) {
            Self::cancel_expanded_directory_load(expanded);
            expanded.is_collapsing = true;
            expanded.animation_progress = expanded.animation_progress.clamp(0.0, 1.0);
        }
        self.sync_active_tab_state();
        Task::batch([
            self.schedule_thumbnail_refresh(),
            self.request_browser_session_save(),
        ])
    }

    fn open_list_directory(&mut self, path: PathBuf) -> Task<Message> {
        if let Some(expanded) = self.expanded_directories.get_mut(&path) {
            expanded.is_expanded = true;
            expanded.is_collapsing = false;
            expanded.animation_progress = expanded.animation_progress.clamp(0.0, 1.0);
            self.sync_active_tab_state();
            return Task::batch([
                self.schedule_thumbnail_refresh(),
                self.request_browser_session_save(),
            ]);
        }

        let mut expanded = ExpandedDirectory {
            entries: Vec::new(),
            directory_discovery: None,
            status: ExpandedDirectoryStatus::Loading,
            is_expanded: true,
            is_collapsing: false,
            animation_progress: 0.0,
            load_generation: 0,
            load_context: None,
            load_cancel: None,
            directory_order_phase: crate::model::DirectoryOrderPhase::Ready {
                field: file_core::SortField::Name,
                direction: file_core::SortDirection::Ascending,
            },
        };
        let (request, cancellation) = Self::next_expanded_directory_load_request(
            DirectoryExpansionLoadContext::BrowserTree {
                pane_id: self.active_pane_id(),
            },
            path.clone(),
            &mut expanded,
        );
        self.expanded_directories.insert(path, expanded);
        self.sync_active_tab_state();
        Task::batch([
            load_expanded_directory_command(request, self.options.clone(), cancellation),
            self.schedule_thumbnail_refresh(),
            self.request_browser_session_save(),
        ])
    }

    fn first_visible_child_path(&self, directory: &Path) -> Option<PathBuf> {
        crate::visible_entries::visible_child_paths(
            directory,
            &self.current_dir,
            &self.entries,
            &self.expanded_directories,
        )
        .into_iter()
        .next()
    }

    fn sync_expanded_directories_to_open_columns(&mut self) {
        let retained_paths = crate::three_column_view::column_directories(self)
            .into_iter()
            .filter(|path| path != &self.current_dir)
            .collect::<HashSet<_>>();
        self.expanded_directories.retain(|path, expanded| {
            if retained_paths.contains(path) {
                expanded.is_expanded = true;
                expanded.is_collapsing = false;
                expanded.animation_progress = 1.0;
                return true;
            }
            Self::cancel_expanded_directory_load(expanded);
            false
        });
    }

    fn sync_open_column_directory_to_list_selection(&mut self) -> Task<Message> {
        let selected = self.selected.clone();
        let selected_kind = selected.as_deref().and_then(|path| self.entry_kind(path));
        match (selected, selected_kind) {
            (Some(path), Some(FileKind::Directory)) => {
                let command = self.open_column_for_directory(path);
                self.sync_expanded_directories_to_open_columns();
                command
            }
            (Some(path), Some(_)) => {
                self.set_deepest_open_column_directory(path.parent().map(Path::to_path_buf));
                self.sync_expanded_directories_to_open_columns();
                Task::none()
            }
            _ => {
                self.set_deepest_open_column_directory(None);
                self.sync_expanded_directories_to_open_columns();
                Task::none()
            }
        }
    }
}

fn list_directory_is_followable(expanded: &ExpandedDirectory) -> bool {
    expanded.is_expanded
        && !expanded.is_collapsing
        && matches!(expanded.status, ExpandedDirectoryStatus::Loaded)
}

fn loading_list_directory() -> ExpandedDirectory {
    ExpandedDirectory {
        entries: Vec::new(),
        directory_discovery: None,
        status: ExpandedDirectoryStatus::Loading,
        is_expanded: true,
        is_collapsing: false,
        animation_progress: 0.0,
        load_generation: 0,
        load_context: None,
        load_cancel: None,
        directory_order_phase: crate::model::DirectoryOrderPhase::Ready {
            field: file_core::SortField::Name,
            direction: file_core::SortDirection::Ascending,
        },
    }
}

fn expanded_directory_is_animating(expanded: &ExpandedDirectory) -> bool {
    (expanded.is_expanded && !expanded.is_collapsing && expanded.animation_progress < 1.0)
        || expanded.is_collapsing
}

fn advance_expanded_directories(
    expanded_directories: &mut std::collections::HashMap<PathBuf, ExpandedDirectory>,
) -> bool {
    let mut changed = false;
    for expanded in expanded_directories.values_mut() {
        if expanded.is_collapsing {
            let next_progress =
                (expanded.animation_progress - LIST_DIRECTORY_ANIMATION_STEP).max(0.0);
            if (next_progress - expanded.animation_progress).abs() > f32::EPSILON {
                expanded.animation_progress = next_progress;
                changed = true;
            }
            if expanded.animation_progress <= f32::EPSILON {
                expanded.is_expanded = false;
                expanded.is_collapsing = false;
                changed = true;
            }
        } else if expanded.is_expanded && expanded.animation_progress < 1.0 {
            let next_progress =
                (expanded.animation_progress + LIST_DIRECTORY_ANIMATION_STEP).min(1.0);
            if (next_progress - expanded.animation_progress).abs() > f32::EPSILON {
                expanded.animation_progress = next_progress;
                changed = true;
            }
        }
    }
    changed
}
