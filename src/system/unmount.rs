use std::path::Path;
use thiserror::Error;

use crate::system::local::is_critical_mount;
use crate::udisks::Udisks;

/// Errors that can occur during unmount operations
#[derive(Error, Debug)]
pub enum UnmountError {
    #[error("Device is busy: {0}")]
    DeviceBusy(String),

    #[error("Permission denied: unmounting requires elevated privileges")]
    PermissionDenied,

    #[error("Mount point not found: {0}")]
    MountPointNotFound(String),

    #[error("Device is a critical system mount and cannot be unmounted")]
    CriticalSystemMount,

    #[error("Unmount failed: {0}")]
    UnmountFailed(String),
}

/// Unmount a local block device via UDisks2's `Filesystem.Unmount` D-Bus method.
///
/// UDisks2 itself performs the Polkit authorization prompt - no manual `pkexec`
/// subprocess is spawned here.
///
/// # Arguments
/// * `mount_point` - The mount point path to unmount (used for the critical-mount
///   safety check and error reporting).
/// * `unix_device` - The raw device path (e.g. "/dev/sdb1"), from `MountedDrive::device`.
/// * `uuid` - The filesystem UUID, if known (preferred for UDisks2 object lookup).
pub async fn unmount_drive(
    mount_point: &Path,
    unix_device: &str,
    uuid: Option<&str>,
) -> Result<(), UnmountError> {
    tracing::info!("Attempting to unmount: {}", mount_point.display());

    if !mount_point.exists() {
        return Err(UnmountError::MountPointNotFound(
            mount_point.display().to_string(),
        ));
    }

    if is_critical_mount(mount_point) {
        tracing::warn!(
            "Attempted to unmount critical system mount: {}",
            mount_point.display()
        );
        return Err(UnmountError::CriticalSystemMount);
    }

    let udisks = Udisks::new()
        .await
        .map_err(|e| UnmountError::UnmountFailed(format!("UDisks2 connection failed: {e}")))?;

    let object = udisks
        .find_block_object(unix_device, uuid)
        .await
        .map_err(|e| UnmountError::UnmountFailed(e.to_string()))?;

    udisks.unmount(&object).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("busy") || msg.contains("in use") || msg.contains("Busy") {
            UnmountError::DeviceBusy(mount_point.display().to_string())
        } else if msg.contains("NotAuthorized") || msg.contains("permission") {
            UnmountError::PermissionDenied
        } else {
            UnmountError::UnmountFailed(msg)
        }
    })?;

    tracing::info!("Successfully unmounted: {}", mount_point.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_critical_mount() {
        assert!(is_critical_mount(Path::new("/")));
        assert!(is_critical_mount(Path::new("/boot")));
        assert!(is_critical_mount(Path::new("/usr")));
        assert!(!is_critical_mount(Path::new("/mnt/data")));
        assert!(!is_critical_mount(Path::new("/run/media/user/disk")));
    }
}
