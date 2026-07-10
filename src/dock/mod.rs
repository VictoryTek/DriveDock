pub mod fstab;
pub mod nixos;

use std::path::Path;

use crate::udisks::Udisks;

/// Detected host OS, relevant to "permanently dock" persistence semantics (see §3.4
/// of the Phase 1 spec: NixOS regenerates `/etc/fstab` wholesale on every
/// `nixos-rebuild`, so a UDisks2 fstab write alone does not survive it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    NixOs,
    OtherLinux,
}

/// Detect whether we're running on NixOS, via the standard `/etc/NIXOS` marker file.
pub fn detect_os() -> Os {
    if Path::new("/etc/NIXOS").exists() {
        Os::NixOs
    } else {
        Os::OtherLinux
    }
}

/// Result of toggling "permanently dock" on a local drive - carries an OS-specific
/// status message for display in the UI's Status section.
pub struct DockResult {
    pub message: String,
}

/// Toggle "permanently dock" for a local block device.
///
/// On both OSes this performs the UDisks2 `AddConfigurationItem`/
/// `RemoveConfigurationItem` fstab write, which survives a plain reboot. On NixOS,
/// enabling additionally returns guidance for making the mount durable across a
/// future `nixos-rebuild` (a materially different, weaker guarantee than on
/// non-NixOS - this asymmetry is surfaced in the returned message, per spec §7a).
pub async fn set_permanent_dock(
    udisks: &Udisks,
    object: &udisks2::Object,
    mount_point: &str,
    fs_type: &str,
    options: &str,
    enable: bool,
) -> anyhow::Result<DockResult> {
    if enable {
        fstab::add_entry(udisks, object, mount_point, fs_type, options).await?;
    } else {
        fstab::remove_entry(udisks, object).await?;
    }

    let os = detect_os();
    let message = match (os, enable) {
        (Os::NixOs, true) => {
            let uuid = udisks.uuid_for(object).await;
            nixos::config_snippet_message(mount_point, fs_type, options, uuid.as_deref())
        }
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
