# DriveDock

A modern Linux drive and network share manager built with GTK4 and libadwaita.

## Overview

DriveDock is a Flatpak application that provides an intuitive interface for:
- Viewing and managing mounted local drives
- Discovering and mounting SMB/CIFS network shares
- Discovering and mounting NFS exports
- Safely unmounting drives when not in use
- Optionally persisting mounts to `/etc/fstab`

## Features

### Local Drives
- List all mounted drives and partitions
- Display device name, mount point, filesystem type
- Show size and usage statistics
- Safe unmount with validation (prevents unmounting system drives)

### Network Drives
- Automatic discovery of SMB shares via Avahi/mDNS
- Automatic discovery of NFS exports
- Mount network shares with authentication support
- Optional persistent mounting

### Safety
- Prevents unmounting critical system mounts (/, /boot, /usr, /var, /home)
- Checks for busy devices before unmounting
- Uses Polkit for privilege escalation
- Validates all operations before execution

## Development Setup

### Prerequisites

- Rust (1.70 or later)
- GTK4 development libraries
- libadwaita development libraries
- Flatpak and flatpak-builder (for Flatpak builds)

#### Fedora/RHEL
```bash
sudo dnf install rust cargo gtk4-devel libadwaita-devel flatpak flatpak-builder
```

#### Ubuntu/Debian
```bash
sudo apt install cargo libgtk-4-dev libadwaita-1-dev flatpak flatpak-builder
```

#### Arch Linux
```bash
sudo pacman -S rust gtk4 libadwaita flatpak flatpak-builder
```

### Building from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/DriveDock.git
cd DriveDock

# Build with cargo
cargo build --release

# Run
cargo run
```

### Building as Flatpak

```bash
# Add GNOME runtime
flatpak install flathub org.gnome.Platform//47 org.gnome.Sdk//47 org.freedesktop.Sdk.Extension.rust-stable

# Build and install
flatpak-builder --user --install --force-clean build-dir org.example.DriveDock.yml

# Run
flatpak run org.example.DriveDock
```

## Architecture

```
src/
├── main.rs           # Application entry point
├── ui/               # User interface
│   ├── mod.rs
│   └── window.rs     # Main application window
├── system/           # Local drive operations
│   ├── mod.rs
│   ├── local.rs      # Drive enumeration
│   └── unmount.rs    # Unmount operations
└── network/          # Network share operations
    ├── mod.rs
    ├── smb.rs        # SMB/CIFS discovery and mounting
    └── nfs.rs        # NFS discovery and mounting
```

## Technology Stack

- **Language**: Rust
- **UI Framework**: GTK4 with libadwaita
- **Async Runtime**: GLib MainContext (GTK-native)
- **Distribution**: Flatpak

## Current Status

**Step 1 Complete** - Foundation established:
- ✅ Rust project structure with GTK4 + libadwaita
- ✅ Main application window with modern GNOME layout
- ✅ Placeholder UI sections (Local Drives, Network Drives, Status)
- ✅ Backend scaffolding with async-ready stub functions
- ✅ Flatpak manifest skeleton
- ✅ Project compiles successfully

**Next Steps**:
- Implement actual drive enumeration from `/proc/mounts`
- Implement SMB/NFS discovery via Avahi
- Add UDisks2 integration for privileged operations
- Implement mounting/unmounting functionality
- Add authentication dialogs for network shares
- Implement fstab persistence

## License

GPL-3.0 - See [LICENSE](LICENSE) file for details

## Contributing

Contributions are welcome! Please ensure:
- Code follows Rust idioms and best practices
- All functions have proper documentation
- Tests are included for new functionality
- UI changes follow GNOME HIG guidelines
