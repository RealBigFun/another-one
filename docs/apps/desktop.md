# `desktop/` — AnotherOne GPUI app

The canonical desktop client. Rust + GPUI; currently the whole app's logic
lives here and will be split in [[../architecture/peer-to-peer-nodes|Phase 1]]
into a headless `core` crate and this thin GPUI shell.

## Entry points

- `desktop/src/main.rs` — GPUI bootstrap, font setup, window creation.
- `desktop/src/app.rs` — `AnotherOneApp` entity holds all state (`ProjectStore`,
  terminal sessions, git state, UI state). ~8.5k LOC today, scheduled for the
  core-extraction split.
- `desktop/src/project_store.rs` — persistence (single JSON at
  `~/.config/another-one/projects.json`).
- `desktop/src/terminal_runtime.rs` / `terminal_launch.rs` — PTY lifecycle
  and alacritty-backed rendering.

## Running

```sh
cargo run -p desktop
```

or the helper script `scripts/dev-watch.sh` for hot-rebuild on source
changes.

Build-time config worth knowing:
- macOS and Linux only; no Windows target.
- `hotpath` feature flag enables `hotpath` performance probes.

## Key dependencies

- `gpui` 0.2 — Zed's UI framework.
- `alacritty_terminal` 0.26 — VT emulator; same crate used by
  [[mobile-core]] (so desktop and mobile agree on parsing).
- `portable-pty` 0.9 — cross-platform PTY spawning. Also used by
  [[daemon-sandbox]].

## Direction

Per [[../architecture/peer-to-peer-nodes]], this app becomes a *client* of
its own embedded daemon once `core` is extracted. Its terminal UI will
consume sessions through the same abstraction any other client (mobile,
CLI) does — just via in-process calls instead of Iroh-over-LAN.

## Known gaps

- All state lives in one entity; [[../postmortems]] may accumulate notes
  on specific bugs.
- No headless mode yet; cannot run without a display.
