use file_core::{DirectoryEntry, DirectoryScan, FileKind};
use iced::Task;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

use super::FileBrowser;
use crate::commands::{
    delayed_thumbnail_refresh_command, load_directory_command, load_expanded_directory_command,
};
use crate::model::{
    trash_location_path, BrowserPaneId, BrowserViewMode, DirectoryExpansionLoadContext,
    DirectoryLoadRequest, DirectoryLoadingPlaceholderEntry, ExpandedDirectory,
    ExpandedDirectoryLoadRequest, ExpandedDirectoryStatus, Message, NavigationMode,
};
use crate::startup_trace;
impl FileBrowser {
    pub(super) fn next_directory_load_request(&mut self, path: PathBuf) -> DirectoryLoadRequest {
        if let Some(cancel) = self.directory_load_cancel.take() {
            cancel.cancel();
        }
        self.directory_load_generation = self.directory_load_generation.wrapping_add(1);
        self.directory_load_cancel = Some(CancellationToken::new());
        DirectoryLoadRequest {
            pane_id: self.active_pane_id(),
            path,
            generation: self.directory_load_generation,
        }
    }

    fn cancel_active_expanded_directory_loads(&mut self) {
        for expanded in self.expanded_directories.values_mut() {
            Self::cancel_expanded_directory_load(expanded);
        }
    }

    fn cancel_active_directory_load(&mut self) {
        if let Some(cancel) = self.directory_load_cancel.take() {
            cancel.cancel();
        }
        self.directory_load_generation = self.directory_load_generation.wrapping_add(1);
    }

    pub(super) fn directory_load_cancellation(
        &self,
        request: &DirectoryLoadRequest,
    ) -> CancellationToken {
        if request.pane_id == self.active_pane_id() {
            return self
                .directory_load_cancel
                .clone()
                .unwrap_or_else(CancellationToken::new);
        }

        self.pane_by_id(request.pane_id)
            .and_then(|pane| pane.directory_load_cancel.clone())
            .unwrap_or_else(CancellationToken::new)
    }

    pub(super) fn next_inactive_directory_load_request(
        pane: &mut crate::model::BrowserPane,
        path: PathBuf,
    ) -> (DirectoryLoadRequest, CancellationToken) {
        if let Some(cancel) = pane.directory_load_cancel.take() {
            cancel.cancel();
        }
        pane.directory_load_generation = pane.directory_load_generation.wrapping_add(1);
        let cancellation = CancellationToken::new();
        pane.directory_load_cancel = Some(cancellation.clone());
        (
            DirectoryLoadRequest {
                pane_id: pane.id,
                path,
                generation: pane.directory_load_generation,
            },
            cancellation,
        )
    }

    pub(super) fn next_expanded_directory_load_request(
        context: DirectoryExpansionLoadContext,
        path: PathBuf,
        expanded: &mut ExpandedDirectory,
    ) -> (ExpandedDirectoryLoadRequest, CancellationToken) {
        if let Some(cancel) = expanded.load_cancel.take() {
            cancel.cancel();
        }
        expanded.load_generation = expanded.load_generation.wrapping_add(1);
        let cancellation = CancellationToken::new();
        expanded.load_cancel = Some(cancellation.clone());
        (
            ExpandedDirectoryLoadRequest {
                context,
                path,
                generation: expanded.load_generation,
            },
            cancellation,
        )
    }

    pub(super) fn cancel_expanded_directory_load(expanded: &mut ExpandedDirectory) {
        if let Some(cancel) = expanded.load_cancel.take() {
            cancel.cancel();
        }
        expanded.load_generation = expanded.load_generation.wrapping_add(1);
    }

    pub(super) fn accept_directory_scan_batch(
        &mut self,
        request: DirectoryLoadRequest,
        batch: file_core::DirectoryScanBatch,
    ) -> Task<Message> {
        if request.pane_id != self.active_pane_id() {
            let options = self.options.clone();
            {
                let Some(pane) = self.pane_by_id_mut(request.pane_id) else {
                    return Task::none();
                };
                if !directory_load_request_matches_pane(
                    &request,
                    pane.current_dir.as_path(),
                    pane.directory_load_generation,
                ) {
                    return Task::none();
                }
                merge_directory_scan_batch(&mut pane.entries, batch, &options);
                pane.directory_loading_placeholder_entries.clear();
                pane.sync_active_tab_state();
            }
            self.resort_size_sorted_list_panes();
            return Task::none();
        }

        if !directory_load_request_matches_pane(
            &request,
            self.current_dir.as_path(),
            self.directory_load_generation,
        ) {
            return Task::none();
        }
        merge_directory_scan_batch(&mut self.entries, batch, &self.options);
        self.directory_loading_placeholder_entries.clear();
        self.sync_active_tab_state();
        self.resort_size_sorted_list_panes();
        self.schedule_thumbnail_refresh()
    }

