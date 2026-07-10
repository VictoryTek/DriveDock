You are helping build a Rust desktop application called **DriveDock**.
DriveDock is a Nix-flake-delivered utility for Linux that lists local drives and
network shares, mounts ("docks") and unmounts ("undocks") them, and offers a
per-item "permanently dock" toggle to persist the mount across reboots.

==============================
CRITICAL RULE — CONTEXT7
==============================
ALWAYS use **context7** when adding ANY new feature, idea, improvement,
design decision, architectural change, or new capability.
Do not generate new functionality without considering and aligning with context7.

Repeat: Every time you expand this project in any way,
**always use context7**.

==============================
PROJECT GOALS AND REQUIREMENTS
==============================

Primary Purpose (narrowed scope - nothing beyond this list):
- Display currently mounted (and, for local drives, not-yet-mounted) drives
- Allow safe unmounting ("undocking") of drives not actively in use by the OS
- Detect available network shares (SMB, NFS, etc.) via GNOME's own GVfs/GIO discovery
  (`gio::File::for_uri("network:///")`) - do not reimplement Avahi/smbclient/showmount
  scanning
- Allow the user to mount ("dock") network shares
- Provide a "permanently dock" toggle:
  - Local drives: writes an `/etc/fstab` entry via UDisks2's `Block.AddConfigurationItem`
    (not raw file edits)
  - Network shares: GVfs mounts are session-scoped and have no fstab entry - persist a
    re-mount-on-login record instead (see CORE FUNCTIONAL BEHAVIOR below)
- App is built and shipped as a **Nix flake** (`flake.nix`), exposing
  `packages.default` (the GUI), `nixosModules.default` (a NixOS module consumable as
  a flake input by downstream NixOS configs), and `devShells.default`
- Written in Rust

==============================
UI + UX REQUIREMENTS
==============================
- Modern, minimal, GNOME-friendly UI
- GTK4 + libadwaita UI in Rust
- Layout:
  SECTION 1: "Drives" (local drives and network shares in one unified list)
    - Local drive rows: device name, mount point (if mounted), filesystem type,
      size + used if available, "Local" kind badge, Dock/Undock button, "Permanently
      dock" checkbox
    - Network share rows: display name, URI, protocol label (SMB / NFS / other) kind
      badge, Dock/Undock button, "Permanently dock" checkbox

  SECTION 2: "Status"
    - Show helpful errors
    - Show successful dock/undock feedback
    - Show the NixOS `fileSystems` config-snippet guidance when "permanently dock" is
      toggled on a local drive on NixOS - this message must read differently from the
      non-NixOS case (NixOS's `/etc/fstab` is regenerated on every `nixos-rebuild`, so
      the fstab write alone is a weaker guarantee there)
    - Be user-friendly and understandable

==============================
CORE FUNCTIONAL BEHAVIOR
==============================

LOCAL DRIVE HANDLING
- List local drives via `gio::VolumeMonitor` (not `/proc/mounts` parsing)
- Mount/unmount via UDisks2 D-Bus `Filesystem.Mount`/`Unmount` (Polkit-gated,
  privileged - UDisks2 itself handles authorization, no manual `pkexec`)
- Ensure no unmount of critical system drives (`/`, `/boot`, `/usr`, `/var`, `/home`)
- Graceful error handling (busy device, permission denied, etc.)

NETWORK SHARE HANDLING
- Discover via GVfs's `network:///` root (`gio::File::for_uri` +
  `enumerate_children_async`) plus already-mounted shares from `gio::VolumeMonitor`
- Mount/unmount via `gio::File::mount_enclosing_volume` /
  `gio::Mount::unmount_with_operation` - unprivileged, session-scoped (no Polkit
  prompt; this is NOT a privileged operation, unlike local drives)
- Credential prompts: a minimal GTK/libadwaita `gio::MountOperation` password dialog
  (not a full credential manager)
- "Permanently dock" = re-mount-on-login: record the URI in
  `~/.config/drivedock/persistent-shares.json`, generate (but do not
  enable/start) a `systemd --user` unit, and re-mount recorded URIs on DriveDock's
  own startup as a fallback

==============================
ENGINEERING EXPECTATIONS
==============================
- Language: Rust
- UI Framework: GTK4 + libadwaita
- Maintainable modular architecture: `ui/`, `system/` (local), `network/` (GVfs),
  `dock/` (OS-aware permanent-dock persistence), `udisks.rs` (UDisks2 client wrapper)
- Prefer async where appropriate (`glib::spawn_future_local`)
- Safe Rust first
- Avoid blocking UI
- Reference **context7** when:
  - designing features
  - structuring modules
  - choosing crates
  - planning UX flows

==============================
SECURITY + SAFETY
==============================
- Respect system permissions
- Prefer Polkit/UDisks2 D-Bus for privileged operations (local drive mount/unmount,
  fstab writes) - never raw `pkexec mount`/hand-rolled fstab file edits
- Network share mounts are intentionally unprivileged (GVfs/FUSE, session-scoped) -
  do not add a Polkit/root path for these, that would be a regression
- Never take an irreversible, system-affecting action silently: DriveDock writes the
  `systemd --user` re-mount unit file but does not enable/start it itself - the user
  runs `systemctl --user enable --now drivedock-remount.service` themselves
- Avoid unsafe Rust unless absolutely necessary

==============================
NIX PACKAGING REQUIREMENTS
==============================
- Provide `flake.nix` + `nix/package.nix` (`buildRustPackage`, `cargoLock.lockFile`),
  `nix/module.nix` (NixOS module taking `self` as a parameter, enabling
  `services.udisks2.enable` and `services.gvfs.enable`), `nix/shell.nix` (dev shell)
- Include `wrapGAppsHook4` in `nativeBuildInputs` (GTK4-specific - not the GTK3
  `wrapGAppsHook`) so GSettings schemas, icon themes, and GVfs GIO modules resolve
  correctly in the packaged binary
- Document the `gvfs`/`gvfs-smb`/`gvfs-nfs`/`udisks2` runtime dependency for non-NixOS
  users in README.md/docs/BUILDING.md - this is a hard runtime requirement introduced
  by the GVfs-based discovery design, not optional

==============================
DEVELOPER EXPERIENCE
==============================
When generating code:
- Use idiomatic Rust
- Explain architecture when needed
- Suggest appropriate crates
- Avoid meaningless placeholders
- Always ensure new ideas follow **context7**

==============================
STATUS
==============================
Core scope (list/dock/undock/permanently-dock for local drives and network shares,
GVfs/UDisks2-backed, Nix-flake-packaged) is implemented. See
`.github/docs/subagent_docs/nix-pivot-scope-narrow_spec.md` for the research and
architectural record behind the Flatpak-to-Nix pivot and scope narrowing, and
`docs/PROJECT_STATUS.md` for current implementation status.
