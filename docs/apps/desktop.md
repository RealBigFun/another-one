# `app/` — AnotherOne GPUI app

The canonical desktop client. Rust + GPUI, backed by shared `core`,
`daemon`, `daemon-client`, and `daemon-transport` crates.

## Entry points

- `app/src/main.rs` — GPUI bootstrap, font setup, window creation.
- `app/src/app.rs` — `AnotherOneApp` entity owns the GPUI state and
  applies daemon projections.
- `core/src/project_store.rs` — app-state persistence and projection helpers.
- `app/src/terminal_runtime.rs` / `core/src/terminal_launch.rs` — PTY lifecycle
  on the desktop side. The renderer-side VT parser retired in design 01
  Phase 5b ([[../designs/01-daemon-canonical-terminal]]); `terminal_runtime.rs`
  is now a snapshot consumer + scrollback cache + viewer-input queue.
  The canonical `alacritty_terminal::Term` lives in the daemon
  ([[../designs/01-daemon-canonical-terminal]] / `daemon/src/terminal/`).
- `app/src/daemon_host.rs` — embedded daemon/session bridge for desktop.

## Running

```sh
cargo run -p another-one
```

or the helper script `scripts/dev-watch.sh` for hot-rebuild on source
changes.

Build-time config worth knowing:
- macOS and Linux only; no Windows target.
- `hotpath` feature flag enables `hotpath` performance probes.

## Key dependencies

- `gpui` 0.2 — Zed's UI framework.
- `alacritty_terminal` 0.26 — VT emulator. Owned daemon-side
  ([[../designs/01-daemon-canonical-terminal]]); the desktop renderer
  pulls in `alacritty_terminal::vte::ansi::Rgb` only at the colour
  boundary. PTY spawn moves to the daemon in Phase 4 (open).
- `portable-pty` 0.9 — cross-platform PTY spawning. Also used by
  [[daemon-sandbox]].

## Direction

Per [[../architecture/peer-to-peer-nodes]], this app is a *client* of
its own embedded daemon. Its terminal UI consumes sessions through the
same abstraction any other client does — via in-process calls locally or
Iroh for paired clients.

## Known gaps

- All state lives in one entity; [[../postmortems]] may accumulate notes
  on specific bugs.
- No headless mode yet; cannot run without a display.
