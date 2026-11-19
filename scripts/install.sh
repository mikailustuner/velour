#!/usr/bin/env bash
# Builds Sonora in release mode and installs it for the current user (no sudo).
# Build runs at low priority (via `arkaplan` when available) with 4 jobs to keep the desktop smooth.
set -euo pipefail
cd "$(dirname "$0")/.."

low() { if command -v arkaplan >/dev/null; then arkaplan "$@"; else nice -n 10 "$@"; fi; }
low cargo build --release -j4 -p sonora

bin="$HOME/.local/bin"; apps="$HOME/.local/share/applications"
icons="$HOME/.local/share/icons/hicolor/scalable/apps"; fonts="$HOME/.local/share/fonts/sonora"
mkdir -p "$bin" "$apps" "$icons" "$fonts"
install -m755 target/release/sonora "$bin/sonora"
install -m644 assets/dev.sonora.Sonora.desktop "$apps/"
install -m644 assets/dev.sonora.Sonora.svg "$icons/"
install -m644 assets/fonts/*.ttf "$fonts/"
fc-cache -f "$fonts" >/dev/null 2>&1 || true
update-desktop-database "$apps" >/dev/null 2>&1 || true
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true

echo "Installed: $bin/sonora"
for dep in yt-dlp deno; do
  command -v "$dep" >/dev/null || echo "warning: $dep not found in PATH (needed for playback)"
done
