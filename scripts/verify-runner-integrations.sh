#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$ROOT_DIR/scripts"

"$SCRIPT_DIR/verify-nextest-integration.sh"
"$SCRIPT_DIR/verify-go-integration.sh"
"$SCRIPT_DIR/verify-npm-tap-integration.sh"
"$SCRIPT_DIR/verify-bun-integration.sh"
"$SCRIPT_DIR/verify-jest-integration.sh"
"$SCRIPT_DIR/verify-vitest-integration.sh"
"$SCRIPT_DIR/verify-pytest-integration.sh"
"$SCRIPT_DIR/verify-unittest-integration.sh"

echo "Runner integration checks passed."
