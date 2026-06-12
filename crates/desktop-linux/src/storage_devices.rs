use std::collections::HashMap;
use std::convert::Infallible;
use std::ffi::OsString;
use std::fmt;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use thiserror::Error;
use udisks2::zbus::names::OwnedInterfaceName;
use udisks2::zbus::zvariant::{OwnedObjectPath, OwnedValue};
use udisks2::{standard_options, Client};

const FILESYSTEM_INTERFACE: &str = "org.freedesktop.UDisks2.Filesystem";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StorageDeviceId(String);

impl StorageDeviceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StorageDeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageDeviceMountState {
    Mounted(Vec<PathBuf>),
    Unmounted,
}

impl StorageDeviceMountState {
    pub fn mount_points(&self) -> &[PathBuf] {
        match self {
            Self::Mounted(paths) => paths,
            Self::Unmounted => &[],
        }
    }

    pub fn primary_mount_path(&self) -> Option<&Path> {
        self.mount_points().first().map(PathBuf::as_path)
    }

    pub fn is_mounted(&self) -> bool {
        matches!(self, Self::Mounted(paths) if !paths.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDevice {
    pub id: StorageDeviceId,
    pub label: String,
    pub device_path: Option<PathBuf>,
    pub filesystem_type: String,
    pub size_bytes: u64,
    pub mount_state: StorageDeviceMountState,
    pub is_removable: bool,
    pub can_unmount: bool,
    pub can_eject: bool,
    pub can_power_off: bool,
}

impl StorageDevice {
    pub fn is_mounted(&self) -> bool {
        self.mount_state.is_mounted()
    }

    pub fn primary_mount_path(&self) -> Option<&Path> {
        self.mount_state.primary_mount_path()
    }
}

#[derive(Debug, Error)]
pub enum StorageDeviceError {
    #[error("UDisks2 error: {0}")]
    UDisks(#[from] udisks2::Error),
    #[error("D-Bus error: {0}")]
    Dbus(#[from] udisks2::zbus::Error),
    #[error("D-Bus object manager error: {0}")]
    Fdo(#[from] udisks2::zbus::fdo::Error),
    #[error("D-Bus object path error: {0}")]
    ObjectPath(#[from] udisks2::zbus::zvariant::Error),
    #[error("storage device action is unavailable: {0}")]
    ActionUnavailable(&'static str),
}

impl From<Infallible> for StorageDeviceError {
    fn from(value: Infallible) -> Self {
        match value {}
    }
}

pub async fn load_storage_devices() -> Result<Vec<StorageDevice>, StorageDeviceError> {
    let client = Client::new().await?;
    let managed_objects = client.object_manager().get_managed_objects().await?;
    let mut devices = Vec::new();

    for (object_path, interfaces) in managed_objects {
        if !has_interface(&interfaces, FILESYSTEM_INTERFACE) {
            continue;
        }
        let object = client.object(object_path.clone())?;
        if let Some(device) = storage_device_from_object(&client, object_path, object).await? {
            devices.push(device);
        }
    }

    devices.sort_by(|left, right| {
        left.label
            .to_lowercase()
            .cmp(&right.label.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(devices)
}

pub async fn mount_storage_device(id: StorageDeviceId) -> Result<PathBuf, StorageDeviceError> {
    let client = Client::new().await?;
    let object = client.object(id.as_str())?;
    let filesystem = object.filesystem().await?;
    let mount_path = filesystem.mount(standard_options(false)).await?;
    Ok(PathBuf::from(mount_path))
}

pub async fn unmount_storage_device(id: StorageDeviceId) -> Result<(), StorageDeviceError> {
    let client = Client::new().await?;
    let object = client.object(id.as_str())?;
    let filesystem = object.filesystem().await?;
    filesystem.unmount(standard_options(false)).await?;
    Ok(())
}

pub async fn eject_or_power_off_storage_device(
    id: StorageDeviceId,
) -> Result<(), StorageDeviceError> {
    let client = Client::new().await?;
    let object = client.object(id.as_str())?;
    let filesystem = object.filesystem().await?;
    let block = object.block().await?;
    let drive = client.drive_for_block(&block).await?;

    if !filesystem.mount_points().await?.is_empty() {
        match filesystem.unmount(standard_options(false)).await {
            Ok(()) | Err(udisks2::Error::NotMounted) => {}
            Err(error) => return Err(error.into()),
        }
    }

    if drive.can_power_off().await.unwrap_or(false) {
        drive.power_off(standard_options(false)).await?;
        return Ok(());
    }
    if drive.ejectable().await.unwrap_or(false) {
        drive.eject(standard_options(false)).await?;
        return Ok(());
    }

    Err(StorageDeviceError::ActionUnavailable(
        "device cannot be ejected or safely removed",
    ))
}

async fn storage_device_from_object(
    client: &Client,
    object_path: OwnedObjectPath,
    object: udisks2::Object,
) -> Result<Option<StorageDevice>, StorageDeviceError> {
    let block = object.block().await?;
    let visibility_hint = udisks_visibility_hint(
        block.hint_ignore().await.unwrap_or(false),
        block.hint_system().await.unwrap_or(false),
    );

    let filesystem = object.filesystem().await?;
    let mount_points = filesystem
        .mount_points()
        .await?
        .into_iter()
        .filter_map(path_from_udisks_bytes)
        .collect::<Vec<_>>();
    if !storage_device_is_visible(visibility_hint, &mount_points) {
        return Ok(None);
    }

    let drive = client.drive_for_block(&block).await.ok();
    let is_removable = if let Some(drive) = drive.as_ref() {
        drive.removable().await.unwrap_or(false)
            || drive.media_removable().await.unwrap_or(false)
            || matches!(
                drive.connection_bus().await.as_deref(),
                Ok("usb") | Ok("firewire")
            )
    } else {
        false
    };
    let device_path = match block
        .preferred_device()
        .await
        .ok()
        .and_then(path_from_udisks_bytes)
    {
        Some(path) => Some(path),
        None => block.device().await.ok().and_then(path_from_udisks_bytes),
    };
    let filesystem_type = block.id_type().await.unwrap_or_default();
    let size_bytes = match block.size().await {
        Ok(size) => size,
        Err(_) => filesystem.size().await.unwrap_or(0),
    };
    let (can_eject, can_power_off, drive_label) = if let Some(drive) = drive.as_ref() {
        (
            drive.ejectable().await.unwrap_or(false),
            drive.can_power_off().await.unwrap_or(false),
            Some(DriveLabelParts {
                vendor: drive.vendor().await.unwrap_or_default(),
                model: drive.model().await.unwrap_or_default(),
            }),
        )
    } else {
        (false, false, None)
    };
    let label = storage_device_label(
        block.hint_name().await.unwrap_or_default(),
        block.id_label().await.unwrap_or_default(),
        drive_label,
        device_path.as_deref(),
        &filesystem_type,
    );
    let mount_state = if mount_points.is_empty() {
        StorageDeviceMountState::Unmounted
    } else {
        StorageDeviceMountState::Mounted(mount_points)
    };

    Ok(Some(StorageDevice {
        id: StorageDeviceId::new(object_path.to_string()),
        label,
        device_path,
        filesystem_type,
        size_bytes,
        can_unmount: mount_state.is_mounted(),
        mount_state,
        is_removable,
        can_eject,
        can_power_off,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UdisksVisibilityHint {
    Ignored,
    System,
    User,
}

fn udisks_visibility_hint(hint_ignore: bool, hint_system: bool) -> UdisksVisibilityHint {
    if hint_ignore {
        UdisksVisibilityHint::Ignored
    } else if hint_system {
        UdisksVisibilityHint::System
    } else {
        UdisksVisibilityHint::User
    }
}

fn storage_device_is_visible(
    visibility_hint: UdisksVisibilityHint,
    mount_points: &[PathBuf],
) -> bool {
    match visibility_hint {
        UdisksVisibilityHint::Ignored => false,
        // Internal data volumes can be marked as System by UDisks; root mount
        // ownership, not the hint alone, decides whether the device is hidden.
        UdisksVisibilityHint::System | UdisksVisibilityHint::User => {
            !mount_points.iter().any(|path| path == Path::new("/"))
        }
    }
}

fn has_interface(
    interfaces: &HashMap<OwnedInterfaceName, HashMap<String, OwnedValue>>,
    expected: &str,
) -> bool {
    interfaces.keys().any(|name| name.as_str() == expected)
}

struct DriveLabelParts {
    vendor: String,
    model: String,
}

fn storage_device_label(
    hint_name: String,
    id_label: String,
    drive: Option<DriveLabelParts>,
    device_path: Option<&Path>,
    filesystem_type: &str,
) -> String {
    first_non_empty([
        hint_name,
        id_label,
        drive
            .map(|drive| [drive.vendor, drive.model].join(" "))
            .unwrap_or_default(),
        device_path
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        filesystem_type.to_owned(),
    ])
    .unwrap_or_else(|| "Storage Device".to_owned())
}

fn first_non_empty(values: impl IntoIterator<Item = String>) -> Option<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
}

fn path_from_udisks_bytes(bytes: Vec<u8>) -> Option<PathBuf> {
    let bytes = trim_nul_bytes(bytes);
    if bytes.is_empty() {
        None
    } else {
        Some(PathBuf::from(OsString::from_vec(bytes)))
    }
}

fn trim_nul_bytes(mut bytes: Vec<u8>) -> Vec<u8> {
    if let Some(index) = bytes.iter().position(|byte| *byte == 0) {
        bytes.truncate(index);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udisks_byte_paths_ignore_trailing_nul() {
        let path = path_from_udisks_bytes(b"/run/media/user/DISK\0ignored".to_vec());

        assert_eq!(path, Some(PathBuf::from("/run/media/user/DISK")));
    }

    #[test]
    fn label_prefers_hint_then_filesystem_label_then_drive_then_path() {
        let label = storage_device_label(
            "".to_owned(),
            "Photos".to_owned(),
            Some(DriveLabelParts {
                vendor: "Vendor".to_owned(),
                model: "Model".to_owned(),
            }),
            Some(Path::new("/dev/sdb1")),
            "vfat",
        );

        assert_eq!(label, "Photos");
    }

    #[test]
    fn label_falls_back_to_device_path() {
        let label = storage_device_label(
            String::new(),
            String::new(),
            None,
            Some(Path::new("/dev/sdb1")),
            "vfat",
        );

        assert_eq!(label, "sdb1");
    }

    #[test]
    fn visibility_keeps_unmounted_internal_data_filesystems() {
        assert!(storage_device_is_visible(UdisksVisibilityHint::System, &[]));
    }

    #[test]
    fn visibility_hides_root_filesystem() {
        assert!(!storage_device_is_visible(
            UdisksVisibilityHint::System,
            &[PathBuf::from("/")]
        ));
    }

    #[test]
    fn visibility_honors_udisks_ignore_hint() {
        assert!(!storage_device_is_visible(
            UdisksVisibilityHint::Ignored,
            &[]
        ));
    }
}
