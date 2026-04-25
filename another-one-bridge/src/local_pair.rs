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

use std::sync::{Arc, OnceLock};

/// Adapter the host binary implements over its
/// `daemon_sandbox::EndpointHandle`. Three methods, all called from
/// FRB tasks on the bridge's tokio runtime.
pub trait LocalPairInfo: Send + Sync + 'static {
    /// Current pairing URL (rotates after [`Self::regenerate_pairing`]).
    fn pairing_url(&self) -> String;

    /// PNG bytes of the QR code encoding [`Self::pairing_url`].
    fn qr_png_bytes(&self) -> Vec<u8>;

    /// Roll a fresh TOFU nonce and rebuild URL + QR. Returns the
    /// underlying error as a string so the bridge doesn't have to
    /// surface a project-wide error type across the FFI.
    fn regenerate_pairing(&self) -> Result<(), String>;
}

static LOCAL_PAIR_INFO: OnceLock<Arc<dyn LocalPairInfo>> = OnceLock::new();

/// Register the embedded daemon's pairing source so subsequent
/// [`crate::api::pair::pairing_info`] calls can find it.
///
/// Call exactly once at host-binary startup, immediately after the
/// `EndpointHandle` is available. `OnceLock` semantics: a second
/// call is silently dropped and the first registration sticks.
pub fn set_local_pair_info(handle: Arc<dyn LocalPairInfo>) {
    let _ = LOCAL_PAIR_INFO.set(handle);
}

pub(crate) fn local_pair_info() -> Option<&'static Arc<dyn LocalPairInfo>> {
    LOCAL_PAIR_INFO.get()
}
