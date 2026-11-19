# Sonora

A lightweight, native YouTube Music player for Linux (GTK4 + Rust + libmpv).
Built for Hyprland/Wayland; works on any desktop with GTK 4.20+.

## Features

- Home, Explore (new releases, charts, moods), search, artists, albums, playlists
- Library (playlists, albums, artists), Liked Music, History, offline Downloads
- Premium audio (256 kbps Opus/AAC when the account has Premium), gapless playback,
  queue with drag-to-reorder, shuffle/repeat, endless radio
- Synced lyrics (LRCLIB) with YouTube Music lyrics as fallback
- Like/unlike, add to/remove from playlists, create/rename/delete playlists, save albums
- MPRIS (media keys, `playerctl`), keeps playing when the window is closed, single instance
- Optional: notifications, tray icon, Discord status, equalizer presets
- Session restore (queue + position), dark/light themes in the Tiphys style

## Requirements

- GTK ≥ 4.20, libmpv, ffmpeg (Debian: `libgtk-4-1 libmpv2 ffmpeg`)
- **yt-dlp** (recent, self-updating binary recommended) and **deno** in `~/.local/bin` or `PATH`
- Signed in to music.youtube.com in **Firefox** (Sonora reads the session cookies; nothing is sent anywhere else)

Keep yt-dlp current: `yt-dlp -U`. When playback suddenly fails, that is almost always the fix.

## Install

```bash
./scripts/install.sh      # user install into ~/.local (no sudo)
./scripts/build-deb.sh    # or build target/deb/sonora_<version>_amd64.deb
```

Builds run at low priority (`arkaplan` when available, otherwise `nice`) with 4 jobs.

## Keyboard

Space play/pause · Ctrl+←/→ previous/next · Shift+←/→ seek 10 s · Ctrl+↑/↓ volume · M mute ·
Ctrl+L like · Ctrl+F search · Alt+← back · Ctrl+, settings · Ctrl+Q quit.
Double-click a song to play; right-click (or ⋯) for more.

Hyprland media keys work through MPRIS, e.g. `bind = , XF86AudioPlay, exec, playerctl play-pause`.

## Resource use (measured on GTX 1050 Ti, release build)

| | GPU rendering (default) | Low memory mode (software) |
|---|---|---|
| Memory (private) | ~250 MB (mostly NVIDIA driver) | ~90 MB |
| CPU idle | ~0.4 % of one core | ~0 % |
| CPU playing | ~2 % | ~3.3 % |
| CPU scrolling a cover grid | ~20 % | ~175 % |

Low memory mode is in Settings (applies after restart). Each new song briefly runs yt-dlp
(~4 s, low priority); the next song is resolved ahead of time so transitions are gapless.

## Development

```bash
cargo test --workspace                      # unit + parser tests
cargo run -p sonora-ytm --example dump      # refresh parser fixtures (personal data, gitignored)
cargo run -p sonora-ytm --example live      # exercise live endpoints
SONORA_AO=null cargo run -p sonora          # run without audio output
```

Layout: `crates/ytm` (InnerTube client + parsers), `crates/player` (libmpv + yt-dlp resolver),
`crates/app` (GTK UI). Design notes: `docs/design.md`; plan: `docs/roadmap.md`.

YouTube Music has no public API; Sonora uses the same internal API as the web app. That is
against YouTube's terms of service, so use it for your own account at your own risk.

## Licenses

Plus Jakarta Sans: SIL OFL 1.1 (`assets/fonts/OFL-PlusJakartaSans.txt`).
Material Symbols icons: Apache 2.0.
