use std::path::PathBuf;

use iced::Task;

use super::FileBrowser;
use crate::commands::{file_properties_command, set_file_properties_permissions_command};
use crate::model::{
    FilePropertiesCategory, FilePropertiesLoadState, FilePropertiesPermissionAccess,
    FilePropertiesPermissionClass, FilePropertiesPermissionUpdate, FilePropertiesPermissions,
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
        self.operation_queue.close_panel();
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.selection_marquee = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.properties = Some(FilePropertiesState::loading(path.clone()));

        Task::batch([
            self.commit_rename_if_active(),
            self.ensure_properties_window(),
            file_properties_command(path),
        ])
    }

    pub(super) fn accept_file_properties(
        &mut self,
        path: PathBuf,
        outcome: Result<FilePropertiesSnapshot, String>,
    ) -> Task<Message> {
        let Some(properties) = self.properties.as_mut() else {
            return Task::none();
        };
        if properties.path != path {
            return Task::none();
        }

        properties.load_state = match outcome {
            Ok(snapshot) => FilePropertiesLoadState::Loaded(snapshot),
            Err(error) => FilePropertiesLoadState::Failed(error),
        };
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
        if properties.permission_update.is_saving() {
            return Task::none();
        }
        let FilePropertiesLoadState::Loaded(snapshot) = &properties.load_state else {
            return Task::none();
        };
        let Some(current_permissions) = snapshot.permissions else {
            return Task::none();
        };

        let next_permissions = current_permissions.toggled(class, access);
        properties.permission_update = FilePropertiesPermissionUpdate::Saving(next_permissions);
        set_file_properties_permissions_command(properties.path.clone(), next_permissions)
    }

    pub(super) fn accept_file_properties_permissions(
        &mut self,
        path: PathBuf,
        outcome: Result<FilePropertiesPermissions, String>,
    ) -> Task<Message> {
        let Some(properties) = self.properties.as_mut() else {
            return Task::none();
        };
        if properties.path != path {
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
