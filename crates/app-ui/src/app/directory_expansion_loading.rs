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
        let DirectoryExpansionLoadContext::BrowserTree { pane_id } = &request.context else {
            return Task::none();
        };
        let pane_id = *pane_id;
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
        self.schedule_thumbnail_refresh()
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
        let DirectoryExpansionLoadContext::BrowserTree { pane_id } = &request.context else {
            return Task::none();
        };
        let pane_id = *pane_id;
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
        let mut loaded_child_count = None;
        let pending_failure = {
            let Some(expanded) = self.expanded_directories.get_mut(&request.path) else {
                return Task::none();
            };
            if !expanded_load_request_matches_directory(&request, expanded) {
                return Task::none();
            }
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
        let command = self.focus_created_entry_for_rename();
        self.sync_active_tab_state();
        self.resort_size_sorted_list_panes();
        Task::batch([
            pending_keyboard_focus,
            command,
            self.schedule_visible_list_directory_summaries_for_pane(pane_id),
            self.schedule_thumbnail_refresh(),
        ])
    }
}

fn expanded_load_request_matches_directory(
    request: &ExpandedDirectoryLoadRequest,
    expanded: &ExpandedDirectory,
) -> bool {
    request.generation == expanded.load_generation
}
