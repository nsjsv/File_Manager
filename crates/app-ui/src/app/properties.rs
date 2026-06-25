use std::path::PathBuf;

use iced::Task;
use tokio_util::sync::CancellationToken;

use super::FileBrowser;
use crate::commands::{
    apply_file_properties_permissions_to_enclosed_items_command, file_properties_command,
    set_file_properties_permissions_command,
};
use crate::model::{
    FilePropertiesCategory, FilePropertiesDirectoryContents, FilePropertiesDirectoryContentsState,
    FilePropertiesLoadState, FilePropertiesPermissionAccess, FilePropertiesPermissionClass,
    FilePropertiesPermissionUpdate, FilePropertiesPermissions, FilePropertiesRequest,
    FilePropertiesSnapshot, FilePropertiesState, Message,
};

impl FileBrowser {
    pub(super) fn open_selected_file_properties(&mut self) -> Task<Message> {
        if !self.file_browser_content_shortcuts_enabled() {
            return Task::none();
        }

        let Some(path) = self.single_selected_properties_path() else {
            return Task::none();
        };

        self.open_file_properties(path)
    }

    pub(super) fn open_file_properties(&mut self, path: PathBuf) -> Task<Message> {
        self.context_menu = None;
        self.open_with = None;
        self.archive_creation = None;
        self.archive_extraction = None;
        self.operation_queue.close_panel();
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.selection_marquee = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        let (request, cancellation) = self.next_file_properties_request(path);
        self.properties = Some(FilePropertiesState::loading(
            request.clone(),
            cancellation.clone(),
        ));

        Task::batch([
            self.commit_rename_if_active(),
            self.ensure_properties_window(),
            file_properties_command(request, cancellation),
        ])
    }

    pub(super) fn clear_file_properties_state(&mut self) {
        if let Some(properties) = self.properties.as_mut() {
            properties.cancel_load();
        }
        self.properties = None;
    }

    pub(super) fn next_file_properties_request(
        &mut self,
        path: PathBuf,
    ) -> (FilePropertiesRequest, CancellationToken) {
        if let Some(properties) = self.properties.as_mut() {
            properties.cancel_load();
        }
        self.properties_load_generation = self.properties_load_generation.wrapping_add(1);
        let cancellation = CancellationToken::new();
        (
            FilePropertiesRequest {
                path,
                generation: self.properties_load_generation,
            },
            cancellation,
        )
    }

    pub(super) fn accept_file_properties(
        &mut self,
        request: FilePropertiesRequest,
        outcome: Result<FilePropertiesSnapshot, String>,
    ) -> Task<Message> {
        let Some(properties) = self.properties.as_mut() else {
            return Task::none();
        };
        if !file_properties_request_matches(properties, &request) {
            return Task::none();
        }

        if outcome.as_ref().is_ok_and(|snapshot| {
            !matches!(
                snapshot.directory_contents,
                FilePropertiesDirectoryContentsState::Loading(_)
            )
        }) {
            properties.load_cancel = None;
        }
        properties.load_state = match outcome {
            Ok(snapshot) => FilePropertiesLoadState::Loaded(snapshot),
            Err(error) => {
                properties.load_cancel = None;
                FilePropertiesLoadState::Failed(error)
            }
        };
        Task::none()
    }

    pub(super) fn accept_file_properties_directory_contents_progress(
        &mut self,
        request: FilePropertiesRequest,
        contents: FilePropertiesDirectoryContents,
    ) -> Task<Message> {
        let Some(properties) = self.properties.as_mut() else {
            return Task::none();
        };
        if !file_properties_request_matches(properties, &request) {
            return Task::none();
        }
        let FilePropertiesLoadState::Loaded(snapshot) = &mut properties.load_state else {
            return Task::none();
        };
        snapshot.size_bytes = contents.total_size_bytes;
        snapshot.disk_size_bytes = contents.total_disk_size_bytes;
        snapshot.directory_contents = FilePropertiesDirectoryContentsState::Loading(Some(contents));
        Task::none()
    }

