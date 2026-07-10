//! Network share discovery via GVfs (`network:///`).
//!
//! Renamed from `gvfs.rs` (see the privileged-mount spec, §3.3). Discovery is
//! unchanged from before: piggyback on `gvfsd-network`'s own server/share browsing
//! (Avahi/SMB-browsing/WS-Discovery) via `gio::File::for_uri("network:///")`.
//!
//! What changed: mounting/unmounting no longer happens here (moved to
//! `crate::network::mount`, which performs real kernel `cifs`/`nfs` mounts via the
//! privileged helper). `NetworkShare.mounted` is therefore no longer sourced from
//! `gio::VolumeMonitor::mounts()` (that only reflects GVfs/FUSE mounts, which we no
//! longer create) - instead it is computed by parsing `/proc/mounts` for `cifs`/
//! `cifs2`/`nfs`/`nfs4` fstype rows matching the share's `//host/share` (SMB) or
//! `host:/export` (NFS) source string.

use anyhow::Result;
use gio::prelude::*;

/// A network share, discovered via GVfs (`network:///`).
#[derive(Debug, Clone)]
pub struct NetworkShare {
    /// Human-readable name (e.g. "media on fileserver").
    pub display_name: String,

    /// The GVfs URI (e.g. "smb://fileserver/media", "nfs://fileserver/export/media").
    pub uri: String,

    /// Protocol label parsed from the URI scheme (e.g. "SMB", "NFS", "DAV").
    pub protocol: String,

    /// Whether this share currently has a real kernel `cifs`/`nfs` mount (per
    /// `/proc/mounts`) - not a GVfs/FUSE mount.
    pub mounted: bool,
}

impl NetworkShare {
    fn protocol_from_uri(uri: &str) -> String {
        let scheme = uri.split(':').next().unwrap_or("").to_ascii_uppercase();
        match scheme.as_str() {
            "SMB" | "CIFS" => "SMB".to_string(),
            "NFS" => "NFS".to_string(),
            "DAV" | "DAVS" => "WebDAV".to_string(),
            "FTP" | "FTPS" | "SFTP" => "FTP".to_string(),
            other if !other.is_empty() => other.to_string(),
            _ => "Network".to_string(),
        }
    }
}

/// Discover network shares by enumerating GVfs's virtual `network:///` root, which
/// is populated by the `gvfsd-network` backend (itself doing the Avahi/SMB-browsing/
/// WS-Discovery work DriveDock does not reimplement). Each share's `mounted` flag is
/// then computed against a live `/proc/mounts` snapshot.
pub async fn scan_network_shares() -> Result<Vec<NetworkShare>> {
    let mut shares: Vec<NetworkShare> = match enumerate_network_root().await {
        Ok(discovered) => discovered,
        Err(e) => {
            // gvfsd-network may not be running/installed - not fatal, just log.
            tracing::warn!("Failed to enumerate network:/// (is gvfs-daemons running?): {e}");
            Vec::new()
        }
    };

    if let Ok(proc_mounts) = std::fs::read_to_string("/proc/mounts") {
        for share in &mut shares {
            share.mounted = is_network_mount_active(&proc_mounts, &share.uri);
        }
    } else {
        tracing::warn!("Failed to read /proc/mounts; assuming all shares are unmounted");
    }

    tracing::info!("Found {} network shares", shares.len());
    Ok(shares)
}

/// Enumerate the top-level of GVfs's virtual `network:///` root (discovered servers),
/// then one level deeper for each server (its shares).
async fn enumerate_network_root() -> Result<Vec<NetworkShare>> {
    let mut shares = Vec::new();
    let network_root = gio::File::for_uri("network:///");

    let servers = enumerate_children(&network_root).await?;
    for server_info in servers {
        let Some(target_uri) = server_info.attribute_string("standard::target-uri") else {
            continue;
        };
        let server_file = gio::File::for_uri(&target_uri);

        // One level deeper: the shares exposed by this server.
        match enumerate_children(&server_file).await {
            Ok(children) => {
                for info in children {
                    let uri = info
                        .attribute_string("standard::target-uri")
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| target_uri.to_string());
                    shares.push(NetworkShare {
                        display_name: info.display_name().to_string(),
                        protocol: NetworkShare::protocol_from_uri(&uri),
                        uri,
                        mounted: false,
                    });
                }
            }
            Err(e) => {
                tracing::debug!("Could not enumerate shares for server {target_uri}: {e}");
            }
        }
    }

    Ok(shares)
}

