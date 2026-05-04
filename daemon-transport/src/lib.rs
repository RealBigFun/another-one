//! Abstract transport surface — verb in, reply out, no wire format.
//!
//! This crate names *what* the daemon and its clients exchange
//! ([`daemon_proto::Control`] verbs, [`daemon_proto::WorkerReply`]
//! replies, raw payload pushes for attached PTYs) and *how* they
//! exchange it at the highest possible level ([`Session`] /
//! [`Transport`] / [`TransportFactory`]). It does not name *over what
//! pipe* — that is the job of a concrete impl (iroh, UDS, in-memory,
//! websocket — pick one).
//!
//! ## Why this layer exists
//!
//! Pre-extraction the daemon and `daemon-client` reached directly for
//! `iroh::endpoint::{SendStream, RecvStream}`, framed bytes, ALPN, and
//! request-id correlation. Network-stack swaps required touching both
//! sides; tests had to spin up real iroh just to exercise the verb
//! layer. The fix is the standard generator/injection split:
//!
//!   - **Daemon** accepts an [`Transport`] from its host and asks it
//!     for [`Session`]s. It never names the underlying pipe.
//!   - **Clients** take a [`TransportFactory`] from their host and ask
//!     it to [`dial`](TransportFactory::dial) a target. They never
//!     name the underlying pipe.
//!   - **Concrete transports** live in sibling crates
//!     (`daemon-transport-iroh`, `daemon-transport-uds`,
//!     `daemon-transport-mem`, …) and implement the traits here.
//!
//! ## Object safety
//!
//! Both [`Session`] and [`Transport`] need to be usable as `dyn` so
//! a daemon held behind `Arc<dyn Transport>` can accept connections
//! produced by a runtime-selected impl. Methods can't be `async fn`
//! directly on a dyn-trait — that desugars to a per-impl `impl Future`
//! which isn't object-safe ahead of dyn-async-fn-in-trait
//! stabilising. We use the project's existing pattern of returning
//! [`SessionFuture`] (a pinned, boxed `dyn Future`) from each method
//! so the trait stays dyn-compatible without `async-trait`'s hidden
//! allocations being implicit.
//!
//! ## What lives in `daemon-proto` vs here
//!
//! | crate                  | content                                                 |
//! | ---------------------- | ------------------------------------------------------- |
//! | `daemon-proto`         | wire shapes — `Control`, `WorkerReply`, envelopes, ALPN |
//! | `daemon-transport`     | abstract `Session` / `Transport` traits (this crate)    |
//! | `daemon-transport-*`   | concrete impls (one per network stack)                  |
//! | `daemon`               | handlers + verb dispatch, generic over `Session`        |
//! | `daemon-client`        | typed client API, generic over `TransportFactory`       |
//!
//! ## Pushed frames vs replies
//!
//! Today's wire layer carries two kinds of daemon → client traffic:
//! **replies** to a specific `Control` (correlated by `request_id`)
//! and **push frames** the daemon emits unsolicited (PTY bytes for an
//! attached tab, future broadcasts). The abstract surface keeps that
//! split:
//!
//!   - [`Session::call`] handles the request/reply path. The transport
//!     is responsible for matching reply to call — callers never see
//!     `request_id`.
//!   - [`Session::events`] yields a stream of [`SessionEvent`]s for
//!     everything else: PTY bytes, broadcasts, transport-level lag /
//!     close notifications. Subscribing is implicit in opening the
//!     session.
//!
//! ## Roadmap
//!
//! Sub-issues building on this surface are tracked in beads under
//! [`another-one-iem`]: factory injection wiring (`9zi`), iroh impl
//! reshape (`pqs`), daemon dispatch refactor (`7re`), typed client API
//! (`f4r`), in-memory test transport (`44q`), UDS transport (`4l7`),
//! mobile end-to-end smoke (`4y2`), iroh-import sweep (`3yy`),
//! architecture doc (`l9v`).

use std::pin::Pin;

use daemon_proto::{Control, WorkerReply};
use futures_core::Stream;

pub mod in_memory;
#[cfg(unix)]
pub mod uds;

