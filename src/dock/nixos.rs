//! NixOS-specific "permanently dock" guidance.
//!
//! On NixOS, `/etc/fstab` is fully regenerated from the `fileSystems` module option
//! on every `nixos-rebuild switch`/`boot` - a runtime UDisks2 fstab write (see
//! `crate::dock::fstab`) survives a plain reboot but is silently wiped on the next
//! rebuild. DriveDock cannot durably persist a mount across a rebuild from a running
//! GUI app; the technically honest behavior is to say so and hand the user the exact
//! declarative config snippet to add themselves (per Phase 1 spec §3.4 / §7a).

/// Build the user-facing guidance message shown in the Status section after enabling
/// "permanently dock" on NixOS. Distinct in wording from the non-NixOS message
/// (`crate::dock::set_permanent_dock`) - this asymmetry must be visible in the UI
/// copy itself, not just code comments.
pub fn config_snippet_message(
    mount_point: &str,
    fs_type: &str,
    options: &str,
    uuid: Option<&str>,
) -> String {
    let device = match uuid {
        Some(uuid) => format!("/dev/disk/by-uuid/{uuid}"),
        None => "/dev/disk/by-uuid/<uuid>".to_string(),
    };
    let opts = if options.is_empty() {
        "\"defaults\"".to_string()
    } else {
        options
            .split(',')
            .filter(|o| !o.is_empty())
            .map(|o| format!("\"{o}\""))
            .collect::<Vec<_>>()
            .join(" ")
    };

    format!(
        "\"{mount_point}\" will mount automatically after a plain reboot. This is NixOS: \
         /etc/fstab is regenerated on every nixos-rebuild, so to make this mount survive a \
         future rebuild, add this to your NixOS configuration:\n\n\
         fileSystems.\"{mount_point}\" = {{\n  device = \"{device}\";\n  fsType = \"{fs_type}\";\n  options = [ {opts} ];\n}};"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_snippet_includes_mount_point_and_uuid() {
        let msg = config_snippet_message("/mnt/data", "ext4", "defaults", Some("1234-ABCD"));
        assert!(msg.contains("/mnt/data"));
        assert!(msg.contains("1234-ABCD"));
        assert!(msg.contains("fileSystems"));
    }

    #[test]
    fn test_config_snippet_without_uuid_uses_placeholder() {
        let msg = config_snippet_message("/mnt/data", "ext4", "", None);
        assert!(msg.contains("<uuid>"));
        assert!(msg.contains("\"defaults\""));
    }
}
