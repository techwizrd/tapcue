#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

check_doc() {
  local doc_path="$1"

  while IFS= read -r ref; do
    local rel_path="${ref#./}"
    if [[ ! -f "$ROOT_DIR/$rel_path" ]]; then
      echo "Missing file referenced in ${doc_path#"$ROOT_DIR"/}: $ref"
      exit 1
    fi
  done < <(grep -Eo '\./scripts/[[:alnum:]_.-]+\.sh' "$doc_path" | sort -u)
}

check_doc "$ROOT_DIR/README.md"
check_doc "$ROOT_DIR/CONTRIBUTING.md"

echo "Documentation script references are valid."
