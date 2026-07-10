# Building DriveDock

DriveDock is packaged and distributed via a [Nix flake](https://nixos.wiki/wiki/Flakes).
Nix handles fetching and pinning the GTK4/libadwaita/GVfs/UDisks2 toolchain, so no
manual system package installation is required if you have Nix with flakes enabled.

## Prerequisites

- [Nix](https://nixos.org/download) with flakes enabled (`experimental-features = nix-command flakes`
  in `nix.conf`, or pass `--extra-experimental-features 'nix-command flakes'`).
- At runtime (any distro, NixOS or not): a running `gvfsd` session daemon with the
  `gvfs-smb`/`gvfs-nfs` GIO modules, and the `udisks2` D-Bus service, for network
  share *discovery* and local drive mount/unmount to work. On NixOS, the flake's
  `nixosModules.default` enables both automatically (`services.gvfs.enable` and
  `services.udisks2.enable`) when `programs.drivedock.enable = true;`. On non-NixOS
  distros, install your distribution's `gvfs`, `gvfs-smb`, `gvfs-nfs`, and `udisks2`
  packages (these are typically already present on GNOME desktops).
- For *mounting* network shares (dock/undock/permanently-dock): `cifs-utils`
  (`mount.cifs`) and/or `nfs-utils` (`mount.nfs`), plus a running Polkit
  authentication agent (e.g. `polkit-gnome-authentication-agent-1`, or the desktop
  shell's built-in one) so `pkexec` can show a graphical password prompt when
  DriveDock's privileged mount helper (`drivedock-mount-helper`) needs
  authorization. Without an agent running, `pkexec` falls back to a text prompt on
  the invoking terminal, which does not work for a GUI app launched from an app
  menu (no controlling terminal). On NixOS, the flake's `nixosModules.default`
  pulls in `cifs-utils`/`nfs-utils` via `boot.supportedFilesystems.cifs`/`.nfs` and
  enables `security.polkit.enable` automatically. Network shares mount at
  `/mnt/<share-name>` (a fixed convention validated by the privileged helper -
  arbitrary paths are rejected).

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
│   ├── discover.rs    # Network share discovery via GIO/GVfs (network:/// browsing)
│   └── mount.rs        # Network share mount/unmount/persist - builds a JSON request
│                        # and spawns `pkexec drivedock-mount-helper` (see below)
├── dock/
│   ├── mod.rs         # OS detection (NixOS vs. other Linux) + permanent-dock orchestration
│   ├── fstab.rs        # Local drive fstab persistence via UDisks2
│   └── nixos.rs        # NixOS `fileSystems` config-snippet guidance text (local drives
│                        # and network shares)
├── udisks.rs          # Thin wrapper around the `udisks2` D-Bus client crate
└── bin/
    └── drivedock-mount-helper.rs   # Privileged helper (no GTK/gio/glib deps), run only
                                    # as root via `pkexec`: performs the real
                                    # `mount.cifs`/`mount.nfs` mount and `/etc/fstab`
                                    # persistence for network shares

nix/
├── package.nix        # buildRustPackage derivation (installs drivedock-mount-helper
│                        # to $out/libexec/drivedock/, plus the Polkit .policy action)
├── module.nix          # NixOS module (programs.drivedock.enable)
└── shell.nix            # Dev shell
```

## Testing

```bash
cargo test
```

or, inside `nix develop`, the same command works unmodified. This exercises the pure
logic in both `[[bin]]` targets (including `drivedock-mount-helper`'s slug
sanitization, `/etc/fstab` line generation/matching, and `/proc/mounts` idempotency
checks) - the helper's actual privileged file/mount operations require root and are
not exercised by `cargo test`.

## Network share mounting

"Dock"/"Undock"/"Permanently dock" on a network share now perform a real kernel
`cifs`/`nfs` mount at `/mnt/<share-name>`, via `drivedock-mount-helper` invoked
through `pkexec` (Polkit-gated by the `org.example.DriveDock.mount-network-share`
action). This will prompt for authentication (via your desktop's Polkit agent) the
first time a mount/persist/unmount/unpersist action runs. Credentials for shares
that need them are written by the helper (as root) to
`/etc/drivedock/credentials/<slug>.cred` (mode `0600`, root:root) - never to
`/etc/fstab` itself.
