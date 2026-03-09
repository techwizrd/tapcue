#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAPCUE_BIN_DEFAULT="$ROOT_DIR/target/debug/tapcue"
TAPCUE_BIN="${TAPCUE_BIN:-$TAPCUE_BIN_DEFAULT}"
FIXTURE_DIR="$ROOT_DIR/tests/runner-fixtures/python-unittest"

if ! command -v uv >/dev/null 2>&1; then
  echo "uv is required for this integration check."
  exit 1
fi

if [[ ! -x "$TAPCUE_BIN" ]]; then
  if [[ "$TAPCUE_BIN" != "$TAPCUE_BIN_DEFAULT" ]]; then
    echo "TAPCUE_BIN is set but not executable: $TAPCUE_BIN"
    exit 1
  fi
  cargo build --locked
fi

tmp_dir="$(mktemp -d)"
tmp_file="$(mktemp)"
trap 'rm -rf "$tmp_dir"; rm -f "$tmp_file"' EXIT

UV_LINK_MODE=copy uv venv --quiet --python python3 "$tmp_dir/venv"
UV_LINK_MODE=copy uv pip install --quiet --python "$tmp_dir/venv/bin/python" --require-hashes -r "$FIXTURE_DIR/requirements.lock"

set +e
PYTHONDONTWRITEBYTECODE=1 "$tmp_dir/venv/bin/python" "$FIXTURE_DIR/run_tap.py" >"$tmp_file"
runner_status=$?
set -e

if [[ $runner_status -eq 0 ]]; then
  echo "Expected failing status from unittest fixture."
  exit 1
fi

if ! grep -Eq '^not ok ' "$tmp_file"; then
  echo "Expected TAP failure lines in unittest output."
  exit 1
fi

if ! grep -Eq '^1\.\.[0-9]+' "$tmp_file"; then
  echo "Expected TAP plan line in unittest output."
  exit 1
fi

set +e
"$TAPCUE_BIN" --format tap --no-notify <"$tmp_file"
tapcue_status=$?
set -e

if [[ $tapcue_status -eq 0 ]]; then
  echo "Expected tapcue to report failure for failing unittest fixture."
  exit 1
fi