// ──────────────────────────────────────────────────────────────────
// Shared types
// ──────────────────────────────────────────────────────────────────

/// Pinned, boxed `dyn Future` returned from trait methods. Mirrors
/// `another_one_core::registry::RegistryFuture` so this crate matches
/// the project's existing dyn-async pattern.
pub type SessionFuture<'a, T> = Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Stream of unsolicited frames the daemon pushes to a session. Pinned
/// so consumers don't have to bound the impl's stream type.
pub type EventStream = Pin<Box<dyn Stream<Item = SessionEvent> + Send>>;

/// What [`TransportFactory::dial`] needs to identify the daemon to
/// connect to. Today's iroh-pairing flow encodes a relay URL + node
/// id + pair-token in a single string; UDS just needs a path; in-
/// memory matches a name. The enum stays open so adding a new
/// transport doesn't require changing every dial site.
#[derive(Clone, Debug)]
pub enum DialTarget {
    /// A pairing URL emitted by the daemon's QR / pairing flow.
    /// Today's iroh impl parses the relay URL + node id + pair token
    /// out of this; alternative iroh-style transports could reuse it.
    PairingUrl(String),
    /// A filesystem path for stream-socket transports (UDS).
    SocketPath(std::path::PathBuf),
    /// An in-process channel name for the test transport. The host
    /// keeps the matching server registry; both sides agree on the
    /// name out of band.
    InProcess(String),
}

/// Anything the daemon may push to a session that isn't a reply to a
/// specific `Session::call`. The transport translates its concrete
/// frames into these so handlers and clients don't see iroh-shaped
/// data.
#[derive(Clone, Debug)]
pub enum SessionEvent {
    /// Raw PTY bytes for an attached tab. The transport tags the
    /// stream with `(section_id, tab_id)` so a client attached to
    /// multiple tabs over one session can demultiplex without a
    /// separate channel per tab.
    PtyBytes {
        section_id: String,
        tab_id: String,
        bytes: Vec<u8>,
    },
    /// A daemon-pushed worker reply that wasn't requested (today
    /// reserved for future broadcast verbs — `ProjectListChanged`,
    /// etc.). Mirrors what `request_id == 0` carries today.
    Push(WorkerReply),
    /// The transport observed a backlog and dropped some events.
    /// Skipping is the transport's choice; surfacing it to the
    /// caller is the trait's contract so consumers can reset state
    /// when needed.
    Lagged { skipped: u64 },
    /// The session has closed. No further events follow this. The
    /// stream itself terminates after yielding this variant.
    Closed { reason: Option<String> },
}

/// Every error the abstract surface produces. Concrete transports
/// flatten their internal error trees into one of these — callers
/// shouldn't need to know whether a failure came from quinn, libc,
/// or a serde decode.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Peer cleanly closed the session. Includes the close reason
    /// (e.g. `"anotherone/incompatible-version"`) when the transport
    /// can extract one.
    #[error("session closed{}", .0.as_deref().map(|r| format!(": {r}")).unwrap_or_default())]
    Closed(Option<String>),
    /// Couldn't reach the peer (dial failed, connection refused,
    /// pair token rejected). String detail is opaque — humans read
    /// it, callers branch on the variant.
    #[error("connect failed: {0}")]
    Connect(String),
    /// Wire-format error (couldn't decode a reply into the typed
    /// shape, header bytes malformed, etc.). A transport's bug or
    /// version mismatch — never the caller's fault.
    #[error("encoding error: {0}")]
    Encoding(String),
    /// Authentication / pairing failure (TOFU nonce mismatch, peer
    /// not paired, etc.). Distinct from `Connect` because UI usually
    /// wants to re-prompt for pairing here, not retry the dial.
    #[error("auth: {0}")]
    Auth(String),
    /// The request hit the per-call timeout. Whether to retry is the
    /// caller's choice — transports surface, they don't decide.
    #[error("timed out")]
    Timeout,
    /// Anything else the transport couldn't classify. New cases
    /// graduate to typed variants over time.
    #[error("transport: {0}")]
    Other(String),
}

