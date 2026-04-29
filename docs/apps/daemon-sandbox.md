# `daemon-sandbox/` — throwaway PTY daemon

Proves the daemon transport shape end-to-end without touching
[[desktop]]. Spawns PTY sessions and bridges framed control/output over
Iroh QUIC; the loopback WebSocket path remains diagnostic only.

## Entry points

- `daemon-sandbox/src/main.rs` — spawns both transports, waits for ctrl_c.
- `daemon-sandbox/src/pty.rs` — shared `PtySession` that both transports
  use; reads `$AGENT_CMD` → `$SHELL` → `bash`.
- `daemon-sandbox/src/transport_ws.rs` — axum WebSocket handler.
  Wire format: binary frames = PTY bytes; text frames = JSON control
  (`{"type":"resize","cols":C,"rows":R}`).
- `daemon-sandbox/src/transport_iroh.rs` — Iroh endpoint, ALPN
  `anotherone/pty/1`, length-prefixed frames, pairing, and terminal
  control messages.
- `daemon-sandbox/src/bin/iroh-client.rs` — CLI smoke test that dials the
  Iroh endpoint and echoes bytes. Handy for isolating Iroh from UI
  clients.

## Running

```sh
# Default (spawns $SHELL)
cargo run -p daemon-sandbox

# With a specific agent
AGENT_CMD=claude cargo run -p daemon-sandbox
```

On startup:
- WebSocket: `ws://127.0.0.1:5617/pty`
- Iroh: pairing ticket written to `/tmp/daemon-sandbox.ticket` for
  `iroh-client` and `slint-poc` to pick up.

For Slint platform build proofs, keep the daemon as a separate dev-time process
and use the target-specific scripts under `scripts/slint/`. Packaging gates must
not embed daemon-sandbox into platform-specific view/layout branches; Slint
platform selection stays in `slint-poc/src/platform.rs`, Cargo targets, and the
script profiles.

## Why it's called sandbox

It exists to prove the architecture, not to ship. Eventually the same
transport logic becomes part of the embedded daemon in [[desktop]] (see
[[../architecture/peer-to-peer-nodes]]) and this crate goes away.

## Known limitations

- The WebSocket path is unauthenticated and disabled by default.
- `/tmp/daemon-sandbox.ticket` is a dev-time shortcut.
- Default relay mesh is Number Zero's "canary" — dev-only per Iroh's
  docs. See [[../postmortems/2026-04-23-iroh-android-hang]] for the
  self-hosted-relay requirement in production.
