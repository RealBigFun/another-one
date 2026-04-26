# `daemon-sandbox/` — throwaway PTY daemon

Proves the mobile-companion transport shape end-to-end without touching
[[desktop]]. Spawns one PTY per connection, bridges bytes over either
WebSocket or Iroh QUIC.

## Entry points

- `daemon-sandbox/src/main.rs` — spawns both transports, waits for ctrl_c.
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
# Default (spawns $SHELL)
cargo run -p daemon-sandbox

# With a specific agent
AGENT_CMD=claude cargo run -p daemon-sandbox
```

On startup:
- WebSocket: `ws://127.0.0.1:5617/pty`
- Iroh: EndpointId printed to logs + written to
  `/tmp/daemon-sandbox.nodeid` for `iroh-client` to pick up.
- Full EndpointAddr (including direct socket addrs) logged — use the
  `Ip(192.168.x.y:PORT)` address to construct
  `iroh://<EndpointId>?direct=10.0.2.2:<PORT>` URLs for the Android
  emulator. See [[mobile]].

## Why it's called sandbox

It exists to prove the architecture, not to ship. Eventually the same
transport logic becomes part of the embedded daemon in [[desktop]] (see
[[../architecture/peer-to-peer-nodes]]) and this crate goes away.

## Known limitations

- No auth / no pairing — anyone who reaches the port can connect.
- Iroh path has no control framing yet; resize messages from mobile are
  silently dropped. Fix planned: length-prefixed frames with a type byte.
- `/tmp/daemon-sandbox.nodeid` hint file is a dev-time shortcut.
- Default relay mesh is Number Zero's "canary" — dev-only per Iroh's
  docs. See [[../postmortems/2026-04-23-iroh-android-hang]] for the
  self-hosted-relay requirement in production.
