use std::path::PathBuf;

use desktop_linux::{StorageDevice, StorageDeviceId};
use iced::Task;

use super::FileBrowser;
use crate::commands::{sidebar_device_action_command, sidebar_devices_command};
use crate::model::{BrowserPaneId, ContextMenuState, Message, NavigationMode};
use crate::sidebar_devices::{SidebarDeviceAction, SidebarDeviceContextMenuState};

impl FileBrowser {
    pub(crate) fn sidebar_device_is_selected(&self, id: &StorageDeviceId) -> bool {
        self.sidebar_devices
            .selected_device_id(&self.current_dir)
            .is_some_and(|selected_id| selected_id == id)
    }

    pub(super) fn accept_sidebar_devices(
        &mut self,
        result: Result<Vec<StorageDevice>, String>,
    ) -> Task<Message> {
        match result {
            Ok(devices) => self.sidebar_devices.accept_loaded(devices),
            Err(error) => self.sidebar_devices.accept_unavailable(error),
        }
        Task::none()
    }

    pub(super) fn refresh_sidebar_devices(&mut self) -> Task<Message> {
        if self.sidebar_devices.begin_refresh() {
            sidebar_devices_command()
        } else {
            Task::none()
        }
    }

    pub(super) fn handle_sidebar_device_hovered(&mut self, id: StorageDeviceId) -> Task<Message> {
        self.hovered_sidebar_device = Some(id);
        Task::none()
    }

    pub(super) fn handle_sidebar_device_hover_cleared(
        &mut self,
        id: StorageDeviceId,
    ) -> Task<Message> {
        if self.hovered_sidebar_device.as_ref() == Some(&id) {
            self.hovered_sidebar_device = None;
        }
        Task::none()
    }

    pub(super) fn handle_sidebar_device_pressed(&mut self, id: StorageDeviceId) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        let Some(device) = self.sidebar_devices.device(&id).cloned() else {
            return rename_command;
        };
        self.context_menu = None;

        if let Some(path) = device.primary_mount_path().map(PathBuf::from) {
            return Task::batch([
                rename_command,
                self.navigate_to(path, NavigationMode::RecordHistory),
            ]);
        }

        self.sidebar_devices.pending_action = Some(id.clone());
        Task::batch([
            rename_command,
            sidebar_device_action_command(id, SidebarDeviceAction::Mount),
        ])
    }

    pub(super) fn handle_sidebar_device_middle_pressed(
        &mut self,
        pane_id: BrowserPaneId,
        id: StorageDeviceId,
    ) -> Task<Message> {
        self.activate_pane(pane_id);
        let rename_command = self.commit_rename_if_active();
        let Some(path) = self
            .sidebar_devices
            .device(&id)
            .and_then(|device| device.primary_mount_path().map(PathBuf::from))
        else {
            return rename_command;
        };

        Task::batch([rename_command, self.open_directory_from_middle_click(path)])
    }

    pub(super) fn handle_sidebar_device_right_clicked(
        &mut self,
        id: StorageDeviceId,
    ) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        let Some(device) = self.sidebar_devices.device(&id).cloned() else {
            return rename_command;
        };

        self.clear_preview();
        self.operation_queue.close_panel();
        self.file_drag = None;
        self.selection_marquee = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.context_menu = Some(ContextMenuState::SidebarDevice(
            SidebarDeviceContextMenuState {
                device,
                position: self.cursor_position,
            },
        ));
        rename_command
    }

    pub(super) fn perform_sidebar_device_action(
        &mut self,
        id: StorageDeviceId,
        action: SidebarDeviceAction,
    ) -> Task<Message> {
        self.context_menu = None;
        self.sidebar_devices.pending_action = Some(id.clone());
        sidebar_device_action_command(id, action)
    }

    pub(super) fn accept_sidebar_device_action_finished(
        &mut self,
        id: StorageDeviceId,
        action: SidebarDeviceAction,
        result: Result<Option<PathBuf>, String>,
    ) -> Task<Message> {
        if self.sidebar_devices.pending_action.as_ref() == Some(&id) {
            self.sidebar_devices.pending_action = None;
        }

        let refresh_command = self.refresh_sidebar_devices();
        match result {
            Ok(Some(path)) if action == SidebarDeviceAction::Mount => Task::batch([
                refresh_command,
                self.navigate_to(path, NavigationMode::RecordHistory),
            ]),
            Ok(_) => refresh_command,
            Err(error) => {
                self.show_global_error(format!(
                    "Could not {} storage device: {error}",
                    sidebar_device_action_error_verb(action)
                ));
                refresh_command
            }
        }
    }
}

fn sidebar_device_action_error_verb(action: SidebarDeviceAction) -> &'static str {
    match action {
        SidebarDeviceAction::Mount => "mount",
        SidebarDeviceAction::Unmount => "unmount",
        SidebarDeviceAction::Eject => "eject",
    }
}
