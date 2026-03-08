#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAPCUE_BIN_DEFAULT="$ROOT_DIR/target/debug/tapcue"
TAPCUE_BIN="${TAPCUE_BIN:-$TAPCUE_BIN_DEFAULT}"
FIXTURE_DIR="$ROOT_DIR/tests/runner-fixtures/npm-tap"

if ! command -v npm >/dev/null 2>&1; then
  echo "npm is required for this integration check."
  exit 1
fi

if [[ ! -x "$TAPCUE_BIN" ]]; then
  if [[ "$TAPCUE_BIN" != "$TAPCUE_BIN_DEFAULT" ]]; then
    echo "TAPCUE_BIN is set but not executable: $TAPCUE_BIN"
    exit 1
  fi
  cargo build --locked
fi
npm ci --prefix "$FIXTURE_DIR" --no-fund --no-audit

tmp_file="$(mktemp)"
trap 'rm -f "$tmp_file"' EXIT

set +e
npm test --prefix "$FIXTURE_DIR" --silent >"$tmp_file"
runner_status=$?
set -e

if [[ $runner_status -eq 0 ]]; then
  echo "Expected failing status from npm TAP fixture."
  exit 1
fi

if ! grep -q '^not ok ' "$tmp_file"; then
  echo "Expected TAP failure line in npm output."
  exit 1
fi

set +e
"$TAPCUE_BIN" --format auto --no-notify --quiet-parse-errors <"$tmp_file"
tapcue_status=$?
set -e

if [[ $tapcue_status -eq 0 ]]; then
  echo "Expected tapcue to report failure for failing npm TAP fixture."
  exit 1
fi
