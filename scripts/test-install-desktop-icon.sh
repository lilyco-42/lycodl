#!/usr/bin/env bash
# Isolated-HOME regression test for install-desktop-icon.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
FAKE_HOME="$TMP/home"
mkdir -p "$FAKE_HOME"

install() {
  HOME="$FAKE_HOME" \
    XDG_DATA_HOME="$FAKE_HOME/.local/share" \
    LYCODL_BIN_DIR="$FAKE_HOME/.local/bin" \
    LYCODL_URL="http://127.0.0.1:19999" \
    "$ROOT/scripts/install-desktop-icon.sh"
}
install > "$TMP/install.log"
install > "$TMP/install2.log"   # idempotency

ICON_ROOT="$FAKE_HOME/.local/share/icons/hicolor"
for size in 16 20 24 28 32 40 48 64 96 128 160 256 512 1024; do
  png="$ICON_ROOT/${size}x${size}/apps/lycodl.png"
  [[ -s "$png" ]] || { echo "missing $png" >&2; exit 1; }
  file "$png" | grep -q "PNG image data, ${size} x ${size}" || {
    echo "bad PNG: $(file "$png")" >&2
    exit 1
  }
done
cmp -s "$ROOT/web/public/app-icon.svg" "$ICON_ROOT/scalable/apps/lycodl.svg" || {
  echo "scalable SVG differs from source" >&2
  exit 1
}
desktop="$FAKE_HOME/.local/share/applications/lycodl.desktop"
grep -q '^Icon=lycodl$' "$desktop" || { echo "Icon missing" >&2; exit 1; }
grep -q "^Exec=$FAKE_HOME/.local/bin/lycodl-app$" "$desktop" || { echo "Exec missing" >&2; exit 1; }
launcher="$FAKE_HOME/.local/bin/lycodl-app"
[[ -x "$launcher" ]] || { echo "launcher not executable" >&2; exit 1; }
grep -q 'http://127.0.0.1:19999' "$launcher" || { echo "URL not rendered" >&2; exit 1; }
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$desktop"
fi
echo DESKTOP_ICON_TEST_OK
