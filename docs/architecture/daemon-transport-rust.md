# Daemon transport boundary (Rust)

> Wire shapes live in `daemon-proto`. Verb-level contracts live in
> `daemon-transport`. Concrete network stacks (iroh, UDS, in-memory,
> websocket — pick one) implement the trait surface and get injected
> at the host. Daemon handlers and client callers never name a
> network stack.

#pattern · #rust · #transport

## What lives where

| crate | content | runtime deps |
|---|---|---|
| `daemon-proto` | wire shapes — `Control`, `WorkerReply`, the envelopes, `ALPN`, `PROTOCOL_VERSION`, `TerminalRestoreStatus`, the `ProjectSummary`/`TaskSummary`/`TabSummary` tree, every wire DTO | serde only |
| `daemon-transport` | abstract `Session` (client side), `ServerSession` (server side), `Transport`, `TransportFactory`, `DialTarget`, `SessionEvent`, `TransportError`, `RequestId`, `SessionFuture` | futures-core, thiserror, tokio (sync only for the in-memory impl) |
| `daemon-transport`'s sub-modules | concrete impls — currently `in_memory` for tests; future `iroh` and `uds` | each adds the runtime its stack needs |
| `daemon` | server-side handlers, registry trait, dispatch loop. Generic over `ServerSession` | tokio + iroh (today) |
| `daemon-client` | client-side typed API, dial helpers. Generic over `TransportFactory` | tokio + iroh (today via `IrohTransportFactory`) |
| `app`, `mobile`, etc. | hosts. Wire in a concrete `TransportFactory` at startup | as they please |

## Trait surface at a glance

```text
                       ┌────────────────────────┐
                       │ daemon-proto (verbs)   │
                       └────────────────────────┘
                                  ▲
                ┌─────────────────┴─────────────────┐
                │                                   │
   ┌────────────────────┐               ┌────────────────────┐
   │ Session (client)   │               │ ServerSession      │
   │  - call(verb)→reply│               │  - next_call()     │
   │  - push_data       │               │  - reply(id, reply)│
   │  - events()→stream │               │  - push_data       │
   │  - close           │               │  - push_reply      │
   └────────────────────┘               │  - close           │
            ▲                           │  - peer_id         │
       dial()                           └────────────────────┘
            │                                    ▲
   ┌────────────────────┐               ┌────────────────────┐
   │ TransportFactory   │               │ Transport          │
   │  - dial(DialTarget)│               │  - accept()        │
   └────────────────────┘               └────────────────────┘
            ▲                                    ▲
            │                                    │
   ┌────────────────────────────────────────────────────────┐
   │  concrete impl (iroh, UDS, in-memory, …)                │
   └────────────────────────────────────────────────────────┘
```

The two `*Session` traits exist because client and server have
opposite directionality:

- **Client `Session::call(verb) → reply`** issues a verb and awaits a
  reply correlated by `RequestId`.
- **Server `ServerSession::next_call() → (RequestId, Control)`**
  receives the verb; the daemon dispatches; **`reply(id, …)`** sends
  the matched reply back.

A shared trait would force every site to ignore half its methods.

## Why the layer exists

Pre-refactor the daemon and its clients reached directly for
`iroh::endpoint::{SendStream, RecvStream}`, length-prefixed frames,
ALPN, and request-id correlation. Every network-stack swap meant
touching both sides; tests had to spin up real iroh just to exercise
the verb layer; type drift between server and client was a
*mirror-by-comment* dance that broke on `gpui-on-mobile` (server on
`/1` ALPN, client on `/0`).

Generator/injection split fixes it:

- Daemon takes a `Box<dyn Transport>` and asks it for `ServerSession`s.
  Doesn't name iroh.
- Clients take an `Arc<dyn TransportFactory>` and ask it to dial.
  Don't name iroh.
- Concrete transports own *all* the iroh/UDS/whatever specifics.

## Adding a new transport — the recipe

1. **New module** under `daemon-transport/src/your_stack.rs` (or a
   sibling crate if it pulls heavy deps you don't want in the trait
   crate).

