#!/bin/sh

set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BIN="$ROOT/target/release/tapcue"

if [ ! -x "$BIN" ]; then
  echo "expected built binary at $BIN" >&2
  exit 1
fi

exec "$BIN" \
  --desktop force-on \
  --project-label tapcue \
  --max-failure-notifications 1 \
  < "$ROOT/tests/fixtures/failing.tap"
