#!/usr/bin/env bash
set -euo pipefail

echo "Installing cargo-nextest (locked)..."
cargo install --locked cargo-nextest

echo "Done. Installed required Rust dev tool: cargo-nextest"
echo "Optional external toolchains for integration checks: Go and Node.js/npm"