// ──────────────────────────────────────────────────────────────────
// Session — one open connection
// ──────────────────────────────────────────────────────────────────

/// One end of a duplex session with a daemon. Verb in, reply out;
/// raw payload pushes for attached channels; an event stream for
/// daemon-initiated traffic. The transport handles correlation
/// (matching replies to calls), pacing, and reconnection — callers
/// see typed verbs.
///
/// `Session` is `Send + Sync` so handlers can spawn it across tasks
/// behind an `Arc`. Implementations that require interior mutability
/// (most do) wrap their state in their own locks; the trait surface
/// asks only for shared references.
pub trait Session: Send + Sync {
    /// Issue a verb and await the matching reply. Caller never sees
    /// a `request_id` — the transport assigns one per call and
    /// resolves the reply.
    ///
    /// Returns `WorkerReply::Err` for daemon-side typed errors;
    /// returns [`TransportError`] for pipe-level failures (closed,
    /// timeout, encoding). Most callers want to flatten both into
    /// one error type; the daemon-client typed API does that
    /// per-verb.
    fn call<'a>(
        &'a self,
        verb: Control,
    ) -> SessionFuture<'a, Result<WorkerReply, TransportError>>;

    /// Push raw payload bytes for an already-attached channel — used
    /// for PTY input on tabs the session has called
    /// `Control::AttachTab` for. No reply; backpressure is the
    /// transport's problem.
    ///
    /// Calling this for a channel the session isn't attached to is
    /// a transport-level error; well-behaved clients only push for
    /// channels they hold an attachment for.
    fn push_data<'a>(
        &'a self,
        section_id: &'a str,
        tab_id: &'a str,
        bytes: &'a [u8],
    ) -> SessionFuture<'a, Result<(), TransportError>>;

    /// Stream of unsolicited events the daemon pushes for this
    /// session — PTY bytes from attached tabs, broadcasts, lag /
    /// close notifications. The stream terminates after yielding
    /// [`SessionEvent::Closed`].
    ///
    /// Implementations may produce a fresh stream each call (e.g.
    /// over a broadcast channel) so multiple consumers can observe
    /// the session, or a single-consumer stream that returns the
    /// same handle. The trait doesn't require either; consumers that
    /// need fan-out should fan out themselves.
    fn events(&self) -> EventStream;

    /// Close the session, optionally surfacing a reason to the peer
    /// (e.g. `"anotherone/incompatible-version"`). Idempotent —
    /// dropping the session also closes it; this method exists so
    /// callers can supply a meaningful reason.
    fn close<'a>(
        &'a self,
        reason: Option<&'a str>,
    ) -> SessionFuture<'a, Result<(), TransportError>>;
}

// ──────────────────────────────────────────────────────────────────
// Transport — server side
// ──────────────────────────────────────────────────────────────────

/// Stable identifier the server side uses to correlate a reply back
/// to the call that produced it. Mirrors the `request_id` field on
/// `daemon_proto::ControlEnvelope` but is opaque to handlers — the
/// transport assigns and tracks ids; handlers route by this newtype.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RequestId(pub u64);

/// One end of a duplex session from the **server** side. Mirror image
/// of [`Session`] — the server receives verbs and sends replies, while
/// also pushing raw bytes to the peer for PTY-attached channels.
///
/// Why a separate trait rather than reusing [`Session`]: the call /
/// reply directionality is reversed. A [`Session::call`] *issues* a
/// verb and awaits a reply; a [`ServerSession::next_call`] *receives*
/// a verb and the handler later issues [`ServerSession::reply`] with
/// the matching `RequestId`. Squeezing both into one trait would
/// force callers to ignore half the methods on every site, which is
/// the exact "generally usable interface" anti-pattern this layer
/// exists to avoid.
pub trait ServerSession: Send + Sync {
    /// Identifier this transport assigned to the connecting peer.
    /// Stable for the life of the session. Concrete transports pick
    /// the natural shape (iroh: hex EndpointId; UDS: pid+uid; in-
    /// memory: caller-supplied label). Handlers use it for logging,
    /// per-peer state, and authorisation lookups.
    fn peer_id(&self) -> &str;

