# DriveDock

A modern Linux drive and network share manager with a simple, GNOME-friendly interface.

## Features

- List local drives and network shares (SMB/NFS/etc.), discovered via GNOME's own
  GVfs/GIO stack - no reimplemented Avahi/smbclient scanning
- Dock (mount) and undock (unmount) drives and shares
- "Permanently dock" toggle to persist a mount across reboots:
  - Local drives: an `/etc/fstab` entry via UDisks2 (works the same on any Linux
    distro, NixOS included - see [docs/BUILDING.md](docs/BUILDING.md) for the NixOS
    caveat around `nixos-rebuild` regenerating `/etc/fstab`)
  - Network shares: a re-mount-on-login `systemd --user` unit, since GVfs network
    mounts are session-scoped and have no `/etc/fstab` entry

## Installation

DriveDock is packaged as a [Nix flake](https://nixos.wiki/wiki/Flakes):

```bash
# Build and run directly
nix run github:yourusername/DriveDock

# Or build a local package
git clone https://github.com/yourusername/DriveDock.git
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
