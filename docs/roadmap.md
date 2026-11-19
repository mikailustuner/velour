# Velour roadmap

Each phase ends with a check that proves it works on the real account.

## Phase 0 — risk spikes ✅ (2026-11-18)
- Login with Firefox cookies, list library playlists; Premium playback (itag 774) via libmpv + yt-dlp.
- Findings: skip `ST-*` and partitioned cookies; InnerTube needs HTTP/1.1 with reqwest;
  `libmpv2` crate (mpv API 2.x); yt-dlp needs deno (EJS); deleted tracks must show as unavailable.

## Phase 1 — skeleton ✅ (2026-11-18)
GTK4 + relm4 window, sidebar, page stack, now-playing bar, theme CSS, dark/light.

## Phase 2 — browse ✅ (2026-11-19)
Generic `Page` model + tolerant parsers (home, explore, library, liked, history, playlist,
album, artist, search, queue, lyrics), continuations, cover cache (disk + 200-entry LRU,
off-thread decode/downscale). 12 parser tests on real responses.
Verified by screenshots: Home, Explore, Library tabs, playlist, artist, search.

## Phase 3 — playback ✅ (2026-11-19)
Player service (libmpv, own event thread), resolver (yt-dlp `web_music` fast path + fallback,
URL cache by expiry), queue logic (7 tests), gapless next via mpv playlist + prefetch,
endless radio, unavailable-track skip (max 3 in a row), session restore.
Verified: Premium 774 stream, gapless switch, play/seek from UI.

## Phase 4 — system integration ✅ (2026-11-19)
MPRIS (verified with playerctl: play/pause/next/position/volume/metadata), close-to-background
+ single instance (verified), shortcuts, notifications via org.freedesktop.Notifications (opt-in).

## Phase 5 — YT Music features ✅ (2026-11-19)
Like/unlike, add/remove/create/rename/delete playlists, save albums, radio, history page,
plays added to YTM history after 30 s of real playback (opt-out), synced lyrics (LRCLIB) with
YTM fallback. Verified: lyrics (synced), radio queue, like/unlike API (one accidental like was
reverted). Playlist edits were not run against the account on purpose.

## Phase 6 — extras ✅ (2026-11-19)
Offline downloads (Opus with tags + cover; command verified), equalizer presets (mpv lavfi),
Discord Rich Presence (needs an application ID), tray (StatusNotifierItem). Tray/Discord are
off by default and untested here (no tray host, Discord not running).

## Phase 7 — polish & ship ✅ (2026-11-19)
Tiphys-style redesign (glass cards, cover-colored ambient glow), settings page, toasts,
loading/empty/error states, low-memory rendering option, icon, desktop entry,
`scripts/install.sh`, `scripts/build-deb.sh`, README with measured resource use.

## Open items
- Sidebar collapse under 900 px is implemented but not visually verified (tiled window).
- Search-as-you-type suggestions (API exists: `search_suggestions`) not wired into the UI yet.
- Playlist edit flows and UI-triggered downloads not exercised end-to-end.
