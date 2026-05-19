#!/usr/bin/env sh
# Build Continuum from this checkout and install it onto your PATH.
#
# No registry, no account, no remote -- just a local Rust toolchain. The
# binaries land in ~/.cargo/bin, side by side, where the adapter expects to
# find the daemon.

set -e

root="$(cd "$(dirname "$0")/.." && pwd)"

echo "Installing continuum-daemon ..."
cargo install --path "$root/crates/continuum-daemon" --locked --force

echo "Installing continuum-adapter ..."
cargo install --path "$root/crates/continuum-adapter" --locked --force

echo
echo "Installed continuum-adapter and continuum-daemon to ~/.cargo/bin"
echo "Point your MCP agent at:  continuum-adapter"
echo "(ensure ~/.cargo/bin is on your PATH, or use the full path)"
