#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAPCUE_BIN_DEFAULT="$ROOT_DIR/target/debug/tapcue"
TAPCUE_BIN="${TAPCUE_BIN:-$TAPCUE_BIN_DEFAULT}"

if ! command -v go >/dev/null 2>&1; then
  echo "Go is required for this integration check."
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
go -C "$ROOT_DIR/tests/runner-fixtures/go-sample" test -json ./... >"$tmp_file"
runner_status=$?
set -e

if [[ $runner_status -eq 0 ]]; then
  echo "Expected failing status from go fixture."
  exit 1
fi

if ! grep -q '"Action":"fail"' "$tmp_file"; then
  echo "Expected go JSON fail action in output."
  exit 1
fi

set +e
"$TAPCUE_BIN" --format auto --no-notify --run-output off run -- go -C "$ROOT_DIR/tests/runner-fixtures/go-sample" test ./...
tapcue_status=$?
set -e

if [[ $tapcue_status -eq 0 ]]; then
  echo "Expected tapcue to report failure for failing go fixture."
  exit 1
fi
