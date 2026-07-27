use std::path::PathBuf;

use gio::glib::MainContext;
use gio::prelude::{DriveExt, FileExt, MountExt, VolumeExt, VolumeMonitorExt};
use thiserror::Error;

use crate::gvfs_paths::{default_gvfs_fuse_root, resolve_gvfs_mount_path, GvfsMountPathError};
use crate::storage_devices::{
    StorageDevice, StorageDeviceAccess, StorageDeviceId, StorageDeviceMountState,
    StorageDeviceRemoval,
};

#[derive(Debug, Error)]
pub enum GvfsDeviceError {
    #[error("could not run GVfs operation on its GLib context: {0}")]
    Context(#[source] gio::glib::BoolError),
    #[error("GVfs volume {identity:?} is no longer available")]
    VolumeUnavailable { identity: String },
    #[error("GVfs device action is unavailable: {0}")]
    ActionUnavailable(&'static str),
    #[error("GVfs operation for {identity:?} failed: {source}")]
    Gio {
        identity: String,
        #[source]
        source: gio::glib::Error,
    },
    #[error("GVfs operation for {identity:?} did not produce a mount: {message}")]
    OperationIncomplete {
        identity: String,
        message: &'static str,
    },
    #[error("GVfs mount path for {identity:?} failed: {source}")]
    MountPath {
        identity: String,
        #[source]
        source: GvfsMountPathError,
    },
    #[error("GVfs blocking task failed: {0}")]
    BlockingTask(#[from] tokio::task::JoinError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GvfsVolumeFacts {
    identity: String,
    label: String,
    class: Option<String>,
    unix_device: Option<String>,
}

pub(super) async fn load_gvfs_storage_devices() -> Result<Vec<StorageDevice>, GvfsDeviceError> {
    tokio::task::spawn_blocking(load_gvfs_storage_devices_blocking).await?
}

fn load_gvfs_storage_devices_blocking() -> Result<Vec<StorageDevice>, GvfsDeviceError> {
    run_on_gio_context(|context| {
        let monitor = gio::VolumeMonitor::get();
        monitor
            .volumes()
            .into_iter()
            .filter_map(|volume| {
                let facts = volume_facts(&volume)?;
                if !is_portable_volume(facts.class.as_deref(), facts.unix_device.as_deref()) {
                    return None;
                }
                Some(storage_device_from_volume(&volume, facts))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|mut devices| {
                devices.sort_by(|left, right| {
                    left.label
                        .to_lowercase()
                        .cmp(&right.label.to_lowercase())
                        .then_with(|| left.id.cmp(&right.id))
                });
                let _ = context;
                devices
            })
    })
}

pub(super) async fn mount_gvfs_storage_device(
    identity: String,
) -> Result<PathBuf, GvfsDeviceError> {
    tokio::task::spawn_blocking(move || {
        run_on_gio_context(|context| {
            let volume = find_volume(&identity)?;
            if let Some(mount) = volume.get_mount() {
                return mounted_path(&identity, &mount);
            }
            if !volume.can_mount() {
                return Err(GvfsDeviceError::ActionUnavailable(
                    "portable device cannot be mounted",
                ));
            }
            context
                .block_on(
                    volume.mount_future(gio::MountMountFlags::NONE, None::<&gio::MountOperation>),
                )
                .map_err(|error| gio_error(identity.clone(), error))?;
            let mount = volume
                .get_mount()
                .ok_or(GvfsDeviceError::OperationIncomplete {
                    identity: identity.clone(),
                    message: "mount completed without a visible GMount",
                })?;
            mounted_path(&identity, &mount)
        })
    })
    .await?
}

pub(super) async fn unmount_gvfs_storage_device(identity: String) -> Result<(), GvfsDeviceError> {
    tokio::task::spawn_blocking(move || {
        run_on_gio_context(|context| {
            let volume = find_volume(&identity)?;
            let mount = volume
                .get_mount()
                .ok_or(GvfsDeviceError::ActionUnavailable(
                    "portable device is not mounted",
                ))?;
            if !mount.can_unmount() {
                return Err(GvfsDeviceError::ActionUnavailable(
                    "portable device cannot be unmounted",
                ));
            }
            context
                .block_on(mount.unmount_with_operation_future(
                    gio::MountUnmountFlags::NONE,
                    None::<&gio::MountOperation>,
                ))
                .map_err(|error| gio_error(identity, error))
        })
    })
    .await?
}

pub(super) async fn remove_gvfs_storage_device(identity: String) -> Result<(), GvfsDeviceError> {
    tokio::task::spawn_blocking(move || {
        run_on_gio_context(|context| {
            let volume = find_volume(&identity)?;
            if let Some(mount) = volume.get_mount().filter(|mount| mount.can_eject()) {
                return context
                    .block_on(mount.eject_with_operation_future(
                        gio::MountUnmountFlags::NONE,
                        None::<&gio::MountOperation>,
                    ))
                    .map_err(|error| gio_error(identity, error));
            }
            if volume.can_eject() {
                return context
                    .block_on(volume.eject_with_operation_future(
                        gio::MountUnmountFlags::NONE,
                        None::<&gio::MountOperation>,
                    ))
                    .map_err(|error| gio_error(identity, error));
            }
            if let Some(drive) = volume.drive() {
                if drive.can_eject() {
                    return context
                        .block_on(drive.eject_with_operation_future(
                            gio::MountUnmountFlags::NONE,
                            None::<&gio::MountOperation>,
                        ))
                        .map_err(|error| gio_error(identity, error));
                }
                if drive.can_stop() {
                    return context
                        .block_on(drive.stop_future(
                            gio::MountUnmountFlags::NONE,
                            None::<&gio::MountOperation>,
                        ))
                        .map_err(|error| gio_error(identity, error));
                }
            }
            Err(GvfsDeviceError::ActionUnavailable(
                "portable device cannot be ejected or safely removed",
            ))
        })
    })
    .await?
}

fn run_on_gio_context<T>(
    operation: impl FnOnce(&MainContext) -> Result<T, GvfsDeviceError>,
) -> Result<T, GvfsDeviceError> {
    let context = MainContext::new();
    context
        .with_thread_default(|| operation(&context))
        .map_err(GvfsDeviceError::Context)?
}

fn find_volume(identity: &str) -> Result<gio::Volume, GvfsDeviceError> {
    let monitor = gio::VolumeMonitor::get();
    monitor
        .volumes()
        .into_iter()
        .find(|volume| volume_facts(volume).is_some_and(|facts| facts.identity == identity))
        .ok_or_else(|| GvfsDeviceError::VolumeUnavailable {
            identity: identity.to_owned(),
        })
}

fn storage_device_from_volume(
    volume: &gio::Volume,
    facts: GvfsVolumeFacts,
) -> Result<StorageDevice, GvfsDeviceError> {
    let mount = volume.get_mount();
    let mount_state = match mount.as_ref() {
        Some(mount) => {
            StorageDeviceMountState::Mounted(vec![mounted_path(&facts.identity, mount)?])
        }
        None => StorageDeviceMountState::Unmounted,
    };
    let drive = volume.drive();
    let can_eject = mount.as_ref().is_some_and(MountExt::can_eject)
        || volume.can_eject()
        || drive.as_ref().is_some_and(DriveExt::can_eject);
    let can_stop = drive.as_ref().is_some_and(DriveExt::can_stop);
    let removal = if can_eject {
        Some(StorageDeviceRemoval::Eject)
    } else if can_stop {
        Some(StorageDeviceRemoval::SafelyRemove)
    } else {
        None
    };
    Ok(StorageDevice {
        id: StorageDeviceId::GvfsVolume(facts.identity),
        label: facts.label,
        device_path: None,
        filesystem_type: "GVfs".to_owned(),
        size_bytes: 0,
        mount_state,
        access: StorageDeviceAccess::RemoteFilesystem,
        is_removable: true,
        can_mount: volume.can_mount(),
        can_unmount: mount.as_ref().is_some_and(MountExt::can_unmount),
        can_eject,
        can_power_off: can_stop,
        removal,
    })
}

fn mounted_path(identity: &str, mount: &gio::Mount) -> Result<PathBuf, GvfsDeviceError> {
    let root = mount.root();
    let default_location = mount.default_location();
    if let Some(path) = default_location.path().or_else(|| root.path()) {
        return Ok(path);
    }
    resolve_gvfs_mount_path(
        root.uri().as_str(),
        default_location.uri().as_str(),
        &default_gvfs_fuse_root(),
    )
    .map_err(|source| GvfsDeviceError::MountPath {
        identity: identity.to_owned(),
        source,
    })
}

fn volume_facts(volume: &gio::Volume) -> Option<GvfsVolumeFacts> {
    let class = volume.identifier("class").map(|value| value.to_string());
    let unix_device = volume
        .identifier("unix-device")
        .map(|value| value.to_string());
    let activation_root_uri = volume.activation_root().map(|file| file.uri().to_string());
    let uuid = volume.uuid().map(|value| value.to_string());
    let identifiers = volume
        .enumerate_identifiers()
        .into_iter()
        .filter_map(|kind| {
            volume
                .identifier(&kind)
                .map(|value| (kind.to_string(), value.to_string()))
        })
        .collect::<Vec<_>>();
    let identity = gvfs_volume_identity(
        activation_root_uri.as_deref(),
        volume.get_mount().as_ref(),
        uuid.as_deref(),
        identifiers,
    )?;
    Some(GvfsVolumeFacts {
        identity,
        label: volume.name().to_string(),
        class,
        unix_device,
    })
}

fn gvfs_volume_identity(
    activation_root_uri: Option<&str>,
    mount: Option<&gio::Mount>,
    uuid: Option<&str>,
    identifiers: Vec<(String, String)>,
) -> Option<String> {
    activation_root_uri
        .map(|uri| format!("activation:{uri}"))
        .or_else(|| uuid.map(|uuid| format!("uuid:{uuid}")))
        .or_else(|| {
            let mut identifiers = identifiers
                .into_iter()
                .filter(|(kind, _)| kind != "class" && kind != "label")
                .map(|(kind, value)| format!("{kind}={value}"))
                .collect::<Vec<_>>();
            identifiers.sort();
            (!identifiers.is_empty()).then(|| format!("identifiers:{}", identifiers.join("\0")))
        })
        .or_else(|| mount.map(|mount| format!("mount:{}", mount.root().uri())))
}

fn is_portable_volume(class: Option<&str>, unix_device: Option<&str>) -> bool {
    class == Some("device") && unix_device.is_none()
}

fn gio_error(identity: String, source: gio::glib::Error) -> GvfsDeviceError {
    GvfsDeviceError::Gio { identity, source }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gio_operation_runs_with_owned_context_as_thread_default() {
        run_on_gio_context(|context| {
            assert_eq!(MainContext::thread_default().as_ref(), Some(context));
            Ok(())
        })
        .expect("GIO context");
    }

    #[test]
    fn portable_filter_keeps_non_native_device_classes() {
        assert!(is_portable_volume(Some("device"), None));
        assert!(!is_portable_volume(Some("device"), Some("/dev/sdb1")));
        assert!(!is_portable_volume(Some("network"), None));
        assert!(!is_portable_volume(None, None));
    }

    #[test]
    fn class_and_label_are_not_unique_fallback_identities() {
        assert_eq!(
            gvfs_volume_identity(
                None,
                None,
                None,
                vec![
                    ("class".to_owned(), "device".to_owned()),
                    ("label".to_owned(), "Phone".to_owned()),
                ],
            ),
            None
        );
    }
    #[test]
    fn activation_identities_keep_portable_protocols_distinct() {
        let mtp = gvfs_volume_identity(Some("mtp://[usb:001,002]/"), None, None, Vec::new());
        let gphoto = gvfs_volume_identity(Some("gphoto2://[usb:001,002]/"), None, None, Vec::new());
        let afc = gvfs_volume_identity(Some("afc://phone/"), None, None, Vec::new());

        assert_eq!(mtp.as_deref(), Some("activation:mtp://[usb:001,002]/"));
        assert_eq!(
            gphoto.as_deref(),
            Some("activation:gphoto2://[usb:001,002]/")
        );
        assert_eq!(afc.as_deref(), Some("activation:afc://phone/"));
        assert_ne!(mtp, gphoto);
    }

    #[test]
    fn activation_identity_does_not_depend_on_display_name() {
        let first = GvfsVolumeFacts {
            identity: "activation:mtp://phone/".to_owned(),
            label: "Phone".to_owned(),
            class: Some("device".to_owned()),
            unix_device: None,
        };
        let second = GvfsVolumeFacts {
            label: "Renamed Phone".to_owned(),
            ..first.clone()
        };

        assert_eq!(first.identity, second.identity);
        assert_ne!(first.label, second.label);
    }
}
