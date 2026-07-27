use std::path::{Path, PathBuf};

use iced::Task;

use super::FileBrowser;
use crate::commands::load_directory_command;
use crate::model::{
    BrowserPaneId, DirectoryLoadFailure, DirectoryLoadRequest, Message, NavigationMode,
};

impl FileBrowser {
    pub(super) fn accept_directory_load_failure(
        &mut self,
        request: DirectoryLoadRequest,
        failure: DirectoryLoadFailure,
    ) -> Task<Message> {
        if request.pane_id == self.active_pane_id() {
            return self.accept_active_directory_load_failure(request, failure);
        }

        self.accept_inactive_directory_load_failure(request, failure)
    }

    fn accept_active_directory_load_failure(
        &mut self,
        request: DirectoryLoadRequest,
        failure: DirectoryLoadFailure,
    ) -> Task<Message> {
        let request_is_current = request.generation == self.directory_load_generation
            && request.path == self.current_dir;
        if !request_is_current {
            return Task::none();
        }

        match failure {
            DirectoryLoadFailure::DirectoryUnavailable { message } => {
                self.recover_active_directory(request.path, message)
            }
            DirectoryLoadFailure::ReadFailed { message } => {
                self.finish_active_failed_directory_load();
                self.show_global_error(message);
                Task::none()
            }
        }
    }

    fn accept_inactive_directory_load_failure(
        &mut self,
        request: DirectoryLoadRequest,
        failure: DirectoryLoadFailure,
    ) -> Task<Message> {
        let request_is_current = self.pane_by_id(request.pane_id).is_some_and(|pane| {
            request.generation == pane.directory_load_generation && request.path == pane.current_dir
        });
        if !request_is_current {
            return Task::none();
        }

        match failure {
            DirectoryLoadFailure::DirectoryUnavailable { message } => {
                self.recover_inactive_directory(request, message)
            }
            DirectoryLoadFailure::ReadFailed { message } => {
                if let Some(pane) = self.pane_by_id_mut(request.pane_id) {
                    finish_failed_directory_load_for_pane(pane);
                }
                self.show_global_error(message);
                Task::none()
            }
        }
    }

    fn recover_active_directory(
        &mut self,
        unavailable_directory: PathBuf,
        unavailable_message: String,
    ) -> Task<Message> {
        let Some(parent_directory) = unavailable_directory.parent().map(ToOwned::to_owned) else {
            self.finish_active_failed_directory_load();
            self.show_global_error(unavailable_message);
            return Task::none();
        };

        self.back_stack
            .retain(|path| !path.starts_with(&unavailable_directory));
        while self
            .back_stack
            .last()
            .is_some_and(|path| path == &parent_directory)
        {
            self.back_stack.pop();
        }
        self.forward_stack
            .retain(|path| !path.starts_with(&unavailable_directory));
        self.navigate_to(parent_directory, NavigationMode::KeepHistory)
    }

    fn recover_inactive_directory(
        &mut self,
        request: DirectoryLoadRequest,
        unavailable_message: String,
    ) -> Task<Message> {
        let Some(parent_directory) = request.path.parent().map(ToOwned::to_owned) else {
            if let Some(pane) = self.pane_by_id_mut(request.pane_id) {
                finish_failed_directory_load_for_pane(pane);
            }
            self.show_global_error(unavailable_message);
            return Task::none();
        };

        let options = self.options.clone();
        let load_command = {
            let Some(pane) = self.pane_by_id_mut(request.pane_id) else {
                return Task::none();
            };
            pane.recover_from_unavailable_directory(&request.path, parent_directory.clone());
            let (parent_request, cancellation) =
                Self::next_inactive_directory_load_request(pane, parent_directory);
            load_directory_command(parent_request, options, cancellation)
        };

        Task::batch([load_command, self.request_browser_session_save()])
    }

    fn finish_active_failed_directory_load(&mut self) {
        self.is_loading = false;
        self.directory_loading_placeholder_entries.clear();
        self.directory_load_cancel = None;
        self.sync_active_tab_state();
    }