2. **Implement `Session`** for the client end:
   - `call(verb)` — assign a `RequestId`, send the verb framed however
     your stack frames things, await the reply correlated by id.
     Concurrent calls must work (a `HashMap<RequestId, oneshot::Sender<WorkerReply>>`
     is the standard pattern).
   - `push_data(section, tab, bytes)` — fire-and-forget bytes for an
     attached tab. Backpressure is your problem, not the caller's.
   - `events()` — return a `Stream<SessionEvent>` that yields
     `PtyBytes` / `Push` / `Lagged` / `Closed`. Terminate the stream
     after yielding `Closed`.
   - `close(reason)` — idempotent; dropping the session must also
     close it.

3. **Implement `ServerSession`** for the server end:
   - `peer_id()` — pick a stable identifier (iroh `EndpointId`, UDS
     pid+uid, `inproc:<name>`, etc.) the daemon uses for logging and
     authorisation.
   - `next_call()` — read the next inbound verb. `Ok(None)` for clean
     close; `Err(TransportError)` for transport faults.
   - `reply(id, reply)` — send the typed reply correlated by id.
   - `push_data(section, tab, bytes)` and `push_reply(reply)` for the
     daemon-initiated paths (PTY output, broadcast verbs).
   - `close(reason)` — same contract as the client side.

4. **Implement `Transport`** (server-side accept loop) and
   **`TransportFactory`** (client-side dial). The factory carries
   per-impl config (relay URLs, TLS roots, retry policy) so individual
   call sites don't plumb it through.

5. **Map your error tree onto `TransportError`.** Don't leak
   stack-specific error types through the trait.

6. **Validate concurrent `call` correlation.** Two tasks racing the
   same session must each get their own reply, not whoever's reply
   comes back first. The in-memory impl's
   `pair_concurrent_calls_correlate` test is the canonical shape.

## What NOT to do

- **Don't reach for `iroh::`, `tokio_uring::`, etc. outside your
  transport module.** If you need it elsewhere, the abstraction
  missed something and the right move is to grow the trait surface,
  not to leak the import.
- **Don't read frames or construct envelopes by hand.** That's the
  transport's job. Handlers and client callers see typed `Control` /
  `WorkerReply` values, never bytes.
- **Don't add a per-verb `match` to your `Session::call`.** Every
  variant routes through the same `request_id` correlation path. The
  legacy `send_legacy_control` helper-routing in `daemon-client` got
  deleted for exactly this reason once the router was in place.
- **Don't share request-id state between sessions.** Each session
  owns its own counter starting at 1 (or 2 if a Hello took 1).
  Cross-session ids are a debugging trap with no upside.
- **Don't squeeze `Session` and `ServerSession` into one trait.** The
  directionality is opposite; a shared trait pushes the type system
  to the wrong place.

## Current concrete impls

- `daemon-transport::in_memory` — paired tokio mpsc channels, no
  framing. Tests + the abstraction-validation goal. See
  `pair(peer_id) -> (server, client)` for unit-test convenience and
  `InMemoryTransport` / `InMemoryTransportFactory` for named-pair
  discovery.
- `daemon-client::iroh_transport::IrohTransportFactory` — wraps the
  legacy `daemon_client::Session` and adapts it to the abstract
  trait. Adapts pairing-URL dial flow, request_id correlation,
  per-call reply routing.

## Open work

The remaining sub-issues under [`another-one-iem`] in beads:

- `pqs` server-side — reshape today's `daemon::transport_iroh` so its
  accept loop produces `Box<dyn ServerSession>`s the daemon dispatch
  layer drives.
- `7re` daemon dispatch — verb dispatch moves out of
  `transport_iroh::handle_incoming` into a transport-agnostic
  `serve_session(session, registry)` function.
- `4l7` UDS transport — second real impl (desktop ↔ MCP shim).
- `4y2` mobile smoke — end-to-end verification that mobile keeps
  working over the abstract API.
- `3yy` iroh-import sweep — find every `use iroh` outside the iroh
  transport module after the dust settles. Anything left is a leak.

Each lands as its own PR; this doc gets updated as they do.
