#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo-nextest >/dev/null 2>&1; then
  echo "cargo-nextest is required. Install via: cargo install --locked cargo-nextest"
  exit 1
fi

set -o pipefail
env NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run --message-format libtest-json-plus | cargo run -- "$@"
