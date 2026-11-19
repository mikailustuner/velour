# Contributing

Issues and pull requests are welcome.

- Build: `cargo build -p velour` (needs GTK ≥ 4.20 and libmpv development files).
- Before sending a PR: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.
- Parser changes: refresh the public fixtures with
  `cargo run -p velour-ytm --example dump -- public` (signed out; tracking keys are stripped) and keep
  tests structural rather than tied to specific songs.
- Never commit cookies, account names, private playlists or `tests/fixtures/private/`.
- Keep the app light: no new background work that runs while idle, and measure CPU/memory for
  anything that renders continuously.