    pub(super) fn accept_directory_scan(
        &mut self,
        request: DirectoryLoadRequest,
        scan: DirectoryScan,
    ) -> Task<Message> {
        if request.pane_id != self.active_pane_id() {
            let current_dir = {
                let Some(pane) = self.pane_by_id_mut(request.pane_id) else {
                    return Task::none();
                };
                if !directory_load_request_matches_pane(
                    &request,
                    pane.current_dir.as_path(),
                    pane.directory_load_generation,
                ) {
                    return Task::none();
                }

                pane.current_dir = scan.path;
                pane.entries = scan.entries;
                if pane.view_mode == crate::model::BrowserViewMode::Icons {
                    crate::model::retain_direct_entry_selection(
                        &pane.entries,
                        &mut pane.selected,
                        &mut pane.selected_paths,
                        &mut pane.selection_anchor,
                    );
                }
                pane.directory_loading_placeholder_entries.clear();
                pane.is_loading = false;
                pane.directory_load_cancel = None;
                pane.sync_active_tab_state();
                pane.current_dir.clone()
            };
            self.resort_size_sorted_list_panes();
            return Task::batch([
                delayed_thumbnail_refresh_command(request.pane_id, current_dir),
                self.schedule_visible_list_directory_summaries_for_pane(request.pane_id),
                self.reveal_address_bar_current_segment(request.pane_id),
            ]);
        }

        if !directory_load_request_matches_pane(
            &request,
            self.current_dir.as_path(),
            self.directory_load_generation,
        ) {
            return Task::none();
        }

        self.reconcile_icon_grid_root_after_scan(&scan.entries);
        self.current_dir = scan.path;
        self.entries = scan.entries;
        if self.view_mode == crate::model::BrowserViewMode::Icons {
            self.retain_icon_grid_visible_selection();
        }
        self.directory_loading_placeholder_entries.clear();
        self.is_loading = false;
        self.directory_load_cancel = None;
        self.clear_global_error();
        startup_trace::mark_once("initial_directory_ready");
        let command = self.focus_created_entry_for_rename();
        self.sync_active_tab_state();
        self.resort_size_sorted_list_panes();
        Task::batch([
            command,
            delayed_thumbnail_refresh_command(request.pane_id, self.current_dir.clone()),
            self.schedule_visible_list_directory_summaries_for_pane(request.pane_id),
            self.request_browser_session_save(),
            self.reveal_address_bar_current_segment(request.pane_id),
        ])
    }

    pub(super) fn navigate_to(&mut self, path: PathBuf, mode: NavigationMode) -> Task<Message> {
        let cancel_address_editing = self.cancel_address_editing();
        let placeholder_entries = self.capture_directory_loading_placeholder_entries();
        if mode == NavigationMode::RecordHistory && !self.is_trash_view && path != self.current_dir
        {
            self.back_stack.push(self.current_dir.clone());
            self.forward_stack.clear();
        }
        self.clear_icon_grid_expansion();
        self.current_dir = path.clone();
        self.is_trash_view = false;
        self.entries.clear();
        self.directory_loading_placeholder_entries = placeholder_entries;
        self.trash_entries.clear();
        self.deepest_open_column_directory = None;
        self.cancel_active_expanded_directory_loads();
        self.expanded_directories.clear();
        self.column_browser_viewport = Default::default();
        self.column_viewports.clear();
        self.clear_selection_context();
        self.is_loading = true;
        self.clear_global_error();
        self.sync_active_tab_state();
        let request = self.next_directory_load_request(path);
        let cancellation = self.directory_load_cancellation(&request);
        Task::batch([
            cancel_address_editing,
            load_directory_command(request, self.options.clone(), cancellation),
            self.request_browser_session_save(),
            self.reveal_address_bar_current_segment(self.active_pane_id()),
        ])
    }

