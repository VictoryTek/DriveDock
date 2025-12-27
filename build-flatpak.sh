#!/bin/bash
# Build script for DriveDock Flatpak

set -e

echo "Building DriveDock Flatpak..."
echo ""

# Check if cargo-sources.json exists
if [ ! -f "cargo-sources.json" ]; then
    echo "❌ cargo-sources.json not found!"
    echo "Run: python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json"
    exit 1
fi

# Build and install
flatpak-builder --user --install --force-clean build-dir org.example.DriveDock.yml

echo ""
echo "✅ Build complete!"
echo "Run with: flatpak run org.example.DriveDock"
