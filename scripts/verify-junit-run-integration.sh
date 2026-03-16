#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAPCUE_BIN_DEFAULT="$ROOT_DIR/target/debug/tapcue"
TAPCUE_BIN="${TAPCUE_BIN:-$TAPCUE_BIN_DEFAULT}"

if [[ ! -x "$TAPCUE_BIN" ]]; then
  if [[ "$TAPCUE_BIN" != "$TAPCUE_BIN_DEFAULT" ]]; then
    echo "TAPCUE_BIN is set but not executable: $TAPCUE_BIN"
    exit 1
  fi
  cargo build --locked
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

cat >"$tmp_dir/gradlew" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

mkdir -p build/test-results/test

rerun=0
for arg in "$@"; do
  if [[ "$arg" == "--rerun-tasks" ]]; then
    rerun=1
    break
  fi
done

if [[ $rerun -eq 1 ]]; then
  cat > build/test-results/test/TEST-sample.xml <<'XML'
<testsuite name="sample-suite">
  <testcase classname="sample.MathTest" name="ok-one" />
  <testcase classname="sample.MathTest" name="ok-two" />
</testsuite>
XML
  echo "BUILD SUCCESSFUL in 2s"
  echo "61 actionable tasks: 61 executed"
else
  echo "BUILD SUCCESSFUL in 1s"
  echo "61 actionable tasks: 1 executed, 60 up-to-date"
fi
EOF

chmod +x "$tmp_dir/gradlew"

pushd "$tmp_dir" >/dev/null

fresh_out="$($TAPCUE_BIN --no-notify --summary-format text --summary-file - run -- ./gradlew --rerun-tasks testDebugUnitTest 2>&1)"
if ! grep -q "status=success" <<<"$fresh_out"; then
  echo "Expected success summary for fresh inferred JUnit run."
  echo "$fresh_out"
  exit 1
fi
if ! grep -q "total=2" <<<"$fresh_out"; then
  echo "Expected fresh inferred JUnit summary to include total=2."
  echo "$fresh_out"
  exit 1
fi

sleep 3

stale_inferred_out="$($TAPCUE_BIN --no-notify --summary-format text --summary-file - run -- ./gradlew testDebugUnitTest 2>&1)"
if grep -q "status=" <<<"$stale_inferred_out"; then
  echo "Did not expect summary for stale inferred JUnit reports."
  echo "$stale_inferred_out"
  exit 1
fi

stale_explicit_out="$($TAPCUE_BIN --no-notify --summary-format text --summary-file - --junit-dir build/test-results --junit-only run -- ./gradlew testDebugUnitTest 2>&1)"
if grep -q "status=" <<<"$stale_explicit_out"; then
  echo "Did not expect summary for stale explicit JUnit reports."
  echo "$stale_explicit_out"
  exit 1
fi

popd >/dev/null
