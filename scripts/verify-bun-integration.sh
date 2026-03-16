#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAPCUE_BIN_DEFAULT="$ROOT_DIR/target/debug/tapcue"
TAPCUE_BIN="${TAPCUE_BIN:-$TAPCUE_BIN_DEFAULT}"
FIXTURE_DIR="$ROOT_DIR/tests/runner-fixtures/bun-default"

if ! command -v bun >/dev/null 2>&1; then
  echo "bun is required for this integration check."
  exit 1
fi

if [[ ! -x "$TAPCUE_BIN" ]]; then
  if [[ "$TAPCUE_BIN" != "$TAPCUE_BIN_DEFAULT" ]]; then
    echo "TAPCUE_BIN is set but not executable: $TAPCUE_BIN"
    exit 1
  fi
  cargo build --locked
fi

tmp_file="$(mktemp)"
trap 'rm -f "$tmp_file"' EXIT

set +e
bun test "$FIXTURE_DIR" >"$tmp_file" 2>&1
runner_status=$?
set -e

if [[ $runner_status -eq 0 ]]; then
  echo "Expected failing status from bun fixture."
  exit 1
fi

if ! grep -Eq '\(fail\)|[[:space:]]fail' "$tmp_file"; then
  echo "Expected failure markers in bun output."
  exit 1
fi

set +e
"$TAPCUE_BIN" --format auto --no-notify --run-output off run -- bun test "$FIXTURE_DIR"
tapcue_status=$?
set -e

if [[ $tapcue_status -eq 0 ]]; then
  echo "Expected tapcue to report failure for failing bun fixture."
  exit 1
fi
