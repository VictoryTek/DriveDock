# DriveDock Project Status

## Overview

DriveDock is a Linux drive and network-share manager built with Rust + GTK4 +
libadwaita, distributed as a Nix flake (previously Flatpak - see "Nix Pivot" below).
Scope is intentionally narrow: list local drives + network shares, dock (mount),
undock (unmount), and a per-item "permanently dock" toggle. Nothing else.

## Project Files

```
DriveDock/
├── Cargo.toml                      # Dependencies: gtk4, libadwaita, gio, udisks2, ...
├── flake.nix                       # Nix flake: packages.default, nixosModules.default, devShells.default
├── nix/
│   ├── package.nix                 # buildRustPackage derivation
│   ├── module.nix                  # NixOS module (programs.drivedock.enable)
│   └── shell.nix                   # Dev shell
├── README.md                       # Project documentation
├── docs/BUILDING.md                # Build instructions (Nix-first)
├── LICENSE                         # GPL-3.0
├── .gitignore                      # Rust and Nix ignore patterns
│
└── src/
    ├── main.rs                     # Application entry point
    │                               # - libadwaita::Application setup
    │                               # - Tracing initialization
    │                               # - Headless `--remount-shares` mode for the
    │                               #   generated systemd --user unit
    │                               # - Window creation
    │
    ├── ui/                         # User Interface Module
    │   ├── mod.rs                  # Module exports
    │   └── window.rs               # Main ApplicationWindow
    │                               # - AdwToolbarView with HeaderBar
    │                               # - Two PreferencesGroup sections:
    │                               #   1. Drives (local + network, unified list)
    │                               #   2. Status (operation feedback, incl. the
    │                               #      NixOS fileSystems config-snippet message)
    │                               # - AdwClamp for responsive layout
    │
    ├── system/                     # Local Drive Management Module
    │   ├── mod.rs                  # Module exports
    │   ├── local.rs                # Drive Enumeration
    │   │                           # - MountedDrive struct projected from gio::Volume/Mount
    │   │                           # - list_local_drives() via gio::VolumeMonitor
    │   │                           # - is_critical_mount() safety check
    │   │                           # - Unit tests
    │   │
    │   └── unmount.rs              # Unmount Operations
    │                               # - UnmountError enum (thiserror)
    │                               # - unmount_drive() via UDisks2 (crate::udisks)
    │                               # - Unit tests
    │
    ├── network/                    # Network Share Discovery Module
    │   ├── mod.rs                  # Module exports
    │   └── gvfs.rs                 # GVfs/GIO-backed discovery + mount/unmount
    │                               # - NetworkShare struct
    │                               # - scan_network_shares() via network:/// + VolumeMonitor
    │                               # - mount_share()/unmount_share() via gio::File/Mount
    │                               # - Minimal GTK/libadwaita credential-prompt MountOperation
    │                               # - Unit tests
    │
    ├── dock/                       # Permanent-dock persistence
    │   ├── mod.rs                  # OS detection (/etc/NIXOS marker) + orchestration
    │   ├── fstab.rs                # Local-drive fstab persistence via UDisks2
    │   ├── nixos.rs                # NixOS fileSystems config-snippet guidance text
    │   └── shares.rs                # Network-share re-mount-on-login persistence
    │                                 # (~/.config/drivedock/persistent-shares.json +
    │                                 # generated systemd --user unit)
    │
    └── udisks.rs                   # Thin wrapper around the `udisks2` D-Bus client crate
```

## Nix Pivot (from Flatpak)

DriveDock switched distribution from Flatpak to a Nix flake. Rationale: the flake
needs to expose a full GUI package plus a NixOS module consumable as a flake input
by a separate NixOS-config repo - a use case Flatpak doesn't serve well. See
`.github/docs/subagent_docs/nix-pivot-scope-narrow_spec.md` for the full research
and architectural decision record behind this pivot (GVfs/GIO discovery, the
UDisks2-for-local/GVfs-for-network mount split, and NixOS fstab semantics).

Key consequences:
- `org.example.DriveDock.yml`, `build-flatpak.sh`, and `setup-dev.sh` are removed.
- `flake.nix` exposes `packages.default` (the GUI app), `nixosModules.default`
  (`programs.drivedock.enable`, which also turns on `services.udisks2` and
  `services.gvfs`), and `devShells.default`.
- Network share discovery no longer shells out to `avahi-browse`/`smbclient`/
  `showmount` - it piggybacks on GNOME's own GVfs discovery via `gio`.
