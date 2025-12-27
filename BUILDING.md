# Building DriveDock

## Prerequisites Note

**Important**: DriveDock requires GTK4 and libadwaita development libraries to compile.

If you see errors like:
```
The system library `glib-2.0` required by crate `glib-sys` was not found.
The system library `gobject-2.0` required by crate `gobject-sys` was not found.
```

You need to install the GTK4 development packages:

### Installation Instructions

#### Fedora / RHEL / CentOS
```bash
sudo dnf install gtk4-devel libadwaita-devel glib2-devel cairo-devel pango-devel gdk-pixbuf2-devel
```

#### Ubuntu / Debian
```bash
sudo apt install libgtk-4-dev libadwaita-1-dev libglib2.0-dev libcairo2-dev libpango1.0-dev libgdk-pixbuf-2.0-dev
```

#### Arch Linux
```bash
sudo pacman -S gtk4 libadwaita glib2 cairo pango gdk-pixbuf2
```

#### openSUSE
```bash
sudo zypper install gtk4-devel libadwaita-devel glib2-devel cairo-devel pango-devel gdk-pixbuf-devel
```

### Alternative: Use Flatpak Builder

If you don't want to install system dependencies, you can build directly as a Flatpak:

```bash
# Add GNOME runtime (includes all GTK dependencies)
flatpak install flathub org.gnome.Platform//47 org.gnome.Sdk//47 org.freedesktop.Sdk.Extension.rust-stable

# Build using Flatpak (no system dependencies needed)
flatpak-builder --user --install --force-clean build-dir org.example.DriveDock.yml

# Run
flatpak run org.example.DriveDock
```

This is the **recommended approach** for development as it:
- Provides a consistent build environment
- Matches the production deployment
- Doesn't require installing system-wide development packages
- Automatically handles all dependencies

## Development Workflow

### Option 1: Native Build (requires system libraries)
```bash
cargo build --release
cargo run
```

### Option 2: Flatpak Build (recommended)
```bash
# Initial setup (one time)
flatpak install flathub org.gnome.Platform//47 org.gnome.Sdk//47 org.freedesktop.Sdk.Extension.rust-stable

# Build and test
flatpak-builder --user --install --force-clean build-dir org.example.DriveDock.yml
flatpak run org.example.DriveDock

# Rebuild after changes
flatpak-builder --user --install --force-clean build-dir org.example.DriveDock.yml
```

### Option 3: Container Build
```bash
# Use toolbx or distrobox for isolated development
toolbox create --distro fedora drivedock-dev
toolbox enter drivedock-dev
sudo dnf install gtk4-devel libadwaita-devel rust cargo
cd /path/to/DriveDock
cargo build
```

## Code Structure

The project follows a clean modular architecture:

```
src/
├── main.rs           # Entry point, GTK application setup
├── ui/
│   ├── mod.rs
│   └── window.rs     # Main window with 3 sections (Local/Network/Status)
├── system/
│   ├── mod.rs
│   ├── local.rs      # Drive enumeration from /proc/mounts
│   └── unmount.rs    # Safe unmount with validation
└── network/
    ├── mod.rs
    ├── smb.rs        # SMB/CIFS discovery (Avahi + smbclient)
    └── nfs.rs        # NFS discovery (Avahi + showmount)
```

All backend functions are async-ready stub implementations with detailed TODO comments explaining the full implementation strategy.

## Next Implementation Steps

1. **Drive Enumeration** (system/local.rs)
   - Parse /proc/mounts
   - Use statvfs for size/usage
   - Filter pseudo-filesystems

2. **SMB Discovery** (network/smb.rs)
   - D-Bus Avahi integration
   - smbclient querying
   - GIO/GVFS integration

3. **NFS Discovery** (network/nfs.rs)
   - Avahi _nfs._tcp browsing
   - showmount -e querying
   - NFSv3/v4 detection

4. **Mount Operations**
   - UDisks2 D-Bus integration
   - Polkit authorization
   - Credential management

5. **UI Improvements**
   - Dynamic drive list updates
   - Authentication dialogs
   - Progress indicators
   - Error toasts

## Testing

Currently, the project includes unit tests for utility functions:

```bash
cargo test
```

Integration tests will be added as functionality is implemented.
