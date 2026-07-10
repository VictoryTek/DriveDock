use anyhow::{anyhow, Result};
use gio::prelude::*;

/// A network share, discovered via GVfs (`network:///`) or already mounted
/// (surfaced via `gio::VolumeMonitor`).
#[derive(Debug, Clone)]
pub struct NetworkShare {
    /// Human-readable name (e.g. "media on fileserver").
    pub display_name: String,

    /// The GVfs URI (e.g. "smb://fileserver/media", "nfs://fileserver/export/media").
    pub uri: String,

    /// Protocol label parsed from the URI scheme (e.g. "SMB", "NFS", "DAV").
    pub protocol: String,

    /// Whether this share is currently mounted.
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

/// Discover network shares.
///
/// Combines two sources, per spec:
/// - Already-mounted network `gio::Mount`s (from the shared `VolumeMonitor` table -
///   these may have been mounted by DriveDock, Nautilus, or any other GVfs client).
/// - Not-yet-mounted shares discovered by enumerating GVfs's virtual `network:///`
///   root, which is populated by the `gvfsd-network` backend (itself doing the
///   Avahi/SMB-browsing/WS-Discovery work DriveDock no longer reimplements).
pub async fn scan_network_shares() -> Result<Vec<NetworkShare>> {
    let mut shares: Vec<NetworkShare> = Vec::new();

    // Already-mounted network shares.
    let monitor = gio::VolumeMonitor::get();
    for mount in monitor.mounts() {
        let uri = mount.root().uri().to_string();
        if !is_network_uri(&uri) {
            continue;
        }
        shares.push(NetworkShare {
            display_name: mount.name().to_string(),
            protocol: NetworkShare::protocol_from_uri(&uri),
            uri,
            mounted: true,
        });
    }

    // Discover not-yet-mounted shares via the GVfs network:// root.
    match enumerate_network_root().await {
        Ok(discovered) => {
            for share in discovered {
                if shares.iter().any(|s| s.uri == share.uri) {
                    continue;
                }
                shares.push(share);
            }
        }
        Err(e) => {
            // gvfsd-network may not be running/installed - not fatal, just log.
            tracing::warn!("Failed to enumerate network:/// (is gvfs-daemons running?): {e}");
        }
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

fn is_network_uri(uri: &str) -> bool {
    const NETWORK_SCHEMES: &[&str] = &["smb", "cifs", "nfs", "dav", "davs", "ftp", "ftps", "sftp"];
    uri.split(':')
        .next()
        .map(|scheme| NETWORK_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// A minimal `gio::MountOperation` that answers GVfs's `ask-password` signal with a
/// GTK/libadwaita password prompt (an `AdwAlertDialog` with a `PasswordEntryRow`).
///
/// This is deliberately minimal (single password field, optional username/domain
/// pre-filled from the signal) - not a full credential manager, per the narrowed scope.
pub fn interactive_mount_operation(
    parent: Option<&gtk::Window>,
) -> gio::MountOperation {
    let operation = gio::MountOperation::new();
    let parent = parent.cloned();

    operation.connect_ask_password(move |op, message, default_user, default_domain, flags| {
        use libadwaita as adw;
        use adw::prelude::*;

        let op = op.clone();

        let dialog = adw::AlertDialog::builder()
            .heading("Authentication Required")
            .body(message)
            .build();

        let content_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .build();

        let username_row = adw::EntryRow::builder().title("Username").build();
        if !default_user.is_empty() {
            username_row.set_text(default_user);
        }
        if flags.contains(gio::AskPasswordFlags::NEED_USERNAME) {
            content_box.append(&username_row);
        }

        let domain_row = adw::EntryRow::builder().title("Domain").build();
        if !default_domain.is_empty() {
            domain_row.set_text(default_domain);
        }
        if flags.contains(gio::AskPasswordFlags::NEED_DOMAIN) {
            content_box.append(&domain_row);
        }

        let password_row = adw::PasswordEntryRow::builder().title("Password").build();
        if flags.contains(gio::AskPasswordFlags::NEED_PASSWORD) {
            content_box.append(&password_row);
        }

        dialog.set_extra_child(Some(&content_box));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("connect", "Connect");
        dialog.set_default_response(Some("connect"));
        dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);

        let username_row_c = username_row.clone();
        let domain_row_c = domain_row.clone();
        let password_row_c = password_row.clone();

        dialog.connect_response(None, move |dialog, response| {
            if response == "connect" {
                op.set_username(Some(&username_row_c.text()));
                op.set_domain(Some(&domain_row_c.text()));
                op.set_password(Some(&password_row_c.text()));
                op.set_password_save(gio::PasswordSave::ForSession);
                op.reply(gio::MountOperationResult::Handled);
            } else {
                op.reply(gio::MountOperationResult::Aborted);
            }
            dialog.close();
        });

        if let Some(parent) = &parent {
            dialog.present(Some(parent));
        } else {
            dialog.present(gtk::Window::NONE);
        }
    });

    operation
}

/// Mount a network share by URI, prompting for credentials via `interactive_mount_operation`
/// if GVfs asks for them.
pub async fn mount_share(uri: &str, parent: Option<&gtk::Window>) -> Result<()> {
    tracing::info!("Mounting network share: {uri}");
    let file = gio::File::for_uri(uri);
    let operation = interactive_mount_operation(parent);

    file.mount_enclosing_volume_future(gio::MountMountFlags::NONE, Some(&operation))
        .await
        .map_err(|e| anyhow!("Failed to mount {uri}: {e}"))?;

    tracing::info!("Successfully mounted: {uri}");
    Ok(())
}

/// Unmount a currently-mounted network share by URI.
pub async fn unmount_share(uri: &str) -> Result<()> {
    tracing::info!("Unmounting network share: {uri}");
    let monitor = gio::VolumeMonitor::get();
    let mount = monitor
        .mounts()
        .into_iter()
        .find(|m| m.root().uri() == uri)
        .ok_or_else(|| anyhow!("No active mount found for {uri}"))?;

    mount
        .unmount_with_operation_future(gio::MountUnmountFlags::NONE, gio::MountOperation::NONE)
        .await
        .map_err(|e| anyhow!("Failed to unmount {uri}: {e}"))?;

    tracing::info!("Successfully unmounted: {uri}");
    Ok(())
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
}
