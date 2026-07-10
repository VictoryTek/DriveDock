//! Network-share "permanently dock" persistence.
//!
//! GVfs network mounts are session-scoped FUSE mounts - there is no `/etc/fstab`
//! entry or UDisks2 `Block` object to attach persistence to (see Phase 1 spec §3.4
//! point 2). Per the confirmed §7a decision, "permanently dock" for a network share
//! means: record the URI in `~/.config/drivedock/persistent-shares.json`, and
//! re-mount every recorded URI both (a) via a generated `systemd --user` unit that
//! runs at login, and (b) as a fallback on DriveDock's own startup.
//!
//! DriveDock writes the unit file but deliberately does NOT run
//! `systemctl --user enable --now` itself - that is a user-facing, system-affecting
//! action the user must opt into explicitly (consistent with this project's
//! "don't take irreversible actions silently" ethos).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistentShares {
    uris: HashSet<String>,
}

fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".config/drivedock"))
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("persistent-shares.json"))
}

fn systemd_unit_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".config/systemd/user/drivedock-remount.service"))
}

fn load() -> Result<PersistentShares> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(PersistentShares::default());
    }
    let data = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save(shares: &PersistentShares) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let data = serde_json::to_string_pretty(shares)?;
    std::fs::write(&path, data).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Mark (or unmark) a network share URI as "permanently dock". When enabling, also
/// (re)writes the `systemd --user` re-mount unit file (but does not enable/start it).
pub fn set_persistent(uri: &str, enable: bool) -> Result<()> {
    let mut shares = load()?;
    if enable {
        shares.uris.insert(uri.to_string());
    } else {
        shares.uris.remove(uri);
    }
    save(&shares)?;

    if enable {
        write_systemd_unit()?;
    }
    Ok(())
}

/// Whether a URI is currently marked "permanently dock".
pub fn is_persistent(uri: &str) -> bool {
    load().map(|s| s.uris.contains(uri)).unwrap_or(false)
}

/// List all URIs currently marked "permanently dock".
pub fn list_persistent() -> Result<Vec<String>> {
    Ok(load()?.uris.into_iter().collect())
}

/// Re-mount every persisted network share. Called on DriveDock startup as a fallback
/// in case the `systemd --user` re-mount unit hasn't run yet, isn't enabled, or failed.
pub async fn remount_persistent_shares() {
    let uris = match list_persistent() {
        Ok(uris) => uris,
        Err(e) => {
            tracing::warn!("Failed to read persistent shares record: {e}");
            return;
        }
    };

    for uri in uris {
        if let Err(e) = crate::network::mount_share(&uri, None).await {
            tracing::warn!("Failed to re-mount persistent share {uri}: {e}");
        }
    }
}

/// Path to the generated systemd user unit, for display in the UI/README.
pub fn systemd_unit_display_path() -> String {
    systemd_unit_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "~/.config/systemd/user/drivedock-remount.service".to_string())
}

/// Write (or overwrite) the `systemd --user` unit that re-mounts all persisted shares
/// at login. Invokes DriveDock itself with a `--remount-shares` flag (handled in
/// `main.rs`) for a headless, windowless re-mount pass.
fn write_systemd_unit() -> Result<()> {
    let exe = std::env::current_exe().context("Failed to determine DriveDock executable path")?;
    let unit_path = systemd_unit_path()?;
    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let unit = format!(
        "[Unit]\n\
         Description=DriveDock: re-mount persistent network shares\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={} --remount-shares\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe.display()
    );

    std::fs::write(&unit_path, unit)
        .with_context(|| format!("Failed to write {}", unit_path.display()))?;

    tracing::info!(
        "Wrote systemd --user unit to {}. Run `systemctl --user enable --now drivedock-remount.service` \
         to have persistent shares re-mount automatically at login.",
        unit_path.display()
    );
    Ok(())
}
