//! Privileged network-share mount/unmount/persist orchestration.
//!
//! Network shares no longer mount via GVfs (session-scoped FUSE) - per the
//! privileged-mount spec, they get a real kernel `cifs`/`nfs` mount at a stable
//! system-wide path (`/mnt/<share-slug>`), performed by the root-executed
//! `drivedock-mount-helper` binary via `pkexec`. This module is the unprivileged
//! GUI-side half of that split: it collects credentials (if needed), builds the
//! JSON request, spawns the helper, and reports the result.

use anyhow::{anyhow, Context, Result};
use libadwaita as adw;
use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::dock::{self, DockResult, Os};
use crate::network::discover::{mount_source_from_uri, NetworkShare};

/// Credentials collected from the user for a share that requires authentication.
/// Plain Rust strings - not tied to a `gio::MountOperation` signal (GVfs is no
/// longer performing the mount).
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    pub domain: String,
}

#[derive(Serialize)]
struct MountRequest<'a> {
    share_name: &'a str,
    fs_type: &'a str,
    source: &'a str,
    mount_point: &'a str,
    options: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<&'a str>,
}

/// Sanitize a share's display name into the slug used for its mount point
/// (`/mnt/<slug>`) and, on the helper side, its credentials filename and fstab tag.
/// Mirrors `drivedock-mount-helper`'s own `slugify()` so the two agree on naming -
/// duplicated rather than shared, since the helper deliberately has no dependency
/// on this crate's non-`std` code (kept minimal/auditable, see the helper's doc
/// comment).
fn slugify(name: &str) -> String {
    name.chars()
        .map(|c| c.to_ascii_lowercase())
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// The kernel filesystem type (`cifs`/`nfs`) for a share's protocol label, or
/// `None` for protocols the privileged helper does not support mounting
/// (e.g. WebDAV, FTP - those remain GVfs-discoverable but not dock-able under this
/// design).
fn fs_type_for_protocol(protocol: &str) -> Option<&'static str> {
    match protocol {
        "SMB" => Some("cifs"),
        "NFS" => Some("nfs"),
        _ => None,
    }
}

fn mount_point_for(share: &NetworkShare) -> String {
    format!("/mnt/{}", slugify(&share.display_name))
}

/// Locate the privileged helper binary's absolute path.
///
/// In a Nix-installed layout, the GUI binary lives at `$out/bin/drivedock` and the
/// helper is installed out-of-band at `$out/libexec/drivedock/drivedock-mount-helper`
/// (see `nix/package.nix`) - deliberately not in `$out/bin`, since it must never be
/// run directly/unprivileged-usefully. We derive that path from our own
/// `current_exe()` at runtime, since the Nix store path (and thus the absolute
/// helper path) is only known at build time, not compile time.
///
/// For local/dev builds (`cargo build`/`cargo run`), Cargo places every `[[bin]]`
/// target as a sibling in the same `target/debug/`/`target/release/` directory - so
/// we fall back to looking for `drivedock-mount-helper` right next to the running
/// executable.
fn helper_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("failed to determine DriveDock's own exe path")?;
    let bin_dir = exe
        .parent()
        .ok_or_else(|| anyhow!("DriveDock exe path has no parent directory"))?;

    // Nix-installed layout: $out/bin/drivedock -> $out/libexec/drivedock/drivedock-mount-helper
    if let Some(prefix) = bin_dir.parent() {
        let candidate = prefix
            .join("libexec")
            .join("drivedock")
            .join("drivedock-mount-helper");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    // Dev/local build fallback: sibling of the current exe (target/debug/, target/release/).
    let candidate = bin_dir.join("drivedock-mount-helper");
    if candidate.is_file() {
        return Ok(candidate);
    }

    Err(anyhow!(
        "could not locate drivedock-mount-helper next to {} or under its libexec/ sibling",
        exe.display()
    ))
}