    pub(super) fn accept_expanded_directory_load_failure(
        &mut self,
        pane_id: BrowserPaneId,
        unavailable_directory: &Path,
        failure: DirectoryLoadFailure,
    ) {
        match failure {
            DirectoryLoadFailure::DirectoryUnavailable { .. } => {
                self.discard_unavailable_expanded_directory(pane_id, unavailable_directory);
            }
            DirectoryLoadFailure::ReadFailed { message } => self.show_global_error(message),
        }
    }

    fn discard_unavailable_expanded_directory(
        &mut self,
        pane_id: BrowserPaneId,
        unavailable_directory: &Path,
    ) {
        if pane_id == self.active_pane_id() {
            let mut active_pane = self.capture_active_pane_snapshot();
            active_pane.discard_unavailable_directory_subtree(unavailable_directory);
            self.apply_pane_browsing_snapshot(active_pane);
            self.sync_active_pane_state();
            return;
        }

        if let Some(pane) = self.pane_by_id_mut(pane_id) {
            pane.discard_unavailable_directory_subtree(unavailable_directory);
        }
    }
}

fn finish_failed_directory_load_for_pane(pane: &mut crate::model::BrowserPane) {
    pane.is_loading = false;
    pane.directory_loading_placeholder_entries.clear();
    pane.directory_load_cancel = None;
    pane.sync_active_tab_state();
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use file_core::{DirectoryEntry, EntryMetadata, FileKind};

    use super::*;
    use crate::app::global_error::{recorded_global_error_count, reset_recorded_global_errors};
    use crate::config;
    use crate::model::{BrowserTab, ExpandedDirectory, ExpandedDirectoryStatus};

    #[test]
    fn unavailable_active_directory_recovers_to_parent_without_global_error() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let parent_directory = PathBuf::from("/workspace/project");
        let unavailable_directory = parent_directory.join("removed");
        browser.current_dir = unavailable_directory.clone();
        browser.directory_load_generation = 7;
        browser.directory_load_cancel = Some(tokio_util::sync::CancellationToken::new());
        browser.is_loading = true;
        browser.back_stack = vec![
            parent_directory.clone(),
            unavailable_directory.join("older-child"),
        ];
        browser.forward_stack = vec![unavailable_directory.join("newer-child")];
        reset_recorded_global_errors();

        drop(browser.accept_directory_load_failure(
            DirectoryLoadRequest {
                pane_id: BrowserPaneId::PRIMARY,
                path: unavailable_directory.clone(),
                generation: 7,
            },
            unavailable_failure(&unavailable_directory),
        ));

        assert_eq!(browser.current_dir, parent_directory.clone());
        assert!(browser.is_loading);
        assert_eq!(browser.directory_load_generation, 8);
        assert_eq!(browser.current_error(), None);
        assert_eq!(recorded_global_error_count(), 0);
        assert!(browser.back_stack.is_empty());
        assert!(browser.forward_stack.is_empty());
    }

    #[test]
    fn stale_unavailable_directory_failure_does_not_change_navigation_state() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let current_directory = PathBuf::from("/workspace/current");
        let stale_directory = PathBuf::from("/workspace/stale");
        browser.current_dir = current_directory.clone();
        browser.directory_load_generation = 9;
        browser.is_loading = true;
        reset_recorded_global_errors();

        drop(browser.accept_directory_load_failure(
            DirectoryLoadRequest {
                pane_id: BrowserPaneId::PRIMARY,
                path: stale_directory.clone(),
                generation: 8,
            },
            unavailable_failure(&stale_directory),
        ));

        assert_eq!(browser.current_dir, current_directory);
        assert_eq!(browser.directory_load_generation, 9);
        assert!(browser.is_loading);
        assert_eq!(browser.current_error(), None);
        assert_eq!(recorded_global_error_count(), 0);
    }

    #[test]
    fn unavailable_inactive_directory_recovers_its_own_pane() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let inactive_pane_id = BrowserPaneId(9);
        let parent_directory = PathBuf::from("/workspace/secondary");
        let unavailable_directory = parent_directory.join("removed");
        let mut inactive_pane = browser.capture_active_pane_snapshot();
        inactive_pane.id = inactive_pane_id;
        inactive_pane.current_dir = unavailable_directory.clone();
        inactive_pane.tabs = vec![BrowserTab::directory(0, unavailable_directory.clone())];
        inactive_pane.active_tab_id = 0;
        inactive_pane.directory_load_generation = 12;
        inactive_pane.is_loading = true;
        inactive_pane.sync_active_tab_state();
        browser.panes.push(inactive_pane);
        reset_recorded_global_errors();

        drop(browser.accept_directory_load_failure(
            DirectoryLoadRequest {
                pane_id: inactive_pane_id,
                path: unavailable_directory.clone(),
                generation: 12,
            },
            unavailable_failure(&unavailable_directory),
        ));

        let recovered_pane = browser.pane_by_id(inactive_pane_id).expect("inactive pane");
        assert_eq!(recovered_pane.current_dir, parent_directory);
        assert!(recovered_pane.is_loading);
        assert_eq!(recovered_pane.directory_load_generation, 13);
        assert_eq!(browser.current_error(), None);
        assert_eq!(recorded_global_error_count(), 0);
    }

    #[test]
    fn unavailable_expanded_directory_discards_its_cached_subtree_without_global_error() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let root_directory = PathBuf::from("/workspace");
        let unavailable_directory = root_directory.join("removed");
        let unavailable_child = unavailable_directory.join("child");
        let remaining_entry = root_directory.join("remaining.txt");
        browser.current_dir = root_directory;
        browser.entries = vec![
            test_entry(unavailable_directory.clone(), FileKind::Directory),
            test_entry(remaining_entry.clone(), FileKind::File),
        ];
        browser
            .expanded_directories
            .insert(unavailable_directory.clone(), loading_expanded_directory(4));
        browser
            .expanded_directories
            .insert(unavailable_child.clone(), loading_expanded_directory(2));
        reset_recorded_global_errors();

        drop(browser.accept_expanded_directory(
            crate::model::ExpandedDirectoryLoadRequest {
                pane_id: BrowserPaneId::PRIMARY,
                path: unavailable_directory.clone(),
                generation: 4,
            },
            Err(unavailable_failure(&unavailable_directory)),
        ));

        assert_eq!(
            browser
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>(),
            vec![remaining_entry]
        );
        assert!(browser
            .expanded_directories
            .keys()
            .all(|path| !path.starts_with(&unavailable_directory)));
        assert_eq!(browser.current_error(), None);
        assert_eq!(recorded_global_error_count(), 0);
    }

    #[test]
    fn directory_read_failure_still_uses_global_error_channel() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let locked_directory = PathBuf::from("/workspace/locked");
        browser.current_dir = locked_directory.clone();
        browser.directory_load_generation = 3;
        browser.is_loading = true;
        reset_recorded_global_errors();

        drop(browser.accept_directory_load_failure(
            DirectoryLoadRequest {
                pane_id: BrowserPaneId::PRIMARY,
                path: locked_directory,
                generation: 3,
            },
            DirectoryLoadFailure::ReadFailed {
                message: "permission denied".to_owned(),
            },
        ));

        assert!(!browser.is_loading);
        assert_eq!(browser.current_error(), Some("permission denied"));
        assert_eq!(recorded_global_error_count(), 1);
    }

    fn unavailable_failure(path: &Path) -> DirectoryLoadFailure {
        DirectoryLoadFailure::DirectoryUnavailable {
            message: format!("directory is unavailable: {path:?}"),
        }
    }

    fn test_entry(path: PathBuf, kind: FileKind) -> DirectoryEntry {
        DirectoryEntry::new(path, kind, EntryMetadata::default(), false, false, false)
    }

    fn loading_expanded_directory(generation: u64) -> ExpandedDirectory {
        ExpandedDirectory {
            entries: Vec::new(),
            status: ExpandedDirectoryStatus::Loading,
            is_expanded: true,
            is_collapsing: false,
            animation_progress: 1.0,
            load_generation: generation,
            load_cancel: Some(tokio_util::sync::CancellationToken::new()),
        }
    }
}
