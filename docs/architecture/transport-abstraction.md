# TerminalTransport abstraction

> Every client talks to the daemon through the same interface: bytes in,
> bytes out, plus a thin control channel. Swapping transport (WebSocket
> → Iroh → in-process → …) is a one-impl change; UI code doesn't move.

#pattern

## The interface

See `mobile/lib/src/transport.dart`:

```dart
abstract class TerminalTransport {
  Stream<Uint8List> get incoming;       // PTY bytes from daemon
  Stream<TransportStatus> get status;
  TransportStatus get currentStatus;

  void connect();
  void sendBytes(List<int> bytes);      // input bytes to daemon's PTY
  void sendResize({required int cols, required int rows});
  Future<void> close();
}
```

Exactly five verbs. Anything you'd conceivably want on a session
(clipboard, paste bracket, focus hints) fits one of them, or becomes a
specific JSON control message under `sendResize`-equivalent encoding.

## Current implementations (Dart)

- **`WebSocketTransport`** (`transport_websocket.dart`) — binary frames
  carry PTY bytes; text frames carry JSON control (`resize`).
- **`IrohTransport`** (`transport_iroh.dart`) — wraps the FRB-generated
  `IrohSession`; QUIC bidi stream. Resize control currently dropped;
  pending framing work on [[../apps/daemon-sandbox]]'s Iroh path.

## How the UI stays clean

`main.dart::_buildTransport(url)` is the *only* place in the widget tree
that cares about URL schemes. The rest of the app holds a
`TerminalTransport?` and talks to it. A future `IrohTransport`
replacing `WebSocketTransport`, or an in-process variant for
[[../architecture/peer-to-peer-nodes|desktop-as-client-of-itself]], is a
local change there.

## Why the interface is this shape

- **Byte streams both directions.** Keeps the
  [[terminal-wrapping-principle]] intact — no structured messages about
  agent output, ever.
- **Control is an out-of-band sub-API** (`sendResize`). Control framing
  differs by transport (WS: text frames; Iroh: type-byte length-prefixed
  frames) but the Dart caller doesn't see that.
- **Status is a first-class stream.** UI binds to it directly;
  `connecting / connected / disconnected / error(detail)` is uniform
  across transports.
- **One-shot lifecycle.** `close()` is terminal; reconnecting is a new
  transport instance. Keeps the state machine small.

## When to extend

Resist adding a sixth verb. Most "new things" are either:
- a new control message → encode it under the existing `sendResize`
  pipeline (which really should be called `sendControl` in a
  refactor);
- a new status variant → extend `TransportStatus`.

## On the Rust side

The `core` extraction in Phase 1 should introduce the same shape on the
Rust side — a `TerminalClient` trait that the GPUI UI, a CLI tool, and
any other in-process consumer implement. Keeps the peer-to-peer model
in [[peer-to-peer-nodes]] internally consistent.
