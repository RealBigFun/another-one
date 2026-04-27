//! Process-wide handoff for the embedded daemon's pairing material
//! (URL + QR PNG + reset).
//!
//! Sibling to [`crate::local_registry`]. Same shape: the host
//! binary registers a source once at boot, FRB API methods consult
//! it on demand. Held behind a trait so the bridge crate doesn't
//! depend on `daemon-sandbox` directly — the host owns the
//! [`daemon_sandbox::EndpointHandle`] and adapts it into
//! [`LocalPairInfo`].
//!
//! Not exposed to Dart. Lives outside `api/` so flutter_rust_bridge's
//! codegen ignores it; the FRB-exposed surface is in
//! `api/pair.rs`, which calls these accessors internally.

use std::sync::{Arc, Mutex, OnceLock};

/// Adapter the host binary implements over its
/// `daemon_sandbox::EndpointHandle`. All methods are called from FRB
/// tasks on the bridge's tokio runtime.
///
/// The pairing-URL/QR-PNG triplet drives the desktop's "Pair mobile"
/// modal. The endpoint-id / direct-addrs / relay-urls triplet drives
/// the loopback bootstrap (`another-one-ojm.9`) — the desktop UI's
/// own iroh `DaemonConnection` dials the embedded daemon at this
/// address so every screen exercises the same wire surface mobile
/// uses.
pub trait LocalPairInfo: Send + Sync + 'static {
    /// Current pairing URL (rotates after [`Self::regenerate_pairing`]).
    fn pairing_url(&self) -> String;

    /// PNG bytes of the QR code encoding [`Self::pairing_url`].
    fn qr_png_bytes(&self) -> Vec<u8>;

    /// Roll a fresh TOFU nonce and rebuild URL + QR. Returns the
    /// underlying error as a string so the bridge doesn't have to
    /// surface a project-wide error type across the FFI.
    fn regenerate_pairing(&self) -> Result<(), String>;

    /// Hex-encoded `EndpointId` of the running daemon. Stable across
    /// boots once the daemon's secret key file exists.
    fn endpoint_id(&self) -> String;

    /// Direct socket addresses (`ip:port`) the daemon is reachable
    /// through. May be empty on a host with no usable network
    /// interfaces, in which case the loopback bootstrap can't dial.
    fn direct_addrs(&self) -> Vec<String>;

    /// Relay URLs the daemon publishes itself through. Empty for the
    /// embedded daemon today — `presets::Minimal` skips relay
    /// publishing — but kept on the trait for shape symmetry with the
    /// mobile pairing path.
    fn relay_urls(&self) -> Vec<String>;
}

static LOCAL_PAIR_INFO: OnceLock<Mutex<Option<Arc<dyn LocalPairInfo>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Arc<dyn LocalPairInfo>>> {
    LOCAL_PAIR_INFO.get_or_init(|| Mutex::new(None))
}

/// Register the embedded daemon's pairing source so subsequent
/// [`crate::api::pair::pairing_info`] calls can find it.
///
/// Called when the embedded daemon endpoint is available. Replaces
/// any prior handle so hot restart / explicit shutdown can install a
/// fresh endpoint without leaving stale pairing material behind.
pub fn set_local_pair_info(handle: Arc<dyn LocalPairInfo>) {
    let mut guard = slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(handle);
}

pub fn clear_local_pair_info() {
    let mut guard = slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

pub(crate) fn local_pair_info() -> Option<Arc<dyn LocalPairInfo>> {
    slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}
