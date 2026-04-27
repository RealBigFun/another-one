# `daemon-sandbox/` — throwaway PTY daemon

Proves the mobile-companion transport shape end-to-end without touching
[[desktop]]. Spawns one PTY per connection and bridges bytes over Iroh
QUIC; the legacy WebSocket path still exists for explicit smoke tests,
but it is insecure and disabled by default.

## Entry points

- `daemon-sandbox/src/main.rs` — spawns Iroh by default, optionally
  starts the legacy WebSocket transport when explicitly enabled, then
  waits for ctrl_c.
- `daemon-sandbox/src/pty.rs` — shared `PtySession` that both transports
  use; reads `$AGENT_CMD` → `$SHELL` → `bash`.
- `daemon-sandbox/src/transport_ws.rs` — axum WebSocket handler.
  Wire format: binary frames = PTY bytes; text frames = JSON control
  (`{"type":"resize","cols":C,"rows":R}`).
- `daemon-sandbox/src/transport_iroh.rs` — Iroh endpoint, ALPN
  `anotherone/pty/1`. Currently raw bytes both directions (no framing —
  resize control is not yet implemented on this path).
- `daemon-sandbox/src/bin/iroh-client.rs` — CLI smoke test that dials the
  Iroh endpoint and echoes bytes. Handy for isolating Iroh from mobile.

## Running

```sh
# Default (spawns $SHELL, Iroh only)
cargo run -p daemon-sandbox

# With a specific agent
AGENT_CMD=claude cargo run -p daemon-sandbox

# Opt in to the insecure legacy WebSocket transport
ANOTHER_ONE_ENABLE_INSECURE_WS=1 cargo run -p daemon-sandbox
```

On startup:
- Iroh: EndpointId printed to logs + written to
  `/tmp/daemon-sandbox.nodeid` for `iroh-client` to pick up.
- Pairing ticket: `/tmp/daemon-sandbox.ticket` (non-sensitive smoke-test
  hint format; omits the one-shot pair token).
- Pairing QR PNG: written to a private per-user
  `another-one-sandbox/pairing/` directory and cleaned up on exit,
  rather than a shared `/tmp` path.
- Full EndpointAddr (including direct socket addrs) logged — use the
  `Ip(192.168.x.y:PORT)` address to construct
  `iroh://<EndpointId>?direct=10.0.2.2:<PORT>` URLs for the Android
  emulator. See [[mobile]].
- WebSocket, when `ANOTHER_ONE_ENABLE_INSECURE_WS=1`: `ws://127.0.0.1:5617/pty`

## Why it's called sandbox

It exists to prove the architecture, not to ship. Eventually the same
transport logic becomes part of the embedded daemon in [[desktop]] (see
[[../architecture/peer-to-peer-nodes]]) and this crate goes away.

## Known limitations

- The legacy WebSocket transport is unauthenticated; it must stay
  disabled outside explicit local smoke tests.
- Iroh path has no control framing yet; resize messages from mobile are
  silently dropped. Fix planned: length-prefixed frames with a type byte.
- `/tmp/daemon-sandbox.nodeid` hint file is a dev-time shortcut.
- Default relay mesh is Number Zero's "canary" — dev-only per Iroh's
  docs. See [[../postmortems/2026-04-23-iroh-android-hang]] for the
  self-hosted-relay requirement in production.
