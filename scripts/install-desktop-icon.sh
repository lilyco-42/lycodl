#!/usr/bin/env bash
# Install the lycodl desktop icon, launcher and .desktop entry for the current user.
# Source of truth: web/public/app-icon.svg. Safe to run repeatedly.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/web/public/app-icon.svg"
DESKTOP_TEMPLATE="$ROOT/scripts/lycodl.desktop.example"
LAUNCHER_TEMPLATE="$ROOT/scripts/lycodl-app.example"

for f in "$SRC" "$DESKTOP_TEMPLATE" "$LAUNCHER_TEMPLATE"; do
  [[ -f "$f" ]] || { echo "install-desktop-icon: missing $f" >&2; exit 1; }
done
command -v rsvg-convert >/dev/null 2>&1 || {
  echo "install-desktop-icon: rsvg-convert is required (librsvg / librsvg2-bin)" >&2
  exit 1
}

HOME_DIR="${HOME:-}"
[[ -n "$HOME_DIR" ]] || { echo "install-desktop-icon: HOME is required" >&2; exit 1; }
DATA_DIR="${XDG_DATA_HOME:-$HOME_DIR/.local/share}"
ICON_ROOT="$DATA_DIR/icons/hicolor"
APP_DIR="$DATA_DIR/applications"
BIN_DIR="${LYCODL_BIN_DIR:-$HOME_DIR/.local/bin}"
LAUNCHER="$BIN_DIR/lycodl-app"
URL="${LYCODL_URL:-http://127.0.0.1:8766}"

# Replace @LAUNCHER@ / @URL@ without sed: bash parameter expansion keeps & literal.
render() {
  local template="$1" output="$2" launcher="$3" url="$4" line
  : > "$output"
  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line//@LAUNCHER@/$launcher}"
    line="${line//@URL@/$url}"
    printf '%s\n' "$line" >> "$output"
  done < "$template"
}

mkdir -p "$APP_DIR" "$BIN_DIR" "$ICON_ROOT/scalable/apps"
for size in 16 20 24 28 32 40 48 64 96 128 160 256 512 1024; do
  dest="$ICON_ROOT/${size}x${size}/apps"
  mkdir -p "$dest"
  rsvg-convert -w "$size" -h "$size" -o "$dest/lycodl.png" "$SRC"
done
cp -f "$SRC" "$ICON_ROOT/scalable/apps/lycodl.svg"
chmod 0644 "$ICON_ROOT/scalable/apps/lycodl.svg"

# Never overwrite a launcher that already exists on this machine.
if [[ ! -x "$LAUNCHER" ]]; then
  render "$LAUNCHER_TEMPLATE" "$LAUNCHER" "$LAUNCHER" "$URL"
  chmod 0755 "$LAUNCHER"
fi
render "$DESKTOP_TEMPLATE" "$APP_DIR/lycodl.desktop" "$LAUNCHER" "$URL"
chmod 0644 "$APP_DIR/lycodl.desktop"

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$ICON_ROOT" >/dev/null 2>&1 || true
fi
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$APP_DIR/lycodl.desktop" || true
fi

echo "install-desktop-icon: 14 PNG sizes + scalable SVG -> $ICON_ROOT"
echo "install-desktop-icon: $APP_DIR/lycodl.desktop (Icon=lycodl)"
echo "install-desktop-icon: launcher $LAUNCHER"
