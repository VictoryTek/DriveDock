use anyhow::Result;
use std::path::PathBuf;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::OsStrExt;

/// Represents a mounted drive or partition
#[derive(Debug, Clone)]
pub struct MountedDrive {
    /// Device name (e.g., "sda1", "nvme0n1p1", "dm-0")
    pub device: String,
    
    /// Mount point path (e.g., "/", "/home", "/mnt/data")
    pub mount_point: PathBuf,
    
    /// Filesystem type (e.g., "ext4", "btrfs", "xfs", "vfat")
    pub fs_type: String,
    
    /// Total size in bytes (if available)
    pub total_size: Option<u64>,
    
    /// Used space in bytes (if available)
    pub used_space: Option<u64>,
    
    /// Whether this drive is safe to unmount
    /// (not a system drive, not in use by critical processes)
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

/// List all currently mounted drives
/// 
/// This function will scan the system for mounted filesystems by:
/// 1. Reading /proc/mounts or using mount command
/// 2. Filtering out pseudo filesystems (proc, sysfs, tmpfs, devtmpfs, etc.)
/// 3. Determining safety for unmounting
/// 
/// Now reads /proc/mounts and filters pseudo-filesystems.
/// TODO: Add statvfs for size/usage stats
pub async fn list_mounted_drives() -> Result<Vec<MountedDrive>> {
    tracing::debug!("Listing mounted drives...");

    let mut drives = Vec::new();

    // Read /proc/mounts
    let file = File::open("/proc/mounts")?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();

        // /proc/mounts format: device mountpoint fstype options dump pass
        if parts.len() < 3 {
            continue;
        }

        let device = parts[0];
        let mount_point = parts[1];
        let fs_type = parts[2];

        // Filter out pseudo-filesystems
        if is_pseudo_filesystem(fs_type) {
            continue;
        }

        // Filter out non-device mounts
        if !is_real_device(device) {
            continue;
        }

        // Unescape mount point (\\040 = space, etc.)
        let mount_point = unescape_mount_point(mount_point);
        let mount_path = PathBuf::from(&mount_point);

        // Get filesystem statistics
        let (total_size, used_space) = get_filesystem_stats(&mount_path);

        // Check if safe to unmount
        let is_safe = is_safe_to_unmount(&mount_path).await;

        drives.push(MountedDrive {
            device: device.to_string(),
            mount_point: mount_path,
            fs_type: fs_type.to_string(),
            total_size,
            used_space,
            is_safe_to_unmount: is_safe,
        });
    }

    tracing::info!("Found {} mounted drives", drives.len());
    Ok(drives)
}

/// Check if filesystem type is pseudo (kernel-managed, not real storage)
fn is_pseudo_filesystem(fs_type: &str) -> bool {
    matches!(
        fs_type,
        "proc" | "sysfs" | "devtmpfs" | "devpts" | "tmpfs" |
        "cgroup" | "cgroup2" | "pstore" | "bpf" | "tracefs" |
        "debugfs" | "securityfs" | "selinuxfs" | "systemd" |
        "mqueue" | "hugetlbfs" | "autofs" | "configfs" |
        "fusectl" | "fuse.portal" | "efivarfs" | "overlay"
    )
}

/// Get filesystem statistics (total size, used space) using statvfs
fn get_filesystem_stats(path: &PathBuf) -> (Option<u64>, Option<u64>) {
    use std::mem;
    
    // Convert path to C string
    let path_cstr = match std::ffi::CString::new(path.as_os_str().as_bytes()) {
        Ok(s) => s,
        Err(_) => return (None, None),
    };
    
    // Call statvfs
    unsafe {
        let mut stat: libc::statvfs = mem::zeroed();
        if libc::statvfs(path_cstr.as_ptr(), &mut stat) == 0 {
            let block_size = stat.f_frsize as u64;
            let total_blocks = stat.f_blocks as u64;
            let free_blocks = stat.f_bfree as u64;
            
            let total_size = total_blocks * block_size;
            let used_size = (total_blocks - free_blocks) * block_size;
            
            (Some(total_size), Some(used_size))
        } else {
            (None, None)
        }
    }
}

/// Check if device name represents real storage
fn is_real_device(device: &str) -> bool {
    device.starts_with("/dev/sd") ||     // SCSI/SATA
    device.starts_with("/dev/nvme") ||   // NVMe
    device.starts_with("/dev/vd") ||     // virtio
    device.starts_with("/dev/hd") ||     // IDE (rare)
    device.starts_with("/dev/mmcblk") || // SD cards
    device.starts_with("/dev/loop") ||   // loop devices
    device.starts_with("/dev/dm-") ||    // device mapper (LVM, LUKS)
    device.starts_with("/dev/md") ||     // software RAID
    device.starts_with("UUID=") ||
    device.starts_with("LABEL=") ||
    device.starts_with("PARTUUID=")
}

/// Unescape mount point from /proc/mounts (\\040 = space, etc.)
fn unescape_mount_point(escaped: &str) -> String {
    let mut result = String::with_capacity(escaped.len());
    let mut chars = escaped.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let mut octal = String::new();
            for _ in 0..3 {
                if let Some(digit) = chars.next() {
                    octal.push(digit);
                }
            }
            if let Ok(code) = u8::from_str_radix(&octal, 8) {
                result.push(code as char);
            } else {
                result.push('\\');
                result.push_str(&octal);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Check if a device is safe to unmount
/// 
/// A device is considered safe to unmount if:
/// - It's not a system-critical mount (/, /boot, /usr, /var, /home)
/// - It's not currently being accessed by any process
/// - It's not a swap partition
/// - It's not locked by the system
/// 
/// # Arguments
/// * `mount_point` - The mount point path to check
/// 
/// # Future Implementation
/// 
/// Will use:
/// - lsof or /proc/*/fd to check file handles
/// - /proc/swaps to detect swap partitions
/// - Hardcoded list of system-critical paths
/// - fuser command as fallback
async fn is_safe_to_unmount(mount_point: &PathBuf) -> bool {
    // System-critical mount points that should never be unmounted
    let critical_paths = ["/", "/boot", "/boot/efi", "/usr", "/var", "/home", "/tmp"];
    
    let mount_str = mount_point.to_string_lossy();
    
    // Don't allow unmounting critical system paths
    if critical_paths.iter().any(|p| mount_str.starts_with(p)) {
        return false;
    }

    // TODO: Check if mount point is in use
    // TODO: Check if mount point is a swap partition
    
    true
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
}
