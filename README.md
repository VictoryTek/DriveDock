# DriveDock

A modern Linux drive and network share manager with a simple, GNOME-friendly interface.

## Features

- List local drives and network shares (SMB/NFS/etc.), discovered via GNOME's own
  GVfs/GIO stack - no reimplemented Avahi/smbclient scanning
- Dock (mount) and undock (unmount) drives and shares
- "Permanently dock" toggle to persist a mount across reboots, for both local
  drives and network shares, as a real `/etc/fstab` entry:
  - Local drives: via UDisks2 (works the same on any Linux distro, NixOS included -
    see [docs/BUILDING.md](docs/BUILDING.md) for the NixOS caveat around
    `nixos-rebuild` regenerating `/etc/fstab`)
  - Network shares (SMB/NFS): mounted as a real kernel `cifs`/`nfs` mount at
    `/mnt/<share-name>` by a small privileged helper (`drivedock-mount-helper`,
    invoked via `pkexec`/Polkit) - a stable, system-wide mount, not a per-session
    GVfs/FUSE mount. See [docs/BUILDING.md](docs/BUILDING.md) for the additional
    runtime requirements this needs (`cifs-utils`/`nfs-utils`, a Polkit
    authentication agent).

## Installation

DriveDock is packaged as a [Nix flake](https://nixos.wiki/wiki/Flakes):

```bash
# Run directly
nix run github:victorytek/drivedock

# Install to profile
nix profile add github:victorytek/drivedock

# Development shell
nix develop github:victorytek/drivedock
```

Or build a local checkout:

```bash
git clone https://github.com/victorytek/DriveDock.git
cd DriveDock
nix build .#default
./result/bin/drivedock
```

On NixOS, consume the flake's `nixosModules.default` from your own configuration
(e.g. a flake input named `drivedock`) and set `programs.drivedock.enable = true;` -
this also enables the `udisks2` and `gvfs` system services DriveDock depends on.

For detailed build/dev instructions (including the native `cargo build` path), see
[docs/BUILDING.md](docs/BUILDING.md).

## Status

🚧 **Early Development** - Core drive enumeration complete, network share features in progress.

## License

GPL-3.0 - See [LICENSE](LICENSE) for details.
