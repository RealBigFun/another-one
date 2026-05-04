//! Iroh client for AnotherOne.
//!
//! Pairs with `daemon-sandbox`'s server-side transport: scans a
//! pairing URL, dials the daemon's iroh endpoint over the
//! `anotherone/pty/0` ALPN, and runs the bidirectional control/data
//! frame protocol from the client side. Lifted from the legacy
//! `mobile-core/src/api/iroh_client.rs` with FRB attributes stripped
//! so it can be linked into the GPUI app (desktop + android cdylib)
//! without dragging Flutter along.
//!
//! Public surface is intentionally narrow — UI code dials by URL and
//! pumps the resulting [`Session`] for status events and replies; all
//! other knobs (DNS resolver, secret-key persistence, retry policy)
//! live behind the API and are tuned in-crate.

pub mod frame;
pub mod iroh_transport;
pub mod pairing_url;
pub mod protocol;
pub mod session;
pub mod status;

pub use iroh_transport::{iroh_factory, pairing_target, socket_target, IrohTransportFactory};
pub use pairing_url::{parse_pairing_url, PairingUrl};
pub use protocol::{
    AgentProvider, Control, ControlEnvelope, ProjectKind, ProjectSummary, TabSummary, TaskSummary,
    WorkerReply, WorkerReplyEnvelope, ALPN, MAX_FRAME_BYTES, PROTOCOL_VERSION, PUSH_REQUEST_ID,
    TY_CONTROL, TY_DATA, TY_WORKER_REPLY,
};
pub use session::{connect, Session, SessionEvent};
pub use status::{drain_status, DialStatus};