    pub(super) fn open_trash_view(&mut self, mode: NavigationMode) -> Task<Message> {
        let cancel_address_editing = self.cancel_address_editing();
        let pane_id = self.active_pane_id();
        let cached_entries = self
            .trash_refresh
            .snapshot()
            .map(|snapshot| snapshot.entries.clone());
        let placeholder_entries = if cached_entries.is_some() {
            Vec::new()
        } else {
            self.capture_directory_loading_placeholder_entries()
        };
        if mode == NavigationMode::RecordHistory && !self.is_trash_view {
            self.back_stack.push(self.current_dir.clone());
            self.forward_stack.clear();
        }
        self.clear_icon_grid_expansion();
        self.current_dir = trash_location_path();
        self.is_trash_view = true;
        let has_cached_entries = cached_entries.is_some();
        self.trash_entries = cached_entries.unwrap_or_default();
        self.entries = self
            .trash_entries
            .iter()
            .map(|trash_entry| trash_entry.entry.clone())
            .collect();
        self.directory_loading_placeholder_entries = placeholder_entries;
        self.deepest_open_column_directory = None;
        self.cancel_active_expanded_directory_loads();
        self.expanded_directories.clear();
        self.column_browser_viewport = Default::default();
        self.column_viewports.clear();
        self.clear_selection_context();
        self.is_loading = !has_cached_entries;
        self.clear_global_error();
        self.cancel_active_directory_load();
        self.sync_active_tab_state();
        Task::batch([
            cancel_address_editing,
            self.request_trash_snapshot_refresh(),
            self.request_browser_session_save(),
            self.reveal_address_bar_current_segment(pane_id),
        ])
    }

    pub(super) fn reload_current(&mut self) -> Task<Message> {
        self.invalidate_list_directory_summaries_for_pane(self.active_pane_id());
        if self.is_trash_view {
            self.invalidate_trash_snapshot_refresh();
        }
        self.reload_current_preserving_list_directory_summaries()
    }

    pub(super) fn reload_current_preserving_list_directory_summaries(&mut self) -> Task<Message> {
        self.clear_transient_interaction_state();
        self.schedule_current_directory_reload_preserving_list_directory_summaries()
    }

    fn schedule_current_directory_reload_preserving_list_directory_summaries(
        &mut self,
    ) -> Task<Message> {
        if self.is_trash_view {
            self.clear_icon_grid_expansion();
            self.directory_loading_placeholder_entries.clear();
            self.is_loading = self.trash_refresh.snapshot().is_none();
            self.clear_global_error();
            self.cancel_active_directory_load();
            self.deepest_open_column_directory = None;
            self.cancel_active_expanded_directory_loads();
            self.expanded_directories.clear();
            return self.request_trash_snapshot_refresh();
        }

        self.directory_loading_placeholder_entries.clear();
        self.is_loading = true;
        self.clear_global_error();

        let mut commands = self.refresh_expanded_directory_commands();
        commands.extend(self.refresh_icon_grid_expansion_commands());
        let request = self.next_directory_load_request(self.current_dir.clone());
        let cancellation = self.directory_load_cancellation(&request);
        commands.push(load_directory_command(
            request,
            self.options.clone(),
            cancellation,
        ));
        Task::batch(commands)
    }

    pub(super) fn reload_visible_panes(&mut self) -> Task<Message> {
        self.invalidate_list_directory_summaries_for_visible_panes();
        self.reload_visible_panes_preserving_list_directory_summaries()
    }

    pub(super) fn reload_visible_panes_after_file_operation(&mut self) -> Task<Message> {
        self.invalidate_list_directory_summaries_for_visible_panes();
        self.reload_visible_panes_after_file_operation_preserving_list_directory_summaries()
    }

    pub(super) fn reload_visible_panes_after_file_operation_preserving_list_directory_summaries(
        &mut self,
    ) -> Task<Message> {
        let active_pane_id = self.active_pane_id();
        let mut commands = vec![
            self.refresh_trash_snapshot_for_visible_panes(),
            self.schedule_current_directory_reload_preserving_list_directory_summaries(),
        ];
        self.sync_active_pane_state();

        for pane_id in self.pane_layout.visible_pane_ids() {
            if pane_id != active_pane_id {
                commands
                    .push(self.reload_inactive_pane_preserving_list_directory_summaries(pane_id));
            }
        }

        Task::batch(commands)
    }

