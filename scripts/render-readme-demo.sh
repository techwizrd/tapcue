#!/bin/sh

set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BIN="$ROOT/target/release/tapcue"

if [ ! -x "$BIN" ]; then
  echo "expected built binary at $BIN" >&2
  exit 1
fi

pause() {
  sleep "$1"
}

run_demo() {
  label=$1
  display=$2
  shift 2

  printf '\033[1;36m# %s\033[0m\n' "$label"
  printf '\033[1;32m$\033[0m %s\n' "$display"
  pause 0.8
  set +e
  "$@"
  status=$?
  set -e
  printf '\033[2m(exit %s)\033[0m\n' "$status"
  pause 1.2
  printf '\n'
}

run_demo \
  "JSON stream auto-detection" \
  "./target/release/tapcue --no-notify --trace-detection --summary-format text --summary-file - < tests/fixtures/go_test_json.ndjson" \
  "$BIN" --no-notify --trace-detection --summary-format text --summary-file - \
  < "$ROOT/tests/fixtures/go_test_json.ndjson"

run_demo \
  "TAP auto-detection" \
  "./target/release/tapcue --no-notify --trace-detection --summary-format text --summary-file - < tests/fixtures/failing.tap" \
  "$BIN" --no-notify --trace-detection --summary-format text --summary-file - \
  < "$ROOT/tests/fixtures/failing.tap"

run_demo \
  "JUnit XML ingestion" \
  "./target/release/tapcue --no-notify --summary-format text --summary-file - --junit-file tests/fixtures/junit_failure.xml --junit-only" \
  "$BIN" --no-notify --summary-format text --summary-file - \
  --junit-file "$ROOT/tests/fixtures/junit_failure.xml" --junit-only
