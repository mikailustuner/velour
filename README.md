<p align="center"><img src="assets/io.github.mikailustuner.Velour.svg" width="96" alt="Velour icon"></p>

<h1 align="center">Velour</h1>

<p align="center">A lightweight, native desktop player for YouTube Music on Linux — GTK4, Rust and libmpv.<br>
<b>Unofficial.</b> Not affiliated with YouTube or Google. See <a href="DISCLAIMER.md">DISCLAIMER</a>.</p>

<p align="center"><img src="docs/screenshots/now-playing.png" alt="Now playing view with synced lyrics" width="900"></p>

## Why

Most desktop YouTube Music apps wrap the website in Electron and use hundreds of megabytes and a
steady stream of CPU. Velour is a native app: its own UI, the YouTube Music data your account
already sees, and audio through libmpv.

## Features

- **Browse** — Home, Explore (new releases, charts, moods), search with suggestions, artists,
  albums, playlists, Library (playlists/albums/artists), Liked Music, History
- **Play** — gapless playback, Premium-quality audio when your account has Premium, queue with
  drag-to-reorder, shuffle/repeat, endless radio, session restore
- **Now playing** — full-size view with a glow taken from the cover art; synced lyrics (LRCLIB)
  with YouTube Music lyrics as fallback
- **Your library** — like/unlike, add/remove songs, create/rename/delete playlists, save albums,
  save the queue as a playlist; filter and sort long playlists
- **Desktop** — MPRIS (media keys, `playerctl`), keeps playing when closed, single instance,
  optional notifications, tray icon and Discord status, equalizer presets
- **Fast** — cached pages open instantly, songs are prepared when you hover them and the next song
  is resolved ahead of time
- Sign in with **Firefox, Chrome, Chromium, Brave, Edge, Vivaldi or Opera**

| | |
|---|---|
| ![Explore](docs/screenshots/explore.png) | ![Artist](docs/screenshots/artist.png) |
| ![Search](docs/screenshots/search.png) | ![Light theme](docs/screenshots/light.png) |

## Requirements

- Linux with **GTK ≥ 4.20**, **libmpv** and **ffmpeg** (Debian/Ubuntu: `libgtk-4-1 libmpv2 ffmpeg`)
- **[yt-dlp](https://github.com/yt-dlp/yt-dlp)** (recent; the self-updating binary is recommended)
  and a JavaScript runtime, **[deno](https://deno.com)**, in `~/.local/bin` or `PATH`
- A browser where you're signed in to music.youtube.com

### Installing yt-dlp and deno

Velour needs a **current** yt-dlp; distribution packages are often months old and stop working
with YouTube. Install the official self-updating binary and deno into `~/.local/bin` (no sudo):

```bash
mkdir -p ~/.local/bin

# yt-dlp (standalone Linux binary; includes everything it needs)
curl -L -o ~/.local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux
chmod +x ~/.local/bin/yt-dlp

# deno (JavaScript runtime yt-dlp uses for YouTube)
curl -L -o /tmp/deno.zip https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip
unzip -o /tmp/deno.zip -d ~/.local/bin && rm /tmp/deno.zip

# check (open a new terminal first if ~/.local/bin was just created)
yt-dlp --version && deno --version
```

If `~/.local/bin` is not in your `PATH`, add `export PATH="$HOME/.local/bin:$PATH"` to your shell
profile. On ARM64 use `yt-dlp_linux_aarch64` and `deno-aarch64-unknown-linux-gnu.zip`.

Velour checks both on start and tells you if something is missing or outdated. When songs
suddenly stop playing, update yt-dlp: Settings → Playback helpers → Update, or `yt-dlp -U`.

## Install

### Option 1: .deb package (Debian testing/sid)

Download the latest `velour_*_amd64.deb` from
[Releases](https://github.com/mikailustuner/velour/releases/latest), then:

```bash
sudo apt install ./velour_*_amd64.deb
```

Then install yt-dlp and deno as shown above. The package is built on Debian sid; on other
distributions (or if apt reports missing dependencies) use option 2.

### Option 2: build from source

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libmpv-dev ffmpeg   # Debian/Ubuntu
curl -fsSL https://sh.rustup.rs | sh                                         # if Rust isn't installed
git clone https://github.com/mikailustuner/velour && cd velour
./scripts/install.sh        # builds and installs into ~/.local (no sudo)
```

`./scripts/build-deb.sh` builds the .deb instead (`target/deb/`). Builds run at low CPU priority
with 4 jobs so the desktop stays responsive.

### Uninstall

```bash
rm ~/.local/bin/velour ~/.local/share/applications/io.github.mikailustuner.Velour.desktop \
   ~/.local/share/icons/hicolor/scalable/apps/io.github.mikailustuner.Velour.svg
rm -r ~/.local/share/fonts/velour ~/.config/velour ~/.cache/velour ~/.local/state/velour   # settings and caches
# or, for the .deb: sudo apt remove velour
```

## Usage

Pick your browser in **Settings → Account** (Firefox is the default). Double-click a song to play,
right-click (or ⋯) for more.

| Keys | |
|---|---|
| Space | play / pause |
| Ctrl + ← / → | previous / next |
| Shift + ← / → | seek 10 s |
| Ctrl + ↑ / ↓ | volume |
| M | mute |
| P | now playing view |
| Ctrl + L | like |
| Ctrl + F | search |
| Alt + ← | back |
| Ctrl + Q | quit |

On Hyprland, media keys work through MPRIS:
`bind = , XF86AudioPlay, exec, playerctl play-pause`.

## Resource use

Measured on a GTX 1050 Ti, release build:

| | GPU rendering (default) | Low memory mode |
|---|---|---|
| Memory (private) | ~250 MB (mostly the NVIDIA driver) | ~90 MB |
| CPU idle | ~0.4 % of one core | ~0 % |
| CPU while playing | ~2 % | ~3.3 % |
| CPU while scrolling covers | ~20 % | ~175 % |

Low memory mode (Settings, applies after restart) uses software rendering. Each new song briefly
runs yt-dlp at low priority (~4 s); hovering a song or the Play button prepares it in advance.

## Privacy

No accounts, servers, analytics or telemetry. Velour reads your browser's YouTube cookies locally
and sends them only to YouTube. Lyrics lookups send the song title, artist, album and duration to
LRCLIB. Plays are added to your YouTube Music history after 30 seconds (can be turned off).
Details: [SECURITY.md](SECURITY.md).

## Legal

Velour is an independent project and is **not affiliated with, endorsed or sponsored by YouTube,
Google LLC or Alphabet Inc.** YouTube and YouTube Music are trademarks of Google LLC. Velour does
not host or distribute any content and does not bundle yt-dlp. It uses the same internal API as
the YouTube Music website, which YouTube's Terms of Service don't permit for third-party clients:
use it with your own account, at your own risk. Offline downloads are off by default and meant
only for content you have the right to keep. Full text: [DISCLAIMER.md](DISCLAIMER.md).

## Development

```bash
cargo test --workspace                                  # unit + parser tests (public fixtures)
cargo run -p velour-ytm --example dump -- public        # refresh public fixtures (signed out)
cargo run -p velour-ytm --example dump -- private       # your own fixtures (gitignored)
cargo run -p velour-ytm --example e2e_playlist          # playlist edits on a throwaway playlist
VELOUR_AO=null cargo run -p velour                      # run without audio output
```

Crates: `crates/ytm` (YouTube Music client + parsers), `crates/player` (libmpv + yt-dlp),
`crates/app` (GTK UI). Design notes in [docs/design.md](docs/design.md). See
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

[GPL-3.0-or-later](LICENSE). Third-party components: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
