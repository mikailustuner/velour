#!/usr/bin/env bash
# Builds a .deb into target/deb/. yt-dlp and deno are not Debian-packaged in a usable version,
# so they are listed as recommendations and the app looks for them in ~/.local/bin and PATH.
set -euo pipefail
cd "$(dirname "$0")/.."
low() { if command -v arkaplan >/dev/null; then arkaplan "$@"; else nice -n 10 "$@"; fi; }
low cargo build --release -j4 -p sonora

version=$(grep -m1 '^version' crates/app/Cargo.toml | cut -d'"' -f2)
arch=$(dpkg --print-architecture)
root=target/deb/sonora_${version}_${arch}
rm -rf "$root"
install -Dm755 target/release/sonora "$root/usr/bin/sonora"
install -Dm644 assets/dev.sonora.Sonora.desktop "$root/usr/share/applications/dev.sonora.Sonora.desktop"
install -Dm644 assets/dev.sonora.Sonora.svg "$root/usr/share/icons/hicolor/scalable/apps/dev.sonora.Sonora.svg"
for f in assets/fonts/*.ttf; do install -Dm644 "$f" "$root/usr/share/fonts/truetype/sonora/$(basename "$f")"; done
install -Dm644 assets/fonts/OFL-PlusJakartaSans.txt "$root/usr/share/doc/sonora/OFL-PlusJakartaSans.txt"
mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<CTRL
Package: sonora
Version: $version
Architecture: $arch
Maintainer: Sonora <sonora@localhost>
Depends: libgtk-4-1 (>= 4.20), libmpv2, ffmpeg
Recommends: yt-dlp
Section: sound
Priority: optional
Description: Lightweight YouTube Music player for Linux
 Native GTK4 client for YouTube Music. Streams through libmpv; needs a recent
 yt-dlp plus a JavaScript runtime (deno) for stream URLs.
CTRL
dpkg-deb --root-owner-group --build "$root" >/dev/null
echo "Built: $root.deb"
