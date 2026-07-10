# Building DriveDock

DriveDock is packaged and distributed via a [Nix flake](https://nixos.wiki/wiki/Flakes).
Nix handles fetching and pinning the GTK4/libadwaita/GVfs/UDisks2 toolchain, so no
manual system package installation is required if you have Nix with flakes enabled.

## Prerequisites

- [Nix](https://nixos.org/download) with flakes enabled (`experimental-features = nix-command flakes`
  in `nix.conf`, or pass `--extra-experimental-features 'nix-command flakes'`).
- At runtime (any distro, NixOS or not): a running `gvfsd` session daemon with the
  `gvfs-smb`/`gvfs-nfs` GIO modules, and the `udisks2` D-Bus service, for network
  share discovery and local drive mount/unmount to work. On NixOS, the flake's
  `nixosModules.default` enables both automatically (`services.gvfs.enable` and
  `services.udisks2.enable`) when `programs.drivedock.enable = true;`. On non-NixOS
  distros, install your distribution's `gvfs`, `gvfs-smb`, `gvfs-nfs`, and `udisks2`
  packages (these are typically already present on GNOME desktops).

## Development Workflow

### Option 1: Nix devShell (recommended)

```bash
nix develop
cargo build
cargo run
```

`nix develop` drops you into a shell with `pkg-config`, GTK4, libadwaita, GVfs,
`rust-analyzer`, `clippy`, and `rustfmt` available - `cargo build`/`cargo check`/
`cargo test` work exactly as in any Rust project from there.

### Option 2: Nix package build

```bash
nix build .#default
./result/bin/drivedock
```

### Option 3: Run directly via flake

```bash
nix run .#default
```

### Option 4: Native build (system libraries installed manually)

If you'd rather not use Nix, install the GTK4/libadwaita/GVfs/UDisks2 development
packages for your distribution and build with Cargo directly:

```bash
cargo build
cargo run
```

If you see errors like:
```
The system library `glib-2.0` required by crate `glib-sys` was not found.
```
you're missing the GTK4/GLib development headers - see `nix/package.nix` for the
exact `buildInputs` list Nix uses, as a reference for equivalent distro packages
(e.g. `gtk4-devel libadwaita-devel glib2-devel` on Fedora, `libgtk-4-dev
libadwaita-1-dev libglib2.0-dev` on Debian/Ubuntu).

## Code Structure

The project follows a clean modular architecture, narrowed to: list local drives +
network shares, mount ("dock"), unmount ("undock"), and a per-item "permanently dock"
toggle.

```
src/
├── main.rs           # Entry point, GTK application setup
├── ui/
│   ├── mod.rs
│   └── window.rs     # Main window: Drives section (local + network) + Status section
├── system/
│   ├── mod.rs
│   ├── local.rs      # Local drive enumeration via gio::VolumeMonitor
│   └── unmount.rs    # Unmount via UDisks2 (crate::udisks), critical-mount safety checks
├── network/
│   ├── mod.rs
│   └── gvfs.rs        # Network share discovery/mount/unmount via GIO/GVfs
├── dock/
│   ├── mod.rs         # OS detection (NixOS vs. other Linux) + permanent-dock orchestration
│   ├── fstab.rs        # Local drive fstab persistence via UDisks2
│   ├── nixos.rs        # NixOS `fileSystems` config-snippet guidance text
│   └── shares.rs        # Network share "permanently dock" (re-mount-on-login) persistence
└── udisks.rs          # Thin wrapper around the `udisks2` D-Bus client crate

nix/
├── package.nix        # buildRustPackage derivation
├── module.nix          # NixOS module (programs.drivedock.enable)
└── shell.nix            # Dev shell
```

## Testing

```bash
cargo test
```

or, inside `nix develop`, the same command works unmodified.

## Persistent network shares

Toggling "Permanently dock" on a network share writes a `~/.config/systemd/user/drivedock-remount.service`
unit (re-mounting recorded shares at login) but does **not** enable/start it
automatically - run:

```bash
systemctl --user enable --now drivedock-remount.service
```

yourself once you're happy with your persistent-share selection. DriveDock also
re-mounts recorded shares on its own startup as a fallback.
