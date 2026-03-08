#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAPCUE_BIN_DEFAULT="$ROOT_DIR/target/debug/tapcue"
TAPCUE_BIN="${TAPCUE_BIN:-$TAPCUE_BIN_DEFAULT}"

if ! command -v cargo-nextest >/dev/null 2>&1; then
  echo "cargo-nextest is required. Install via: cargo install --locked cargo-nextest"
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
NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run --manifest-path "$ROOT_DIR/tests/runner-fixtures/rust-nextest/Cargo.toml" --message-format libtest-json-plus >"$tmp_file"
runner_status=$?
set -e

if [[ $runner_status -eq 0 ]]; then
  echo "Expected failing status from rust-nextest fixture."
  exit 1
fi

if ! grep -q '"type":"test"' "$tmp_file"; then
  echo "Expected JSON test events from nextest."
  exit 1
fi

set +e
"$TAPCUE_BIN" --format json --no-notify <"$tmp_file"
tapcue_status=$?
set -e

if [[ $tapcue_status -eq 0 ]]; then
  echo "Expected tapcue to report failure for failing nextest fixture."
  exit 1
fi
