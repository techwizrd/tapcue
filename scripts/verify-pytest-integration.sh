#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAPCUE_BIN_DEFAULT="$ROOT_DIR/target/debug/tapcue"
TAPCUE_BIN="${TAPCUE_BIN:-$TAPCUE_BIN_DEFAULT}"
FIXTURE_DIR="$ROOT_DIR/tests/runner-fixtures/python-pytest"

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
PYTHONDONTWRITEBYTECODE=1 "$tmp_dir/venv/bin/python" -m pytest "$FIXTURE_DIR" >"$tmp_file"
runner_status=$?
set -e

if [[ $runner_status -eq 0 ]]; then
  echo "Expected failing status from pytest fixture."
  exit 1
fi

if ! grep -Eqi 'fail|failed' "$tmp_file"; then
  echo "Expected failure markers in pytest output."
  exit 1
fi

set +e
PYTHONDONTWRITEBYTECODE=1 "$TAPCUE_BIN" --format auto --no-notify --run-output off run -- "$tmp_dir/venv/bin/python" -m pytest "$FIXTURE_DIR"
tapcue_status=$?
set -e

if [[ $tapcue_status -eq 0 ]]; then
  echo "Expected tapcue to report failure for failing pytest fixture."
  exit 1
fi
