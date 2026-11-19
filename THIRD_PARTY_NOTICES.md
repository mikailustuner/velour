# Third-party notices

Velour is licensed under the GNU GPL v3.0 or later (see `LICENSE`). It includes or uses:

| Component | License | Where |
|---|---|---|
| Plus Jakarta Sans (Tokotype) | SIL Open Font License 1.1 | `assets/fonts/`, license in `assets/fonts/OFL-PlusJakartaSans.txt` |
| Material Symbols (Google) | Apache License 2.0 | icons in `crates/app/icons/` (unmodified SVGs) — https://github.com/google/material-design-icons |
| Rust crates (gtk-rs, relm4, reqwest, serde, tokio, libmpv2, mpris-server, ksni, image, …) | MIT / Apache-2.0 / similar | resolved by Cargo; see `Cargo.lock` |

Used at runtime but **not** included or redistributed:

| Component | License | Notes |
|---|---|---|
| libmpv / mpv | GPL-2.0-or-later / LGPL | system library |
| GTK 4 | LGPL-2.1-or-later | system library |
| yt-dlp | Unlicense | installed by the user |
| deno | MIT | installed by the user |
| LRCLIB lyrics API | lrclib.net terms | synced lyrics; Velour sends song title/artist/album/duration only |

YouTube Music data shown in the app (titles, artwork, lyrics) belongs to its respective owners.