    pub(super) fn accept_file_properties_directory_contents(
        &mut self,
        request: FilePropertiesRequest,
        outcome: Result<FilePropertiesDirectoryContents, String>,
    ) -> Task<Message> {
        let Some(properties) = self.properties.as_mut() else {
            return Task::none();
        };
        if !file_properties_request_matches(properties, &request) {
            return Task::none();
        }
        let FilePropertiesLoadState::Loaded(snapshot) = &mut properties.load_state else {
            return Task::none();
        };
        properties.load_cancel = None;
        match outcome {
            Ok(contents) => {
                snapshot.size_bytes = contents.total_size_bytes;
                snapshot.disk_size_bytes = contents.total_disk_size_bytes;
                snapshot.directory_contents =
                    FilePropertiesDirectoryContentsState::Loaded(contents);
            }
            Err(error) => {
                snapshot.directory_contents = FilePropertiesDirectoryContentsState::Failed(error);
            }
        }
        Task::none()
    }

    pub(super) fn select_file_properties_category(
        &mut self,
        category: FilePropertiesCategory,
    ) -> Task<Message> {
        if let Some(properties) = self.properties.as_mut() {
            properties.selected_category = category;
        }
        Task::none()
    }

    pub(super) fn toggle_file_properties_permission(
        &mut self,
        class: FilePropertiesPermissionClass,
        access: FilePropertiesPermissionAccess,
    ) -> Task<Message> {
        let Some(properties) = self.properties.as_mut() else {
            return Task::none();
        };
        if properties.permission_update.is_in_progress() {
            return Task::none();
        }
        let FilePropertiesLoadState::Loaded(snapshot) = &properties.load_state else {
            return Task::none();
        };
        let Some(current_permissions) = snapshot.permissions else {
            return Task::none();
        };

        let next_permissions = current_permissions.toggled(class, access);
        properties.permission_update = FilePropertiesPermissionUpdate::SavingCurrentItem {
            permissions: next_permissions,
        };
        set_file_properties_permissions_command(
            current_file_properties_request(properties),
            next_permissions,
        )
    }

    pub(super) fn apply_file_properties_permissions_to_enclosed_items(&mut self) -> Task<Message> {
        let Some(properties) = self.properties.as_mut() else {
            return Task::none();
        };
        if properties.permission_update.is_in_progress() {
            return Task::none();
        }
        let FilePropertiesLoadState::Loaded(snapshot) = &properties.load_state else {
            return Task::none();
        };
        if snapshot.kind != file_core::FileKind::Directory {
            return Task::none();
        }
        let Some(current_permissions) = snapshot.permissions else {
            return Task::none();
        };

        properties.permission_update = FilePropertiesPermissionUpdate::ApplyingToEnclosedItems {
            permissions: current_permissions,
        };
        apply_file_properties_permissions_to_enclosed_items_command(
            current_file_properties_request(properties),
            current_permissions,
        )
    }

    pub(super) fn accept_file_properties_permissions(
        &mut self,
        request: FilePropertiesRequest,
        outcome: Result<FilePropertiesPermissions, String>,
    ) -> Task<Message> {
        let Some(properties) = self.properties.as_mut() else {
            return Task::none();
        };
        if !file_properties_request_matches(properties, &request) {
            return Task::none();
        }

        match outcome {
            Ok(permissions) => {
                if let FilePropertiesLoadState::Loaded(snapshot) = &mut properties.load_state {
                    snapshot.permissions = Some(permissions);
                }
                properties.permission_update = FilePropertiesPermissionUpdate::Idle;
            }
            Err(error) => {
                properties.permission_update = FilePropertiesPermissionUpdate::Failed(error);
            }
        }
        Task::none()
    }

    pub(super) fn accept_file_properties_enclosed_permissions(
        &mut self,
        request: FilePropertiesRequest,
        outcome: Result<FilePropertiesPermissions, String>,
    ) -> Task<Message> {
        let Some(properties) = self.properties.as_mut() else {
            return Task::none();
        };
        if !file_properties_request_matches(properties, &request) {
            return Task::none();
        }

        match outcome {
            Ok(permissions) => {
                if let FilePropertiesLoadState::Loaded(snapshot) = &mut properties.load_state {
                    snapshot.permissions = Some(permissions);
                }
                properties.permission_update = FilePropertiesPermissionUpdate::Idle;
            }
            Err(error) => {
                properties.permission_update = FilePropertiesPermissionUpdate::Failed(error);
            }
        }
        Task::none()
    }

