#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAPCUE_BIN_DEFAULT="$ROOT_DIR/target/debug/tapcue"
TAPCUE_BIN="${TAPCUE_BIN:-$TAPCUE_BIN_DEFAULT}"
FIXTURE_DIR="$ROOT_DIR/tests/runner-fixtures/jest-json"

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
npm run --prefix "$FIXTURE_DIR" test:json --silent >"$tmp_file"
runner_status=$?
set -e

if [[ $runner_status -eq 0 ]]; then
  echo "Expected failing status from jest fixture."
  exit 1
fi

if ! grep -q '"numFailedTests"' "$tmp_file"; then
  echo "Expected Jest JSON summary in output."
  exit 1
fi

set +e
"$TAPCUE_BIN" --format json --no-notify --run-output off run -- npm run --prefix "$FIXTURE_DIR" test:json --silent
tapcue_status=$?
set -e

if [[ $tapcue_status -eq 0 ]]; then
  echo "Expected tapcue to report failure for failing jest fixture."
  exit 1
fi