/// Enumerate all children of a `gio::File` directory-like location.
async fn enumerate_children(dir: &gio::File) -> Result<Vec<gio::FileInfo>> {
    let enumerator = dir
        .enumerate_children_future(
            "standard::*",
            gio::FileQueryInfoFlags::NONE,
            glib::Priority::DEFAULT,
        )
        .await?;

    let mut infos = Vec::new();
    loop {
        let batch = enumerator
            .next_files_future(10, glib::Priority::DEFAULT)
            .await?;
        if batch.is_empty() {
            break;
        }
        infos.extend(batch);
    }
    Ok(infos)
}

// Kept for interface stability with the pre-privileged-mount design (unchanged per
// the Phase 1 spec §3.3) even though `scan_network_shares` no longer merges in
// `gio::VolumeMonitor::mounts()` - retained as a small, independently-tested pure
// helper in case discovery needs to filter a raw URI list again in the future.
#[allow(dead_code)]
pub(crate) fn is_network_uri(uri: &str) -> bool {
    const NETWORK_SCHEMES: &[&str] = &["smb", "cifs", "nfs", "dav", "davs", "ftp", "ftps", "sftp"];
    uri.split(':')
        .next()
        .map(|scheme| NETWORK_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Convert a GVfs share URI (e.g. `smb://host/share`, `nfs://host/export/path`) into
/// the `mount.cifs`/`mount.nfs` "source" string kernel mounts use in `/proc/mounts`
/// (`//host/share` for CIFS, `host:/export/path` for NFS). Returns `None` for
/// unsupported/unrecognized schemes (e.g. `dav://`, `ftp://` - those never get a
/// kernel mount under this design).
pub(crate) fn mount_source_from_uri(uri: &str) -> Option<String> {
    let (scheme, rest) = uri.split_once("://")?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host.is_empty() {
        return None;
    }
    match scheme.to_ascii_lowercase().as_str() {
        "smb" | "cifs" => Some(format!("//{host}/{path}")),
        "nfs" => Some(format!("{host}:/{path}")),
        _ => None,
    }
}

/// Whether `uri` currently has a real kernel `cifs`/`cifs2`/`nfs`/`nfs4` mount, per
/// a `/proc/mounts` snapshot.
pub(crate) fn is_network_mount_active(proc_mounts: &str, uri: &str) -> bool {
    const NETWORK_FS_TYPES: &[&str] = &["cifs", "cifs2", "nfs", "nfs4"];
    let Some(source) = mount_source_from_uri(uri) else {
        return false;
    };
    proc_mounts.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let Some(mount_source) = fields.next() else {
            return false;
        };
        let _mount_dir = fields.next();
        let Some(fs_type) = fields.next() else {
            return false;
        };
        NETWORK_FS_TYPES.contains(&fs_type) && mount_source == source
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_from_uri() {
        assert_eq!(NetworkShare::protocol_from_uri("smb://host/share"), "SMB");
        assert_eq!(NetworkShare::protocol_from_uri("nfs://host/export"), "NFS");
        assert_eq!(NetworkShare::protocol_from_uri("dav://host/share"), "WebDAV");
    }

    #[test]
    fn test_is_network_uri() {
        assert!(is_network_uri("smb://host/share"));
        assert!(is_network_uri("nfs://host/export"));
        assert!(!is_network_uri("file:///mnt/data"));
    }

    #[test]
    fn test_mount_source_from_uri_smb() {
        assert_eq!(
            mount_source_from_uri("smb://fileserver/media"),
            Some("//fileserver/media".to_string())
        );
    }

    #[test]
    fn test_mount_source_from_uri_nfs() {
        assert_eq!(
            mount_source_from_uri("nfs://fileserver/export/media"),
            Some("fileserver:/export/media".to_string())
        );
    }

    #[test]
    fn test_mount_source_from_uri_unsupported_scheme() {
        assert_eq!(mount_source_from_uri("dav://host/share"), None);
    }

    #[test]
    fn test_is_network_mount_active() {
        let proc_mounts = "//fileserver/media /mnt/media-fileserver cifs rw,relatime 0 0\n\
             fileserver:/export/media /mnt/nfs-media nfs4 rw,relatime 0 0\n\
             /dev/sda1 / ext4 rw,relatime 0 0\n";
        assert!(is_network_mount_active(
            proc_mounts,
            "smb://fileserver/media"
        ));
        assert!(is_network_mount_active(
            proc_mounts,
            "nfs://fileserver/export/media"
        ));
        assert!(!is_network_mount_active(proc_mounts, "smb://other/share"));
    }
}
