use anyhow::Result;
use gio::prelude::*;
use std::path::PathBuf;

/// Represents a local drive/partition, projected from `gio::Volume`/`gio::Mount`.
#[derive(Debug, Clone)]
pub struct MountedDrive {
    /// Device path (e.g. "/dev/sda1", "/dev/nvme0n1p1"), from
    /// `gio::Volume::identifier("unix-device")`.
    pub device: String,

    /// Display name (e.g. "500 GB Hard Drive", a filesystem label, etc.).
    pub name: String,

    /// Filesystem UUID, if known (`gio::Volume::identifier("uuid")`) - preferred for
    /// correlating with UDisks2 objects (see `crate::udisks`).
    pub uuid: Option<String>,

    /// Mount point path, if currently mounted.
    pub mount_point: Option<PathBuf>,

    /// Filesystem type (e.g. "ext4", "btrfs", "vfat"), if currently mounted.
    pub fs_type: String,

    /// Total size in bytes (if available).
    pub total_size: Option<u64>,

    /// Used space in bytes (if available).
    pub used_space: Option<u64>,

    /// Whether this drive is currently mounted.
    pub is_mounted: bool,

    /// Whether this drive is safe to unmount (not a system-critical mount point).
    pub is_safe_to_unmount: bool,
}

impl MountedDrive {
    /// Format size for display (e.g., "512 GB", "1.5 TB")
    pub fn format_size(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{} {}", size as u64, UNITS[unit_index])
        } else {
            format!("{:.1} {}", size, UNITS[unit_index])
        }
    }
}

/// List local drives/partitions, sourced from `gio::VolumeMonitor` rather than
/// parsing `/proc/mounts` directly.
///
/// Includes both mounted and unmounted local volumes (those with a
/// `unix-device` identifier), so the UI can offer a "Dock" action on
/// not-yet-mounted removable media in addition to "Undock" on mounted ones.
/// Network volumes (which have no `unix-device` identifier) are excluded -
/// those are handled by `crate::network::discover`/`crate::network::mount`.
pub async fn list_local_drives() -> Result<Vec<MountedDrive>> {
    tracing::debug!("Listing local drives via gio::VolumeMonitor...");

    let monitor = gio::VolumeMonitor::get();
    let mut drives = Vec::new();

    for volume in monitor.volumes() {
        let Some(device) = volume.identifier("unix-device") else {
            // No unix-device identifier means this isn't a local block device
            // (e.g. a network volume) - skip it.
            continue;
        };

        let name = volume.name().to_string();
        let uuid = volume
            .identifier("uuid")
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());

        let mut drive = MountedDrive {
            device: device.to_string(),
            name,
            uuid,
            mount_point: None,
            fs_type: String::new(),
            total_size: None,
            used_space: None,
            is_mounted: false,
            is_safe_to_unmount: false,
        };

        if let Some(mount) = volume.get_mount() {
            let root = mount.root();
            if let Some(path) = root.path() {
                drive.fs_type = query_fs_type(&root).await;
                let (total, used) = query_fs_stats(&root).await;
                drive.total_size = total;
                drive.used_space = used;
                drive.is_safe_to_unmount = !is_critical_mount(&path);
                drive.is_mounted = true;
                drive.mount_point = Some(path);
            }
        }

        drives.push(drive);
    }

    tracing::info!("Found {} local drives", drives.len());
    Ok(drives)
}

/// Query the filesystem type for a mounted root, via GIO's `filesystem::type` attribute.
async fn query_fs_type(root: &gio::File) -> String {
    root.query_filesystem_info_future(gio::FILE_ATTRIBUTE_FILESYSTEM_TYPE, glib::Priority::DEFAULT)
        .await
        .ok()
        .and_then(|info| info.attribute_string(gio::FILE_ATTRIBUTE_FILESYSTEM_TYPE))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Query total/used space for a mounted root, via GIO's `filesystem::size`/`filesystem::free`.
async fn query_fs_stats(root: &gio::File) -> (Option<u64>, Option<u64>) {
    let attrs = format!(
        "{},{}",
        gio::FILE_ATTRIBUTE_FILESYSTEM_SIZE,
        gio::FILE_ATTRIBUTE_FILESYSTEM_FREE
    );
    match root
        .query_filesystem_info_future(&attrs, glib::Priority::DEFAULT)
        .await
    {
        Ok(info) => {
            let total = info
                .has_attribute(gio::FILE_ATTRIBUTE_FILESYSTEM_SIZE)
                .then(|| info.attribute_uint64(gio::FILE_ATTRIBUTE_FILESYSTEM_SIZE));
            let free = info
                .has_attribute(gio::FILE_ATTRIBUTE_FILESYSTEM_FREE)
                .then(|| info.attribute_uint64(gio::FILE_ATTRIBUTE_FILESYSTEM_FREE));
            let used = match (total, free) {
                (Some(total), Some(free)) => Some(total.saturating_sub(free)),
                _ => None,
            };
            (total, used)
        }
        Err(_) => (None, None),
    }
}

/// Check if a mount point is a critical system mount that must never be unmounted.
///
/// Preserved unchanged in behavior from the pre-pivot implementation - used both
/// here (to gate the "Dock/Undock" UI affordance) and in `crate::system::unmount`.
pub fn is_critical_mount(mount_point: &std::path::Path) -> bool {
    let critical_paths = ["/", "/boot", "/boot/efi", "/usr", "/var", "/home"];
    let mount_str = mount_point.to_string_lossy();
    critical_paths.iter().any(|&critical| mount_str == critical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(MountedDrive::format_size(512), "512 B");
        assert_eq!(MountedDrive::format_size(1024), "1.0 KB");
        assert_eq!(MountedDrive::format_size(1536), "1.5 KB");
        assert_eq!(MountedDrive::format_size(1024 * 1024), "1.0 MB");
        assert_eq!(MountedDrive::format_size(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_is_critical_mount() {
        assert!(is_critical_mount(std::path::Path::new("/")));
        assert!(is_critical_mount(std::path::Path::new("/boot")));
        assert!(is_critical_mount(std::path::Path::new("/usr")));
        assert!(!is_critical_mount(std::path::Path::new("/mnt/data")));
        assert!(!is_critical_mount(std::path::Path::new(
            "/run/media/user/disk"
        )));
    }
}
