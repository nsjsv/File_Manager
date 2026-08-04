use file_core::DirectoryScan;
use iced::Task;

use super::FileBrowser;
use crate::model::{
    DirectoryExpansionLoadContext, DirectoryLoadFailure, ExpandedDirectory,
    ExpandedDirectoryLoadRequest, ExpandedDirectoryStatus, Message,
};

impl FileBrowser {
    pub(super) fn accept_expanded_directory_batch(
        &mut self,
        request: ExpandedDirectoryLoadRequest,
        batch: file_core::DirectoryScanBatch,
    ) -> Task<Message> {
        if matches!(
            &request.context,
            DirectoryExpansionLoadContext::IconGrid { .. }
        ) {
            return self.accept_icon_grid_directory_batch(request, batch);
        }
        let Some(load_owner) = self.browser_tree_load_owner(&request) else {
            return Task::none();
        };
        let pane_id = load_owner.pane_id();
        if pane_id != self.active_pane_id() {
            let options = self.options.clone();
            {
                let Some(pane) = self.pane_by_id_mut(pane_id) else {
                    return Task::none();
                };
                let Some(expanded) = pane.expanded_directories.get_mut(&request.path) else {
                    return Task::none();
                };
                if !expanded_load_request_matches_directory(&request, expanded) {
                    return Task::none();
                }
                super::navigation::merge_directory_scan_batch(
                    &mut expanded.entries,
                    batch,
                    &options,
                );
                pane.sync_active_tab_state();
            }
            self.resort_size_sorted_list_panes();
            return Task::none();
        }

        let Some(expanded) = self.expanded_directories.get_mut(&request.path) else {
            return Task::none();
        };
        if !expanded_load_request_matches_directory(&request, expanded) {
            return Task::none();
        }
        super::navigation::merge_directory_scan_batch(&mut expanded.entries, batch, &self.options);
        self.sync_active_tab_state();
        self.resort_size_sorted_list_panes();
        Task::batch([
            self.schedule_thumbnail_refresh(),
            self.remeasure_active_file_drop_layout(),
        ])
    }

    pub(super) fn accept_expanded_directory(
        &mut self,
        request: ExpandedDirectoryLoadRequest,
        scan: Result<DirectoryScan, DirectoryLoadFailure>,
    ) -> Task<Message> {
        if matches!(
            &request.context,
            DirectoryExpansionLoadContext::IconGrid { .. }
        ) {
            return self.accept_icon_grid_directory(request, scan);
        }
        let Some(load_owner) = self.browser_tree_load_owner(&request) else {
            return Task::none();
        };
        let pane_id = load_owner.pane_id();
        if pane_id != self.active_pane_id() {
            let (pending_failure, loaded_child_count) = {
                let Some(pane) = self.pane_by_id_mut(pane_id) else {
                    return Task::none();
                };
                let Some(expanded) = pane.expanded_directories.get_mut(&request.path) else {
                    return Task::none();
                };
                if !expanded_load_request_matches_directory(&request, expanded) {
                    return Task::none();
                }

                expanded.load_context = None;
                expanded.load_cancel = None;
                let mut pending_failure = None;
                let mut loaded_child_count = None;
                match scan {
                    Ok(scan) => {
                        expanded.entries = scan.entries;
                        expanded.status = ExpandedDirectoryStatus::Loaded;
                        loaded_child_count = Some(expanded.entries.len());
                    }
                    Err(failure) => {
                        expanded.entries.clear();
                        expanded.status = ExpandedDirectoryStatus::Error;
                        pending_failure = Some(failure);
                    }
                }
                pane.sync_active_tab_state();
                (pending_failure, loaded_child_count)
            };
            self.resort_size_sorted_list_panes();
            if let Some(loaded_child_count) = loaded_child_count {
                self.remember_loaded_list_directory_children(&request.path, loaded_child_count);
            }
            if let Some(failure) = pending_failure {
                self.accept_expanded_directory_load_failure(pane_id, &request.path, failure);
            }
            return self.schedule_visible_list_directory_summaries_for_pane(pane_id);
        }

        let loaded_path = request.path.clone();
        let loaded_successfully = scan.is_ok();
        let mut loaded_child_count = None;
        let pending_failure = {
            let Some(expanded) = self.expanded_directories.get_mut(&request.path) else {
                return Task::none();
            };
            if !expanded_load_request_matches_directory(&request, expanded) {
                return Task::none();
            }
            expanded.load_context = None;
            expanded.load_cancel = None;

            match scan {
                Ok(scan) => {
                    expanded.entries = scan.entries;
                    expanded.status = ExpandedDirectoryStatus::Loaded;
                    loaded_child_count = Some(expanded.entries.len());
                    None
                }
                Err(failure) => {
                    expanded.entries.clear();
                    expanded.status = ExpandedDirectoryStatus::Error;
                    Some(failure)
                }
            }
        };

        if let Some(loaded_child_count) = loaded_child_count {
            self.remember_loaded_list_directory_children(&request.path, loaded_child_count);
        }
        if let Some(failure) = pending_failure {
            self.accept_expanded_directory_load_failure(pane_id, &request.path, failure);
        }
        let pending_keyboard_focus = self.complete_pending_keyboard_column_focus(&loaded_path);
        let expansion_follow = if load_owner.advances_list_follow() {
            self.advance_list_expansion_follow(loaded_successfully)
        } else {
            Task::none()
        };
        let command = self.focus_created_entry_for_rename();
        self.sync_active_tab_state();
        self.resort_size_sorted_list_panes();
        Task::batch([
            pending_keyboard_focus,
            expansion_follow,
            command,
            self.schedule_visible_list_directory_summaries_for_pane(pane_id),
            self.schedule_thumbnail_refresh(),
            self.remeasure_active_file_drop_layout(),
        ])
    }
    fn browser_tree_load_owner(
        &self,
        request: &ExpandedDirectoryLoadRequest,
    ) -> Option<BrowserTreeLoadOwner> {
        match &request.context {
            DirectoryExpansionLoadContext::BrowserTree { pane_id } => {
                Some(BrowserTreeLoadOwner::Persistent(*pane_id))
            }
            DirectoryExpansionLoadContext::ListFollow { pane_id, .. }
                if self.list_expansion_follow_request_is_current(request) =>
            {
                Some(BrowserTreeLoadOwner::ListFollow(*pane_id))
            }
            DirectoryExpansionLoadContext::ListFollow { .. }
            | DirectoryExpansionLoadContext::IconGrid { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserTreeLoadOwner {
    Persistent(crate::model::BrowserPaneId),
    ListFollow(crate::model::BrowserPaneId),
}

impl BrowserTreeLoadOwner {
    fn pane_id(self) -> crate::model::BrowserPaneId {
        match self {
            Self::Persistent(pane_id) | Self::ListFollow(pane_id) => pane_id,
        }
    }

    fn advances_list_follow(self) -> bool {
        matches!(self, Self::ListFollow(_))
    }
}

fn expanded_load_request_matches_directory(
    request: &ExpandedDirectoryLoadRequest,
    expanded: &ExpandedDirectory,
) -> bool {
    request.generation == expanded.load_generation
        && expanded.load_context.as_ref() == Some(&request.context)
}