/// Spawn `pkexec <helper> <subcommand>`, write `request` as JSON to its stdin, and
/// wait for it to exit. Maps a non-zero exit or spawn failure to a clear error.
fn run_helper(subcommand: &str, request: &MountRequest) -> Result<()> {
    let helper = helper_path()?;
    let payload = serde_json::to_vec(request).context("failed to serialize mount request")?;

    let mut child = Command::new("pkexec")
        .arg(&helper)
        .arg(subcommand)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn pkexec {}", helper.display()))?;

    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("failed to open helper stdin"))?
        .write_all(&payload)
        .context("failed to write request to helper stdin")?;

    let output = child
        .wait_with_output()
        .context("failed to wait for helper to exit")?;

    if !output.status.success() {
        return Err(anyhow!(
            "drivedock-mount-helper {subcommand} failed (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(())
}

/// Mount a network share as a real kernel `cifs`/`nfs` mount at `/mnt/<slug>`, via
/// the privileged helper.
pub async fn mount_share(share: &NetworkShare, creds: Option<Credentials>) -> Result<()> {
    let fs_type = fs_type_for_protocol(&share.protocol).ok_or_else(|| {
        anyhow!(
            "\"{}\" is a {} share - only SMB/CIFS and NFS shares can be docked system-wide",
            share.display_name,
            share.protocol
        )
    })?;
    let source = mount_source_from_uri(&share.uri)
        .ok_or_else(|| anyhow!("could not determine a mount source for {}", share.uri))?;
    let mount_point = mount_point_for(share);

    tracing::info!("Mounting network share {} at {mount_point}", share.uri);

    let request = MountRequest {
        share_name: &share.display_name,
        fs_type,
        source: &source,
        mount_point: &mount_point,
        options: "",
        username: creds.as_ref().map(|c| c.username.as_str()),
        password: creds.as_ref().map(|c| c.password.as_str()),
        domain: creds.as_ref().map(|c| c.domain.as_str()),
    };

    run_helper("mount", &request)?;
    tracing::info!("Successfully mounted {} at {mount_point}", share.uri);
    Ok(())
}

/// Unmount a network share previously mounted via `mount_share`.
pub async fn unmount_share(share: &NetworkShare) -> Result<()> {
    let fs_type = fs_type_for_protocol(&share.protocol).unwrap_or("cifs");
    let source = mount_source_from_uri(&share.uri)
        .ok_or_else(|| anyhow!("could not determine a mount source for {}", share.uri))?;
    let mount_point = mount_point_for(share);

    tracing::info!("Unmounting network share {} at {mount_point}", share.uri);

    let request = MountRequest {
        share_name: &share.display_name,
        fs_type,
        source: &source,
        mount_point: &mount_point,
        options: "",
        username: None,
        password: None,
        domain: None,
    };

    run_helper("unmount", &request)?;
    tracing::info!("Successfully unmounted {}", share.uri);
    Ok(())
}

/// Toggle "permanently dock" for a network share: writes (or removes) a tagged
/// `/etc/fstab` entry via the privileged helper's `persist`/`unpersist`
/// subcommand. `_netdev` is always included by the helper, so the entry mounts
/// automatically at boot without any DriveDock-side re-mount machinery. On NixOS,
/// also returns the `fileSystems` config-snippet guidance (mirroring
/// `dock::set_permanent_dock` for local drives), since `/etc/fstab` is regenerated
/// wholesale on every `nixos-rebuild`.
pub async fn set_persistent(
    share: &NetworkShare,
    creds: Option<Credentials>,
    enable: bool,
) -> Result<DockResult> {
    let fs_type = fs_type_for_protocol(&share.protocol).ok_or_else(|| {
        anyhow!(
            "\"{}\" is a {} share - only SMB/CIFS and NFS shares can be permanently docked",
            share.display_name,
            share.protocol
        )
    })?;
    let source = mount_source_from_uri(&share.uri)
        .ok_or_else(|| anyhow!("could not determine a mount source for {}", share.uri))?;
    let mount_point = mount_point_for(share);

    let request = MountRequest {
        share_name: &share.display_name,
        fs_type,
        source: &source,
        mount_point: &mount_point,
        options: "",
        username: creds.as_ref().map(|c| c.username.as_str()),
        password: creds.as_ref().map(|c| c.password.as_str()),
        domain: creds.as_ref().map(|c| c.domain.as_str()),
    };

    run_helper(if enable { "persist" } else { "unpersist" }, &request)?;

    let slug = slugify(&share.display_name);
    let credentials_path = format!("/etc/drivedock/credentials/{slug}.cred");
    let message = match (dock::detect_os(), enable) {
        (Os::NixOs, true) => dock::nixos::network_config_snippet_message(
            &mount_point,
            &source,
            fs_type,
            &credentials_path,
        ),
        (Os::NixOs, false) => format!(
            "\"{mount_point}\" will no longer mount automatically after a plain reboot. \
             If you added a `fileSystems.\"{mount_point}\"` block to your NixOS configuration, \
             remove it there too and run nixos-rebuild for the change to fully take effect."
        ),
        (Os::OtherLinux, true) => format!(
            "\"{mount_point}\" will now mount automatically at boot (fstab entry added)."
        ),
        (Os::OtherLinux, false) => format!(
            "\"{mount_point}\" will no longer mount automatically at boot (fstab entry removed)."
        ),
    };

    Ok(DockResult { message })
}

/// Present a minimal credential-collection dialog (username/domain/password) and
/// asynchronously return what the user entered, or `None` if they cancelled.
///
/// Reuses the same `AdwAlertDialog` + `PasswordEntryRow` layout as the removed
/// `interactive_mount_operation()` (GVfs `ask-password` handler), but returns plain
/// Rust strings instead of answering a `gio::MountOperation` signal - the privileged
/// helper (not GVfs) performs the actual mount.
pub async fn collect_credentials(parent: Option<&gtk::Window>) -> Option<Credentials> {
    use adw::prelude::*;
    use futures::channel::oneshot;

    let (tx, rx) = oneshot::channel::<Option<Credentials>>();
    let tx = std::cell::RefCell::new(Some(tx));

    let dialog = adw::AlertDialog::builder()
        .heading("Authentication Required")
        .body("Enter credentials to mount this network share.")
        .build();

    let content_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .build();

    let username_row = adw::EntryRow::builder().title("Username").build();
    let domain_row = adw::EntryRow::builder().title("Domain").build();
    let password_row = adw::PasswordEntryRow::builder().title("Password").build();
    content_box.append(&username_row);
    content_box.append(&domain_row);
    content_box.append(&password_row);

    dialog.set_extra_child(Some(&content_box));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("connect", "Connect");
    dialog.set_default_response(Some("connect"));
    dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);

    let username_row_c = username_row.clone();
    let domain_row_c = domain_row.clone();
    let password_row_c = password_row.clone();

    dialog.connect_response(None, move |dialog, response| {
        let result = if response == "connect" {
            Some(Credentials {
                username: username_row_c.text().to_string(),
                domain: domain_row_c.text().to_string(),
                password: password_row_c.text().to_string(),
            })
        } else {
            None
        };
        if let Some(tx) = tx.borrow_mut().take() {
            let _ = tx.send(result);
        }
        dialog.close();
    });

    if let Some(parent) = parent {
        dialog.present(Some(parent));
    } else {
        dialog.present(gtk::Window::NONE);
    }

    rx.await.unwrap_or(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Media Fileserver"), "mediafileserver");
        assert_eq!(slugify("media-fileserver"), "media-fileserver");
    }

    #[test]
    fn test_fs_type_for_protocol() {
        assert_eq!(fs_type_for_protocol("SMB"), Some("cifs"));
        assert_eq!(fs_type_for_protocol("NFS"), Some("nfs"));
        assert_eq!(fs_type_for_protocol("WebDAV"), None);
    }

    #[test]
    fn test_mount_point_for() {
        let share = NetworkShare {
            display_name: "Media Fileserver".to_string(),
            uri: "smb://fileserver/media".to_string(),
            protocol: "SMB".to_string(),
            mounted: false,
        };
        assert_eq!(mount_point_for(&share), "/mnt/mediafileserver");
    }
}
