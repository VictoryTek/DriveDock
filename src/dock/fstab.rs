//! Local-drive "permanently dock" persistence via UDisks2's `Block` fstab
//! configuration items (see `crate::udisks`).
//!
//! This path is used identically on non-NixOS Linux and NixOS - it writes an
//! `/etc/fstab` entry that survives a plain reboot on either OS. It is *not*
//! sufficient on its own to survive a NixOS `nixos-rebuild` (which regenerates
//! `/etc/fstab` from the `fileSystems` module option); the NixOS-specific
//! additional guidance lives in `crate::dock::nixos`.

use anyhow::Result;
use udisks2::Object;

use crate::udisks::Udisks;

/// Add an fstab entry for a local block device via UDisks2.
pub async fn add_entry(
    udisks: &Udisks,
    object: &Object,
    mount_point: &str,
    fs_type: &str,
    options: &str,
) -> Result<()> {
    udisks
        .add_fstab_entry(object, mount_point, fs_type, options)
        .await
}

/// Remove a local block device's fstab entry, un-toggling "permanently dock".
pub async fn remove_entry(udisks: &Udisks, object: &Object) -> Result<()> {
    udisks.remove_fstab_entry(object).await
}
