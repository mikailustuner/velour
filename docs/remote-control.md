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
| `state` | | track, index, queueLength, playing, loading, position, duration, volume, muted, shuffle, repeat, signedIn |
| `queue` | | `{index, tracks, radio}` |
| `lyrics` | | `{videoId, kind: synced\|plain\|none, lines[{time, text}], text, source}` for the current song |
| `toggle`, `next`, `prev`, `clear` | | new state |
| `seek` | `seconds` | new state |
| `volume` | `percent` (0–100) | new state |
| `mute`, `shuffle` | `on` | new state |
| `repeat` | `mode`: off\|all\|one | new state |
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
| `home`, `liked`, `history` | | page |
| `library` | `tab`: playlists\|albums\|artists | page |
| `rate` | `videoId`, `like`: like\|dislike\|indifferent | `{}` |
| `playlists` | | your own playlists `[{id, title}]` |
| `add-to-playlist` | `playlistId`, `videoIds` | `{}` |
| `create-playlist` | `title`, `videoIds?` | `{id}` |

A page is `{header?, sections[{title, layout: list|carousel|grid, items, more?}], continuation?}`; an item is a
track (`kind: "track"`) or a card (`kind: "card"`, `title`, `subtitle`, `thumbnail`, `target`, `round`). Targets are
`{kind: "browse", id, params?, pageType}` (open with `page`) or `{kind: "watch", …}` (play with `play-target`).

Control answers are sent right after the command is applied; mpv reports some changes (pause, position) a moment
later, so ask for `state` again shortly after.
