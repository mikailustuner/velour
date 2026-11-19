# Security

Velour reads your browser's YouTube session cookies locally to sign in. If you find a way that
cookies, tokens or other personal data could leak (to logs, files readable by other users, or any
host other than YouTube), please report it privately via
**GitHub → Security → Report a vulnerability** on this repository instead of opening a public issue.

What Velour stores, all under your home directory:

- `~/.config/velour/settings.json` — preferences
- `~/.local/state/velour/` — queue/session, stream URL cache, downloads index
- `~/.cache/velour/` — cover art and page cache (safe to delete)

Cookies are never written to disk by Velour. For Chromium-based browsers, yt-dlp exports them to
a temporary file inside a private (0700) directory that is deleted immediately after reading.
