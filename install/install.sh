#!/usr/bin/env bash
# lycodl one-line installer for Linux and macOS.
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/lilyco-42/lycodl/master/install/install.sh | bash
#   bash install.sh v0.1.1
set -euo pipefail

VERSION="${1:-latest}"
REPO="lilyco-42/lycodl"
HOME_DIR="${HOME:-$(getent passwd "$(id -u)" | cut -d: -f6)}"
BIN_DIR="${LYCODL_BIN_DIR:-$HOME_DIR/.local/bin}"
OPT_DIR="${LYCODL_OPT_DIR:-$HOME_DIR/.local/opt}"

log() { printf '%s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }

case "$(uname -s)" in
  Linux) os="linux" ;;
  Darwin) os="macos" ;;
  *) die "unsupported OS: $(uname -s) (Windows: use install.ps1)" ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) arch="x64" ;;
  aarch64 | arm64) arch="aarch64" ;;
  *) die "unsupported arch: $(uname -m)" ;;
esac

if [ "$os" = "macos" ]; then
  asset="lycodl-macos-universal.tar.gz"
  folder="lycodl-macos-universal"
elif [ "$arch" = "x64" ]; then
  asset="lycodl-linux-x64.tar.gz"
  folder="lycodl-linux-x64"
else
  asset="lycodl-linux-aarch64.tar.gz"
  folder="lycodl-linux-aarch64"
fi

if [ "$VERSION" = "latest" ]; then
  url="https://github.com/$REPO/releases/latest/download/$asset"
else
  url="https://github.com/$REPO/releases/download/$VERSION/$asset"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

log "downloading $url"
curl -fL --retry 3 --retry-delay 2 "$url" -o "$tmp/$asset"

log "installing into $OPT_DIR/lycodl"
mkdir -p "$OPT_DIR" "$BIN_DIR"
rm -rf "$OPT_DIR/lycodl"
tar -C "$tmp" -xzf "$tmp/$asset"
mv "$tmp/$folder" "$OPT_DIR/lycodl"

ln -sf "$OPT_DIR/lycodl/lycodl" "$BIN_DIR/lycodl"
log "linked $BIN_DIR/lycodl"

if ! command -v node >/dev/null 2>&1; then
  log "warn: Node.js not found - download works, but multi-platform publishing needs Node 18+"
fi

log "ok: $("$BIN_DIR/lycodl" --version 2>/dev/null | head -1 || true)"
log "run: lycodl --web    (or: $BIN_DIR/lycodl --web)"