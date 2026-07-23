use std::path::{Path, PathBuf};

use desktop_linux::{StorageDevice, StorageDeviceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarDeviceAction {
    Mount,
    Unmount,
    Eject,
}

impl SidebarDeviceAction {
    pub(crate) fn label(self, device: &SidebarDeviceEntry) -> &'static str {
        match self {
            Self::Mount => "Mount",
            Self::Unmount => "Unmount",
            Self::Eject if device.can_power_off => "Safely Remove",
            Self::Eject => "Eject",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidebarDeviceEntry {
    pub(crate) id: StorageDeviceId,
    pub(crate) label: String,
    pub(crate) detail: Option<String>,
    pub(crate) size_bytes: u64,
    pub(crate) mount_points: Vec<PathBuf>,
    pub(crate) can_unmount: bool,
    pub(crate) can_eject: bool,
    pub(crate) can_power_off: bool,
}

impl SidebarDeviceEntry {
    pub(crate) fn from_storage_device(device: StorageDevice) -> Self {
        let mount_points = device.mount_state.mount_points().to_vec();
        let detail = device
            .primary_mount_path()
            .map(|path| path.to_string_lossy().into_owned())
            .or_else(|| {
                device
                    .device_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
            })
            .filter(|detail| !detail.is_empty());

        Self {
            id: device.id,
            label: device.label,
            detail,
            size_bytes: device.size_bytes,
            mount_points,
            can_unmount: device.can_unmount,
            can_eject: device.can_eject,
            can_power_off: device.can_power_off,
        }
    }

    pub(crate) fn primary_mount_path(&self) -> Option<&Path> {
        self.mount_points.first().map(PathBuf::as_path)
    }

    pub(crate) fn is_mounted(&self) -> bool {
        self.primary_mount_path().is_some()
    }

    pub(crate) fn available_actions(&self) -> Vec<SidebarDeviceAction> {
        let mut actions = Vec::new();
        if self.is_mounted() && self.can_unmount {
            actions.push(SidebarDeviceAction::Unmount);
        }
        if !self.is_mounted() {
            actions.push(SidebarDeviceAction::Mount);
        }
        if self.can_power_off || self.can_eject {
            actions.push(SidebarDeviceAction::Eject);
        }
        actions
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarDeviceContextMenuState {
    pub(crate) device: SidebarDeviceEntry,
    pub(crate) position: iced::Point,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SidebarDeviceState {
    pub(crate) devices: Vec<SidebarDeviceEntry>,
    pub(crate) unavailable: Option<String>,
    pub(crate) is_loading: bool,
    pub(crate) pending_action: Option<StorageDeviceId>,
}

impl SidebarDeviceState {
    pub(crate) fn loading() -> Self {
        Self {
            is_loading: true,
            ..Self::default()
        }
    }

    pub(crate) fn accept_loaded(&mut self, devices: Vec<StorageDevice>) {
        self.devices = devices
            .into_iter()
            .map(SidebarDeviceEntry::from_storage_device)
            .collect();
        self.unavailable = None;
        self.is_loading = false;
    }

    pub(crate) fn accept_unavailable(&mut self, error: String) {
        self.devices.clear();
        self.unavailable = Some(error);
        self.is_loading = false;
    }

    pub(crate) fn begin_refresh(&mut self) -> bool {
        if self.is_loading {
            false
        } else {
            self.is_loading = true;
            true
        }
    }

    pub(crate) fn device(&self, id: &StorageDeviceId) -> Option<&SidebarDeviceEntry> {
        self.devices.iter().find(|device| &device.id == id)
    }

    pub(crate) fn selected_device_id(&self, current_dir: &Path) -> Option<&StorageDeviceId> {
        selected_sidebar_device(&self.devices, current_dir).map(|device| &device.id)
    }
}

pub(crate) fn selected_sidebar_device<'a>(
    devices: &'a [SidebarDeviceEntry],
    current_dir: &Path,
) -> Option<&'a SidebarDeviceEntry> {
    devices
        .iter()
        .flat_map(|device| {
            device
                .mount_points
                .iter()
                .filter(move |mount_point| current_dir.starts_with(mount_point))
                .map(move |mount_point| (device, mount_point.components().count()))
        })
        .max_by_key(|(_, depth)| *depth)
        .map(|(device, _)| device)
}

#[cfg(test)]
mod tests {
    use super::*;
    use desktop_linux::StorageDeviceMountState;

    fn device(id: &str, mount_points: Vec<PathBuf>) -> SidebarDeviceEntry {
        SidebarDeviceEntry {
            id: StorageDeviceId::new(id),
            label: id.to_owned(),
            detail: None,
            size_bytes: 0,
            mount_points,
            can_unmount: true,
            can_eject: false,
            can_power_off: false,
        }
    }

    #[test]
    fn selected_device_uses_longest_matching_mount_prefix() {
        let devices = vec![
            device("outer", vec![PathBuf::from("/run/media/user")]),
            device("inner", vec![PathBuf::from("/run/media/user/photos")]),
        ];

        let selected = selected_sidebar_device(&devices, Path::new("/run/media/user/photos/raw"))
            .expect("selected device");

        assert_eq!(selected.id, StorageDeviceId::new("inner"));
    }

    #[test]
    fn unmounted_device_offers_mount_action() {
        let device = device("disk", Vec::new());

        assert_eq!(device.available_actions(), vec![SidebarDeviceAction::Mount]);
    }

    #[test]
    fn mounted_removable_device_offers_unmount_and_eject() {
        let mut device = device("disk", vec![PathBuf::from("/media/disk")]);
        device.can_eject = true;

        assert_eq!(
            device.available_actions(),
            vec![SidebarDeviceAction::Unmount, SidebarDeviceAction::Eject]
        );
    }

    #[test]
    fn storage_device_projection_keeps_devices_out_of_favorites_model() {
        let storage = StorageDevice {
            id: StorageDeviceId::new("/org/freedesktop/UDisks2/block_devices/sdb1"),
            label: "USB".to_owned(),
            device_path: Some(PathBuf::from("/dev/sdb1")),
            filesystem_type: "vfat".to_owned(),
            size_bytes: 8,
            mount_state: StorageDeviceMountState::Unmounted,
            is_removable: true,
            can_unmount: false,
            can_eject: true,
            can_power_off: true,
        };

        let entry = SidebarDeviceEntry::from_storage_device(storage);

        assert_eq!(entry.label, "USB");
        assert!(entry.mount_points.is_empty());
    }
}
