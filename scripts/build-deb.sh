#!/usr/bin/env bash
# Builds a .deb into target/deb/. yt-dlp and deno are not Debian-packaged in a usable version,
# so they are listed as recommendations and the app looks for them in ~/.local/bin and PATH.
set -euo pipefail
cd "$(dirname "$0")/.."
low() { if command -v arkaplan >/dev/null; then arkaplan "$@"; else nice -n 10 "$@"; fi; }
low cargo build --release -j4 -p velour

version=$(grep -m1 '^version' crates/app/Cargo.toml | cut -d'"' -f2)
arch=$(dpkg --print-architecture)
root=target/deb/velour_${version}_${arch}
rm -rf "$root"
install -Dm755 target/release/velour "$root/usr/bin/velour"
install -Dm644 assets/io.github.mikailustuner.Velour.desktop "$root/usr/share/applications/io.github.mikailustuner.Velour.desktop"
install -Dm644 assets/io.github.mikailustuner.Velour.svg "$root/usr/share/icons/hicolor/scalable/apps/io.github.mikailustuner.Velour.svg"
for f in assets/fonts/*.ttf; do install -Dm644 "$f" "$root/usr/share/fonts/truetype/velour/$(basename "$f")"; done
install -Dm644 assets/fonts/OFL-PlusJakartaSans.txt "$root/usr/share/doc/velour/OFL-PlusJakartaSans.txt"
for f in LICENSE DISCLAIMER.md THIRD_PARTY_NOTICES.md README.md; do install -Dm644 "$f" "$root/usr/share/doc/velour/$f"; done
mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<CTRL
Package: velour
Version: $version
Architecture: $arch
Maintainer: mikailustuner <228338090+mikailustuner@users.noreply.github.com>
Depends: libgtk-4-1 (>= 4.20), libmpv2, ffmpeg
Recommends: yt-dlp
Section: sound
Priority: optional
Description: Lightweight desktop player for YouTube Music (unofficial)
 Native GTK4 client for YouTube Music. Streams through libmpv; needs a recent
 yt-dlp plus a JavaScript runtime (deno) for stream URLs.
CTRL
dpkg-deb --root-owner-group --build "$root" >/dev/null
echo "Built: $root.deb"
