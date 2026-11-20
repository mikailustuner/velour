# Remote control socket

Lets other programs on this computer (for example a phone remote such as Tiphys, which
talks to a small agent on the PC) control playback, browse YouTube Music and read lyrics through Velour.

- **Off by default.** Settings → Desktop → Remote control.
- Socket: `$XDG_RUNTIME_DIR/velour/control.sock`. The directory is `0700`, the socket `0600`: only your own user
  can connect. Velour refuses a directory that is a symlink or belongs to another user.
- One request per connection: send one JSON line, read one JSON line back.

```console
$ echo '{"v":1,"op":"state"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/velour/control.sock
{"v":1,"ok":true,"result":{"track":{…},"playing":true,"position":31.2,…}}
```

Errors: `{"v":1,"ok":false,"error":{"code":"…","message":"…"}}` with code `badRequest`, `unsupportedVersion`,
`tooLarge`, `notFound`, `notSignedIn`, `timeout`, `busy` or `failed`.

## Operations (protocol 1)

Field names are camelCase. Tracks use the shape returned by `queue`/`search`
(`videoId`, `title`, `artists[{name, browseId}]`, `album`, `duration`, `thumbnail`, `setVideoId`, `playable`, `like`).

| op | fields | result |
|---|---|---|
| `state` | | track, index, queueLength, playing, loading, position, duration, volume, muted, shuffle, repeat, signedIn, rev, queueRev, sleep, slept, eq |
| `wait` | `since` (a `rev`), `timeout?` (seconds, 1–25, default 20) | state, once `rev` ≠ `since` or the timeout passes |
| `queue` | | `{index, tracks, radio}` |
| `lyrics` | | `{videoId, kind: synced\|plain\|none, lines[{time, text}], text, source}` for the current song |
| `toggle`, `next`, `prev`, `clear` | | new state |
| `seek` | `seconds` | new state |
| `volume` | `percent` (0–100) | new state |
| `mute`, `shuffle` | `on` | new state |
| `repeat` | `mode`: off\|all\|one | new state |
| `eq` | | `{preset, presets}` |
| `set-eq` | `preset` (one of `presets`; else `notFound`) | `{preset, presets}` |
| `sleep` | `minutes?` (1–720) or `endOfTrack?: true`; neither cancels | new state |
| `show` | | `{}` (brings the Velour window to the front, also when it runs in the background) |
| `jump`, `remove` | `index` | new state |
| `move` | `from`, `to` | new state |
| `play-next`, `add-to-queue` | `track` | new state |
| `play` | `tracks`, `start`, `shuffle` | new state |
| `play-target` | `target` (`{kind:"watch", videoId?, playlistId?, params?}`) | new state |
| `radio` | `videoId` | new state |
| `search` | `query` | page |
| `suggest` | `query` | list of strings |
| `page` | `id`, `params?` (album, playlist, artist) | page |
| `more` | `token` (a page's `continuation`) | page |
| `home`, `explore`, `liked`, `history` | | page |
| `library` | `tab`: playlists\|albums\|artists | page |
| `rate` | `videoId`, `like`: like\|dislike\|indifferent | `{}` |
| `playlists` | | your own playlists `[{id, title}]` |
| `add-to-playlist` | `playlistId`, `videoIds` | `{}` |
| `create-playlist` | `title`, `videoIds?` | `{id}` |
| `save-queue` | `title` | `{id}` of a new playlist with the queue's songs (`notFound` if the queue is empty) |
| `save-to-library` | `playlistId`, `save` | `{}` |
| `remove-from-playlist` | `playlistId`, `videoId`, `setVideoId` | `{}` |

A page is `{header?, sections[{title, layout: list|carousel|grid, items, more?}], continuation?}`; an item is a
track (`kind: "track"`) or a card (`kind: "card"`, `title`, `subtitle`, `thumbnail`, `target`, `round`). Targets are
`{kind: "browse", id, params?, pageType}` (open with `page`) or `{kind: "watch", …}` (play with `play-target`).

Control answers are sent right after the command is applied; mpv reports some changes (pause, position) a moment
later, so ask for `state` again shortly after.

Ops that talk to YouTube Music (`search`, `suggest`, `page`, `more`, `home`, `explore`, `library`, `liked`,
`history`, `lyrics` when not cached, `rate`, `add-to-playlist`, `create-playlist`, `save-queue`, `save-to-library`,
`remove-from-playlist`) answer `notSignedIn` before sign-in finished and can take a few seconds.

### State fields

- `rev` — a number that goes up whenever something a remote shows changes: song or queue position, playing/paused,
  loading, volume, mute, shuffle, repeat, the queue, the current song's like, the duration, the sleep timer, the
  equalizer preset, sign-in, or a seek. The position moving on during playback doesn't change it.
- `queueRev` — changes when the queue's songs, order or current index (or the radio flag) change; fetch `queue`
  again when it does.
- `sleep` — `null`, or `{endsAt, endOfTrack}`: `endsAt` is a Unix time in seconds (`null` for end of track).
- `slept` — how often the sleep timer has gone off since Velour started; it going up means "fired", `sleep`
  turning `null` without it means "cancelled".
- `eq` — the current equalizer preset.

### Waiting for changes

Instead of polling `state`, send `wait` with the last `rev` you saw. If it is already different you get the state at
once; otherwise the answer comes as soon as something changes, or after `timeout` seconds with the same `rev`.
Then send the next `wait`. A few waits can be parked at once; beyond that they answer immediately.

### Sleep timer

`sleep` with `minutes` pauses playback after that time, fading the volume out over the last 8 seconds (the volume
setting itself doesn't change). With `endOfTrack: true` playback stops when the current song ends and the next song
is lined up, paused, so Play continues with it; skipping songs meanwhile keeps the timer. A new `sleep` replaces the
old one, `sleep` without fields cancels it. When it goes off, `sleep` becomes `null` and `slept` goes up.
