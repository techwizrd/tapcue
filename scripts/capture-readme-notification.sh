#!/bin/sh

set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
OUT_DIR="$ROOT/docs/media"
FULL_OUT="$OUT_DIR/tapcue-notification-full.png"
CROP_OUT="$OUT_DIR/tapcue-notification.png"

mkdir -p "$OUT_DIR"

# Trigger a real tapcue notification, then ask the GNOME portal for a screenshot.
sh -lc '"$1" >/dev/null 2>&1 || true & sleep 1; gdbus call --session --dest org.freedesktop.portal.Desktop --object-path /org/freedesktop/portal/desktop --method org.freedesktop.portal.Screenshot.Screenshot "" "{\"interactive\": <false>, \"handle_token\": <\"tapcuecap2\">}" >/dev/null; sleep 2' sh "$ROOT/scripts/show-readme-notification.sh"

LATEST=$(ls -t "$HOME"/Pictures/Screenshot*.png 2>/dev/null | head -1)

if [ -z "${LATEST:-}" ]; then
  echo "no portal screenshot found in $HOME/Pictures" >&2
  exit 1
fi

cp "$LATEST" "$FULL_OUT"

python3 - "$FULL_OUT" "$CROP_OUT" <<'PY'
from PIL import Image
import sys

source, target = sys.argv[1:3]
image = Image.open(source)
width, height = image.size

# GNOME notification banners appear near the top center. Crop generously so the
# banner reads well in the README while staying tied to the real screenshot.
crop_width = min(760, width)
crop_height = min(170, height)
left = max((width - crop_width) // 2, 0)
top = 36
right = min(left + crop_width, width)
bottom = min(top + crop_height, height)

image.crop((left, top, right, bottom)).save(target)
PY

printf 'wrote %s and %s\n' "$FULL_OUT" "$CROP_OUT"
