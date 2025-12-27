#!/bin/bash
# First-time setup for DriveDock Flatpak development

set -e

echo "🔧 Setting up DriveDock Flatpak development environment..."
echo ""

# Add Flathub if not already added
echo "📦 Ensuring Flathub is added..."
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo

# Install GNOME runtime and SDK
echo ""
echo "📥 Installing GNOME Platform and SDK (this may take a few minutes)..."
flatpak install --user -y flathub org.gnome.Platform//47 org.gnome.Sdk//47

# Install Rust SDK extension
echo ""
echo "🦀 Installing Rust SDK extension..."
flatpak install --user -y flathub org.freedesktop.Sdk.Extension.rust-stable//24.08

# Install Python dependencies for cargo generator
echo ""
echo "🐍 Checking Python dependencies..."
if ! python3 -c "import aiohttp, tomlkit" 2>/dev/null; then
    echo "Installing Python dependencies (aiohttp, tomlkit)..."
    pip3 install --user --break-system-packages aiohttp tomlkit 2>/dev/null || \
    pip3 install --user aiohttp tomlkit
fi

# Download cargo generator if needed
if [ ! -f "flatpak-cargo-generator.py" ]; then
    echo ""
    echo "📥 Downloading flatpak-cargo-generator.py..."
    curl -L -o flatpak-cargo-generator.py \
        https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
    chmod +x flatpak-cargo-generator.py
fi

# Generate cargo sources if needed
if [ ! -f "cargo-sources.json" ]; then
    echo ""
    echo "🔨 Generating cargo-sources.json..."
    python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json
fi

echo ""
echo "✅ Setup complete!"
echo ""
echo "Next steps:"
echo "  1. Build: ./build-flatpak.sh"
echo "  2. Run:   flatpak run org.example.DriveDock"
echo ""
echo "Or test locally without Flatpak:"
echo "  cargo build --release"
echo "  ./target/release/drivedock"
