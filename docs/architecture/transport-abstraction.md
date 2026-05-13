# Transport abstraction

> Every client talks to the daemon through one session shape: typed control calls, pushed worker replies, and tagged PTY byte streams. Swapping transport (in-process, Iroh, future local sockets) should not change UI state code.

#pattern

## Current interface

The source of truth is the Rust `daemon_transport::Session` trait and the shared wire types in `daemon-proto`:

```rust
pub trait Session: Send + Sync {
    fn call(&self, control: daemon_proto::Control) -> SessionFuture<daemon_proto::WorkerReply>;
    fn events(&self) -> Pin<Box<dyn Stream<Item = SessionEvent> + Send + '_>>;
}

pub enum SessionEvent {
    PtyBytes { section_id: String, tab_id: String, bytes: Vec<u8> },
    Push(daemon_proto::WorkerReply),
    Lagged { skipped: u64 },
    Closed { reason: Option<String> },
}
```

UI code submits durable actions as `Control` calls and consumes projections through `WorkerReply::ProjectList` pushes. PTY bytes stay byte streams keyed by `(section_id, tab_id)` so the terminal-wrapping principle remains intact.

## Current implementations

- **In-memory session** (`daemon-transport/src/in_memory.rs`) — used by the embedded desktop daemon and integration tests. Same request/reply/event semantics as remote transports, no network.
- **Iroh session** (`daemon-client/src/session.rs`, `daemon/src/transport_iroh.rs`) — QUIC transport for paired clients. Carries the same `ControlEnvelope`, `WorkerReplyEnvelope`, and tagged PTY data frames defined in `daemon-proto`.
- **UDS / MCP bridge** (`daemon-transport/src/uds.rs`, `daemon/src/transport_mcp.rs`) — local machine automation path that routes into the same daemon registry and dispatch layer.

## How UI stays clean

The app owns a single session handle. Desktop receives an in-process session from `app/src/daemon_host.rs`; remote pairing replaces that handle with an Iroh-backed session. Call sites still use:

- `session.call(Control::...)` for request/reply work;
- `SessionEvent::Push(WorkerReply::ProjectList { .. })` for daemon projections;
- `SessionEvent::PtyBytes { .. }` for terminal output.

Client-side projection ingestion is shared in `AnotherOneApp::apply_latest_project_list_projection`: newest `ProjectList` wins, `ProjectStore::absorb_projection` applies the read model, expanded repo state is mirrored locally, and open section tabs reconcile through the section-state seam.

## Why the interface is this shape

- **Typed control calls.** New app behavior is a `daemon_proto::Control` variant plus a daemon dispatch arm, not transport-specific UI code.
- **Pushed projections.** State changes broadcast `WorkerReply::ProjectList` so clients converge without polling.
- **Tagged PTY streams.** Every PTY chunk carries `(section_id, tab_id)`, preventing attach/focus races from routing output into the wrong tab.
- **Transport status stays out-of-band.** Dial/pairing status belongs to the concrete client (`daemon-client::status`) while app state remains transport-independent.

## When to extend

Prefer adding a `Control` variant or `WorkerReply` variant over adding transport-specific methods. Add a `SessionEvent` variant only when the event is transport-independent and every session implementation can define equivalent semantics.

## See also

- [[terminal-wrapping-principle]]
- [[peer-to-peer-nodes]]
- [[daemon-transport-rust]]
- [[gui-mcp-unification]]