- Local drive listing no longer parses `/proc/mounts` - it uses `gio::VolumeMonitor`.
- Mount/unmount is split by kind: local block devices go through UDisks2 D-Bus
  (Polkit-gated, privileged); network shares go through GVfs (unprivileged,
  session-scoped FUSE mounts).

## Key Architectural Decisions

### 1. UI Framework: GTK4 + libadwaita
Unchanged by the pivot - `AdwToolbarView`, `AdwPreferencesGroup`, `AdwClamp` for a
native GNOME-style adaptive layout.

### 2. Async Strategy: GLib MainContext
GTK4's native event loop; async functions return standard Rust futures via
`glib::spawn_future_local`. `tokio` remains an optional, currently-unused feature.

### 3. Discovery/Mount Backend: GIO/GVfs + UDisks2
- **Local drives**: `gio::VolumeMonitor` for listing; UDisks2 D-Bus
  (`Filesystem.Mount`/`Unmount`, `Block.AddConfigurationItem`/`RemoveConfigurationItem`)
  for privileged mount/unmount and fstab persistence.
- **Network shares**: `gio::File::for_uri("network:///")` + `enumerate_children_async`
  for discovery (backed by GVfs's own Avahi/SMB-browsing internals); `gio::File::mount_enclosing_volume`
  / `gio::Mount::unmount_with_operation` for mount/unmount (unprivileged, session-scoped).

### 4. Error Handling: anyhow + thiserror
Unchanged - `thiserror` for library errors (`UnmountError`), `anyhow` for
application-level error propagation.

### 5. Module Organization
- `ui/` - user interface only
- `system/` - local drive listing/unmount
- `network/` - GVfs-backed network share discovery/mount/unmount
- `dock/` - OS-aware "permanently dock" persistence
- `udisks.rs` - UDisks2 D-Bus client wrapper

## Implementation Status

### Complete
- Nix flake packaging (`flake.nix`, `nix/package.nix`, `nix/module.nix`, `nix/shell.nix`)
- Local drive listing via `gio::VolumeMonitor` (`src/system/local.rs`)
- Local drive mount/unmount + fstab persistence via UDisks2 (`src/system/unmount.rs`, `src/udisks.rs`, `src/dock/fstab.rs`)
- Network share discovery/mount/unmount via GVfs/GIO (`src/network/gvfs.rs`)
- Network share "permanently dock" (re-mount-on-login) persistence (`src/dock/shares.rs`)
- NixOS-aware permanent-dock guidance (`src/dock/mod.rs`, `src/dock/nixos.rs`)
- Rewritten UI binding all of the above (`src/ui/window.rs`)

### Known limitations / follow-ups
- `CLAUDE.md` itself still references the removed Flatpak forbidden-commands/manifest
  and needs a follow-up update (explicitly out of scope for the Phase 1/2 cycle that
  did this pivot - flagged for the user).
- Nix flake validation (`nix build`/`nix flake check`) should be re-run in an
  environment with reliable network access to the Nix binary cache before treating
  the flake as fully verified end-to-end.
- No automated CI is configured yet (`.github/workflows/` is empty); `scripts/preflight.sh`
  runs the same `cargo check`/`cargo build`/`cargo test` checks locally.

## Dependencies

### Core UI
- `gtk = { version = "0.9", package = "gtk4" }`
- `libadwaita = { version = "0.7", package = "libadwaita" }`
- `glib = "0.20"`
- `gio = "0.20"` - `VolumeMonitor`, `File`, `Mount`, `Volume`, `Drive`, `MountOperation`

### System/D-Bus
- `udisks2 = "0.3"` - UDisks2 D-Bus client (pulls in `zbus` transitively)

### Utilities
- `anyhow = "1.0"` - Error handling
- `thiserror = "1.0"` - Error derive macros
- `tracing` / `tracing-subscriber` - Structured logging
- `serde` / `serde_json` - persistent-shares JSON record

### Optional
- `tokio` (`tokio-runtime` feature) - reserved for future async needs, currently unused

## Nix Packaging

**Approach**: `pkgs.rustPlatform.buildRustPackage` with `cargoLock.lockFile = ./Cargo.lock;`.

**Build inputs**: `nativeBuildInputs = [ pkg-config wrapGAppsHook4 ]`,
`buildInputs = [ gtk4 libadwaita glib ]`.

**NixOS module** (`nix/module.nix`): `programs.drivedock.enable` installs the package
and enables `services.udisks2.enable = true;` and `services.gvfs.enable = true;` -
both are hard runtime dependencies of the pivot's GVfs/UDisks2-based design.

**Flake outputs**: `packages.default` / `packages.drivedock`, `nixosModules.default` /
`nixosModules.drivedock`, `devShells.default`.