    fn single_selected_properties_path(&self) -> Option<PathBuf> {
        match self.selected_paths.len() {
            0 => self.selected.clone(),
            1 => self.selected_paths.iter().next().cloned(),
            _ => None,
        }
    }
}

fn file_properties_request_matches(
    properties: &FilePropertiesState,
    request: &FilePropertiesRequest,
) -> bool {
    properties.path == request.path && properties.load_generation == request.generation
}

fn current_file_properties_request(properties: &FilePropertiesState) -> FilePropertiesRequest {
    FilePropertiesRequest {
        path: properties.path.clone(),
        generation: properties.load_generation,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use file_core::FileKind;

    use super::*;

    #[test]
    fn properties_directory_contents_rejects_stale_generation_for_same_path() {
        let path = PathBuf::from("/workspace/project");
        let mut browser = browser_with_loaded_directory_properties(path.clone(), 2);
        let stale_request = FilePropertiesRequest {
            path,
            generation: 1,
        };

        let _command = browser.accept_file_properties_directory_contents(
            stale_request,
            Ok(directory_contents(9, 9, 9)),
        );

        let FilePropertiesLoadState::Loaded(snapshot) =
            &browser.properties.as_ref().unwrap().load_state
        else {
            panic!("properties should remain loaded");
        };
        assert!(matches!(
            snapshot.directory_contents,
            FilePropertiesDirectoryContentsState::Loading(None)
        ));
        assert_eq!(snapshot.size_bytes, 0);
    }

    #[test]
    fn properties_directory_contents_accepts_current_generation() {
        let path = PathBuf::from("/workspace/project");
        let mut browser = browser_with_loaded_directory_properties(path.clone(), 2);
        let request = FilePropertiesRequest {
            path,
            generation: 2,
        };

        let _command = browser
            .accept_file_properties_directory_contents(request, Ok(directory_contents(3, 2, 512)));

        let FilePropertiesLoadState::Loaded(snapshot) =
            &browser.properties.as_ref().unwrap().load_state
        else {
            panic!("properties should remain loaded");
        };
        assert_eq!(snapshot.size_bytes, 512);
        assert!(matches!(
            snapshot.directory_contents,
            FilePropertiesDirectoryContentsState::Loaded(FilePropertiesDirectoryContents {
                file_count: 3,
                directory_count: 2,
                ..
            })
        ));
        assert!(browser.properties.as_ref().unwrap().load_cancel.is_none());
    }

    #[test]
    fn current_item_permission_save_updates_snapshot_permissions() {
        let path = PathBuf::from("/workspace/project");
        let mut browser = browser_with_loaded_directory_properties(path.clone(), 2);
        browser.properties.as_mut().unwrap().permission_update =
            FilePropertiesPermissionUpdate::SavingCurrentItem {
                permissions: FilePropertiesPermissions::from_mode(0o755),
            };
        let request = FilePropertiesRequest {
            path,
            generation: 2,
        };

        let _command = browser.accept_file_properties_permissions(
            request,
            Ok(FilePropertiesPermissions::from_mode(0o700)),
        );

        let FilePropertiesLoadState::Loaded(snapshot) =
            &browser.properties.as_ref().unwrap().load_state
        else {
            panic!("properties should remain loaded");
        };
        assert_eq!(
            snapshot.permissions,
            Some(FilePropertiesPermissions::from_mode(0o700))
        );
        assert_eq!(
            browser.properties.as_ref().unwrap().permission_update,
            FilePropertiesPermissionUpdate::Idle
        );
    }

    #[test]
    fn enclosed_permission_result_rejects_stale_generation_for_same_path() {
        let path = PathBuf::from("/workspace/project");
        let mut browser = browser_with_loaded_directory_properties(path.clone(), 2);
        browser.properties.as_mut().unwrap().permission_update =
            FilePropertiesPermissionUpdate::ApplyingToEnclosedItems {
                permissions: FilePropertiesPermissions::from_mode(0o755),
            };
        let stale_request = FilePropertiesRequest {
            path,
            generation: 1,
        };

        let _command = browser.accept_file_properties_enclosed_permissions(
            stale_request,
            Ok(FilePropertiesPermissions::from_mode(0o700)),
        );

        let FilePropertiesLoadState::Loaded(snapshot) =
            &browser.properties.as_ref().unwrap().load_state
        else {
            panic!("properties should remain loaded");
        };
        assert_eq!(
            snapshot.permissions,
            Some(FilePropertiesPermissions::from_mode(0o755))
        );
        assert_eq!(
            browser.properties.as_ref().unwrap().permission_update,
            FilePropertiesPermissionUpdate::ApplyingToEnclosedItems {
                permissions: FilePropertiesPermissions::from_mode(0o755)
            }
        );
    }

    #[test]
    fn permission_action_is_ignored_while_enclosed_permissions_are_applying() {
        let path = PathBuf::from("/workspace/project");
        let mut browser = browser_with_loaded_directory_properties(path, 2);
        browser.properties.as_mut().unwrap().permission_update =
            FilePropertiesPermissionUpdate::ApplyingToEnclosedItems {
                permissions: FilePropertiesPermissions::from_mode(0o755),
            };

        let _command = browser.toggle_file_properties_permission(
            FilePropertiesPermissionClass::Owner,
            FilePropertiesPermissionAccess::Write,
        );

        assert_eq!(
            browser.properties.as_ref().unwrap().permission_update,
            FilePropertiesPermissionUpdate::ApplyingToEnclosedItems {
                permissions: FilePropertiesPermissions::from_mode(0o755)
            }
        );
    }

    #[test]
    fn enclosed_permission_action_is_ignored_while_enclosed_permissions_are_applying() {
        let path = PathBuf::from("/workspace/project");
        let mut browser = browser_with_loaded_directory_properties(path, 2);
        browser.properties.as_mut().unwrap().permission_update =
            FilePropertiesPermissionUpdate::ApplyingToEnclosedItems {
                permissions: FilePropertiesPermissions::from_mode(0o755),
            };

        let _command = browser.apply_file_properties_permissions_to_enclosed_items();

        assert_eq!(
            browser.properties.as_ref().unwrap().permission_update,
            FilePropertiesPermissionUpdate::ApplyingToEnclosedItems {
                permissions: FilePropertiesPermissions::from_mode(0o755)
            }
        );
    }

    #[test]
    fn opening_new_properties_cancels_previous_load() {
        let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
        let old_cancel = CancellationToken::new();
        browser.properties = Some(FilePropertiesState::loading(
            FilePropertiesRequest {
                path: PathBuf::from("/workspace/old"),
                generation: 1,
            },
            old_cancel.clone(),
        ));
        browser.properties_load_generation = 1;

        let (_request, _cancel) =
            browser.next_file_properties_request(PathBuf::from("/workspace/new"));

        assert!(old_cancel.is_cancelled());
        assert_eq!(browser.properties_load_generation, 2);
    }

    fn browser_with_loaded_directory_properties(path: PathBuf, generation: u64) -> FileBrowser {
        let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
        browser.properties = Some(FilePropertiesState {
            path: path.clone(),
            load_state: FilePropertiesLoadState::Loaded(directory_snapshot(path)),
            selected_category: FilePropertiesCategory::Information,
            permission_update: FilePropertiesPermissionUpdate::Idle,
            load_generation: generation,
            load_cancel: Some(CancellationToken::new()),
        });
        browser
    }

    fn directory_snapshot(path: PathBuf) -> FilePropertiesSnapshot {
        FilePropertiesSnapshot {
            name: OsString::from("project"),
            kind: FileKind::Directory,
            type_label: "Folder".to_owned(),
            location: path.parent().unwrap().to_path_buf(),
            created: None,
            modified: None,
            accessed: None,
            size_bytes: 0,
            disk_size_bytes: 0,
            directory_contents: FilePropertiesDirectoryContentsState::Loading(None),
            permissions: Some(FilePropertiesPermissions::from_mode(0o755)),
        }
    }

    fn directory_contents(
        file_count: usize,
        directory_count: usize,
        total_size_bytes: u64,
    ) -> FilePropertiesDirectoryContents {
        FilePropertiesDirectoryContents {
            file_count,
            directory_count,
            total_size_bytes,
            total_disk_size_bytes: total_size_bytes,
        }
    }
}
