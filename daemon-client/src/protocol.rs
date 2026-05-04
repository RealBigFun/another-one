//! Wire-protocol types — re-exported from the shared `daemon-proto`
//! crate so the daemon and every client deserialize the same code path
//! instead of two hand-mirrored copies. Drift used to bite us
//! (`gpui-on-mobile` shipped on `/0` raw `Control` while the live
//! daemon had moved to `/1` with `ControlEnvelope` + `Hello`); making
//! the daemon and client share the same types removes the class of
//! bug.
//!
//! Existing callers reach for `daemon_client::protocol::*`, so this
//! module stays — but its only job is to re-export. The constants
//! `ALPN`, `PROTOCOL_VERSION`, `TY_DATA`, `TY_CONTROL`,
//! `TY_WORKER_REPLY`, `MAX_FRAME_BYTES`, `PUSH_REQUEST_ID`, the
//! `Control` / `WorkerReply` enums, the envelopes, and every wire
//! struct below the envelopes all live in `daemon-proto` now.

// TODO(another-one-eha): delete this module. Callers should import
// directly from `daemon_proto`. Kept as a glob re-export so the
// extraction PR could land without touching every importer.
pub use daemon_proto::*;