    pub(super) fn reload_visible_panes_preserving_list_directory_summaries(
        &mut self,
    ) -> Task<Message> {
        let active_pane_id = self.active_pane_id();
        let mut commands = vec![self.reload_current_preserving_list_directory_summaries()];
        self.sync_active_pane_state();

        for pane_id in self.pane_layout.visible_pane_ids() {
            if pane_id != active_pane_id {
                commands
                    .push(self.reload_inactive_pane_preserving_list_directory_summaries(pane_id));
            }
        }

        Task::batch(commands)
    }

    fn reload_inactive_pane_preserving_list_directory_summaries(
        &mut self,
        pane_id: BrowserPaneId,
    ) -> Task<Message> {
        let options = self.options.clone();
        let has_trash_snapshot = self.trash_refresh.snapshot().is_some();
        let Some(pane) = self.pane_by_id_mut(pane_id) else {
            return Task::none();
        };

        pane.directory_loading_placeholder_entries.clear();
        pane.is_loading = if pane.is_trash_view {
            !has_trash_snapshot
        } else {
            true
        };

        if pane.is_trash_view {
            pane.deepest_open_column_directory = None;
            for expanded in pane.expanded_directories.values_mut() {
                Self::cancel_expanded_directory_load(expanded);
            }
            pane.expanded_directories.clear();
            pane.sync_active_tab_state();
            return self.request_trash_snapshot_refresh();
        }

        let current_dir = pane.current_dir.clone();
        let expanded_paths = pane
            .expanded_directories
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for path in &expanded_paths {
            if let Some(expanded) = pane.expanded_directories.get_mut(path) {
                expanded.status = ExpandedDirectoryStatus::Loading;
                expanded.is_expanded = true;
                expanded.is_collapsing = false;
                expanded.animation_progress = 1.0;
            }
        }
        pane.sync_active_tab_state();

        let mut commands = Vec::new();
        for path in expanded_paths {
            let Some(expanded) = pane.expanded_directories.get_mut(&path) else {
                continue;
            };
            let (request, cancellation) = Self::next_expanded_directory_load_request(
                DirectoryExpansionLoadContext::BrowserTree { pane_id },
                path,
                expanded,
            );
            commands.push(load_expanded_directory_command(
                request,
                options.clone(),
                cancellation,
            ));
        }
        let (request, cancellation) = Self::next_inactive_directory_load_request(pane, current_dir);
        commands.push(load_directory_command(request, options, cancellation));
        Task::batch(commands)
    }

    pub(super) fn reload_observed_directory(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_trash_view {
            return Task::none();
        }

        if path == self.current_dir {
            self.invalidate_list_directory_summary_subtree_and_ancestor_chain(&path);
            return self.reload_current_preserving_list_directory_summaries();
        }

        let mut commands = Vec::with_capacity(2);
        if let Some(command) = self.reload_observed_icon_grid_directory(&path) {
            commands.push(command);
        }

        self.invalidate_list_directory_summary_subtree_and_ancestor_chain(&path);

        let pane_id = self.active_pane_id();
        if let Some(expanded) = self.expanded_directories.get_mut(&path) {
            expanded.status = ExpandedDirectoryStatus::Loading;
            expanded.is_expanded = true;
            expanded.is_collapsing = false;
            expanded.animation_progress = 1.0;
            let (request, cancellation) = Self::next_expanded_directory_load_request(
                DirectoryExpansionLoadContext::BrowserTree { pane_id },
                path,
                expanded,
            );
            commands.push(load_expanded_directory_command(
                request,
                self.options.clone(),
                cancellation,
            ));
        }
        Task::batch(commands)
    }

    pub(super) fn navigate_up(&mut self) -> Task<Message> {
        if self.is_trash_view {
            return Task::none();
        }

        if let Some(parent) = self.current_dir.parent() {
            self.navigate_to(parent.to_path_buf(), NavigationMode::RecordHistory)
        } else {
            Task::none()
        }
    }

    pub(super) fn navigate_back(&mut self) -> Task<Message> {
        if self.is_trash_view {
            if let Some(path) = self.back_stack.pop() {
                return self.navigate_to(path, NavigationMode::KeepHistory);
            }
            return Task::none();
        }

        if let Some(path) = self.back_stack.pop() {
            self.forward_stack.push(self.current_dir.clone());
            self.navigate_to(path, NavigationMode::KeepHistory)
        } else {
            Task::none()
        }
    }

