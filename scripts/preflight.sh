#!/usr/bin/env bash
# Preflight validation for DriveDock.
#
# Runs cargo check, cargo build, and cargo test only. Never invokes
# `flatpak install`, `flatpak-builder --install`, or any command that
# mutates system state (see CLAUDE.md FORBIDDEN COMMANDS).
#
# On NixOS/Nix systems where GTK4/libadwaita pkg-config files aren't on
# the default PATH, falls back to a read-only `nix-shell` to provide
# them for the duration of the build.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

run() {
    echo "+ $*"
    "$@"
}

if command -v pkg-config >/dev/null 2>&1 && pkg-config --exists gtk4 libadwaita glib-2.0 2>/dev/null; then
    RUNNER=(run)
else
    echo "GTK4/libadwaita pkg-config files not found natively — using nix-shell"
    RUNNER=(nix-shell -p pkg-config gtk4 libadwaita glib --run)
fi

echo "== cargo check =="
if [ "${RUNNER[0]}" = "run" ]; then
    cargo check
else
    "${RUNNER[@]}" "cargo check"
fi

echo "== cargo build =="
if [ "${RUNNER[0]}" = "run" ]; then
    cargo build
else
    "${RUNNER[@]}" "cargo build"
fi

echo "== cargo test =="
if [ "${RUNNER[0]}" = "run" ]; then
    cargo test
else
    "${RUNNER[@]}" "cargo test"
fi

echo "== preflight: all checks passed =="
