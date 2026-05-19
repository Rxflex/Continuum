# Build Continuum from this checkout and install it onto your PATH.
#
# No registry, no account, no remote -- just a local Rust toolchain. The
# binaries land in ~/.cargo/bin, side by side, where the adapter expects to
# find the daemon.

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent

Write-Host "Installing continuum-daemon ..."
cargo install --path (Join-Path $root "crates\continuum-daemon") --locked --force

Write-Host "Installing continuum-adapter ..."
cargo install --path (Join-Path $root "crates\continuum-adapter") --locked --force

Write-Host ""
Write-Host "Installed continuum-adapter and continuum-daemon to ~/.cargo/bin"
Write-Host "Point your MCP agent at:  continuum-adapter"
Write-Host "(ensure ~/.cargo/bin is on your PATH, or use the full path)"
