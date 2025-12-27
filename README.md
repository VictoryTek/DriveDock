# DriveDock

A modern Linux drive and network share manager with a simple, GNOME-friendly interface.

## Features

- View and manage mounted local drives
- Discover and mount SMB/CIFS network shares
- Discover and mount NFS exports  
- Safely unmount drives when not in use
- Optionally persist mounts to `/etc/fstab`

## Installation

**From Source:**
```bash
git clone https://github.com/yourusername/DriveDock.git
cd DriveDock
./setup-dev.sh
./build-flatpak.sh
flatpak run org.example.DriveDock
```

For detailed build instructions, see [docs/BUILDING.md](docs/BUILDING.md).

## Status

🚧 **Early Development** - Core drive enumeration complete, network share features in progress.

## License

GPL-3.0 - See [LICENSE](LICENSE) for details.