    /// Block until the next inbound call arrives. Returns the
    /// `RequestId` (so the handler can [`reply`](Self::reply) later)
    /// alongside the verb. `Ok(None)` signals the peer cleanly closed
    /// the call channel — no further calls will arrive but
    /// [`push_data`](Self::push_data) may still be valid until the
    /// session itself closes.
    fn next_call<'a>(
        &'a self,
    ) -> SessionFuture<'a, Result<Option<(RequestId, Control)>, TransportError>>;

    /// Send a reply to a call previously yielded by
    /// [`next_call`](Self::next_call). The transport correlates the
    /// reply to the originating call by `RequestId`. Sending a reply
    /// for an unknown id is a transport-level error — handlers
    /// should only reply to ids they received.
    fn reply<'a>(
        &'a self,
        request_id: RequestId,
        reply: WorkerReply,
    ) -> SessionFuture<'a, Result<(), TransportError>>;

    /// Push raw payload bytes to the peer for an attached channel
    /// (PTY output for an attached tab). Mirror of
    /// [`Session::push_data`] from the server's perspective. No
    /// reply; backpressure is the transport's problem.
    fn push_data<'a>(
        &'a self,
        section_id: &'a str,
        tab_id: &'a str,
        bytes: &'a [u8],
    ) -> SessionFuture<'a, Result<(), TransportError>>;

    /// Push an unsolicited [`WorkerReply`] (the daemon-initiated
    /// broadcast variant — `request_id == PUSH_REQUEST_ID` on the
    /// wire). Used for future verbs like project-list-changed.
    fn push_reply<'a>(
        &'a self,
        reply: WorkerReply,
    ) -> SessionFuture<'a, Result<(), TransportError>>;

    /// Close the session with an optional reason byte string the
    /// transport surfaces to the peer (concrete transports translate
    /// it into the natural primitive — QUIC close reason, UDS
    /// shutdown, etc.). Idempotent.
    fn close<'a>(
        &'a self,
        reason: Option<&'a [u8]>,
    ) -> SessionFuture<'a, Result<(), TransportError>>;
}

/// The server-side surface. Daemon embedders construct a `Transport`
/// (today: `IrohTransport::bind(...)`) and yield it to
/// `daemon::run_endpoint`, which pulls [`ServerSession`]s off it and
/// dispatches verbs via the registry.
///
/// `accept` returns `None` when the transport has been shut down
/// gracefully. Per-session errors are non-fatal — log and call again.
/// Anything fatal to the transport itself surfaces as the next
/// `accept` returning `Err`.
pub trait Transport: Send {
    /// Block until the next incoming session arrives.
    ///
    /// `&mut self` because most transports are stateful (a single
    /// accept loop owns the listening socket / endpoint). Concurrent
    /// accept makes no sense at this layer — the transport hands off
    /// each session for the daemon to drive on its own task.
    fn accept(
        &mut self,
    ) -> SessionFuture<'_, Result<Option<Box<dyn ServerSession>>, TransportError>>;
}

// ──────────────────────────────────────────────────────────────────
// TransportFactory — client side / generator pattern
// ──────────────────────────────────────────────────────────────────

/// Generator for client-side [`Session`]s. The host wires in a
/// concrete factory at startup; everything below this layer takes
/// `Arc<dyn TransportFactory>` (or generic `T: TransportFactory`) and
/// no longer names the network stack.
///
/// Why a factory and not a free function: dial parameters that don't
/// fit in [`DialTarget`] (per-impl config — keep-alive intervals, TLS
/// roots, retry policy) live on the factory. Two callers in the same
/// process can share one factory's config without each plumbing it
/// through every dial.
pub trait TransportFactory: Send + Sync {
    /// Open a fresh session against `target`. The factory is free to
    /// reuse pooled connections internally, but the returned
    /// [`Session`] presents as if it were freshly opened — calls and
    /// events for one session never bleed into another.
    fn dial<'a>(
        &'a self,
        target: DialTarget,
    ) -> SessionFuture<'a, Result<Box<dyn Session>, TransportError>>;
}