    pub(super) fn navigate_forward(&mut self) -> Task<Message> {
        if let Some(path) = self.forward_stack.pop() {
            self.back_stack.push(self.current_dir.clone());
            self.navigate_to(path, NavigationMode::KeepHistory)
        } else {
            Task::none()
        }
    }

    pub(crate) fn entry_for_path(&self, path: &Path) -> Option<&DirectoryEntry> {
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .or_else(|| match self.view_mode {
                BrowserViewMode::Icons => self
                    .icon_grid_expansion
                    .as_ref()
                    .and_then(|state| state.entry(path)),
                BrowserViewMode::List | BrowserViewMode::Columns => self
                    .expanded_directories
                    .values()
                    .flat_map(|expanded| expanded.entries.iter())
                    .find(|entry| entry.path == path),
            })
    }

    pub(super) fn entry_kind_recursive(&self, path: &Path) -> Option<FileKind> {
        self.entry_for_path(path).map(|entry| entry.kind)
    }

    fn capture_directory_loading_placeholder_entries(
        &self,
    ) -> Vec<DirectoryLoadingPlaceholderEntry> {
        if self.view_mode != BrowserViewMode::List {
            return Vec::new();
        }
        crate::visible_entries::visible_entries(&self.entries, &self.expanded_directories)
            .into_iter()
            .map(|visible_entry| DirectoryLoadingPlaceholderEntry {
                entry: visible_entry.entry.clone(),
                depth: visible_entry.depth,
                animation_progress: visible_entry.animation_progress,
            })
            .collect()
    }

    fn clear_selection_context(&mut self) {
        self.selected = None;
        self.selected_paths.clear();
        self.selection_anchor = None;
        self.drag_selection_anchor = None;
        self.column_resize_drag = None;
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.hovered_entry = None;
        self.hovered_sidebar = None;
        self.cursor_paste_directory = None;
        self.last_activation_click = None;
        self.column_return_targets.clear();
        self.pending_keyboard_column_focus = None;
        self.clear_preview();
        self.context_menu = None;
        self.renaming = None;
        self.selection_marquee = None;
    }

    pub(super) fn clear_transient_interaction_state(&mut self) {
        self.drag_selection_anchor = None;
        self.column_resize_drag = None;
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.hovered_entry = None;
        self.hovered_sidebar = None;
        self.cursor_paste_directory = None;
        self.last_activation_click = None;
        self.pending_keyboard_column_focus = None;
        self.clear_preview();
        self.context_menu = None;
        self.renaming = None;
        self.selection_marquee = None;
    }

    fn refresh_expanded_directory_commands(&mut self) -> Vec<Task<Message>> {
        let paths = self
            .expanded_directories
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for path in &paths {
            if let Some(expanded) = self.expanded_directories.get_mut(path) {
                expanded.status = ExpandedDirectoryStatus::Loading;
                expanded.is_expanded = true;
                expanded.is_collapsing = false;
                expanded.animation_progress = 1.0;
            }
        }

        let pane_id = self.active_pane_id();
        paths
            .into_iter()
            .filter_map(|path| {
                let expanded = self.expanded_directories.get_mut(&path)?;
                let (request, cancellation) = Self::next_expanded_directory_load_request(
                    DirectoryExpansionLoadContext::BrowserTree { pane_id },
                    path,
                    expanded,
                );
                Some(load_expanded_directory_command(
                    request,
                    self.options.clone(),
                    cancellation,
                ))
            })
            .collect()
    }
}

fn directory_load_request_matches_pane(
    request: &DirectoryLoadRequest,
    current_dir: &Path,
    generation: u64,
) -> bool {
    request.generation == generation && request.path.as_path() == current_dir
}

pub(super) fn merge_directory_scan_batch(
    entries: &mut Vec<DirectoryEntry>,
    batch: file_core::DirectoryScanBatch,
    options: &file_core::ScanOptions,
) {
    for entry in batch.entries {
        if let Some(existing) = entries
            .iter_mut()
            .find(|existing| existing.path == entry.path)
        {
            *existing = entry;
        } else {
            entries.push(entry);
        }
    }
    file_core::sort_entries(entries, options);
}
