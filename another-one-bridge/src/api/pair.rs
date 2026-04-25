//! FRB-exposed pairing surface for the embedded daemon.
//!
//! Reads from a host-registered [`crate::local_pair::LocalPairInfo`]
//! (see that module for boot-order semantics). Boot-order forgiving
//! — if the host hasn't registered a source yet (e.g. embedded
//! daemon still starting), [`pairing_info`] returns `None` and the
//! UI shows a "daemon not ready" empty state.

use crate::local_pair;

/// Snapshot of the embedded daemon's current pairing material.
/// Stable for one render of the pair-mobile modal; refetch after
/// [`regenerate_local_pairing`] to pick up the rotated nonce.
pub struct PairingInfo {
    pub url: String,
    pub qr_png_bytes: Vec<u8>,
}

/// Current pairing material, or `None` if the host hasn't registered
/// the embedded daemon yet (boot race, or the binary was built
/// without daemon-sandbox).
pub fn pairing_info() -> Option<PairingInfo> {
    let handle = local_pair::local_pair_info()?;
    Some(PairingInfo {
        url: handle.pairing_url(),
        qr_png_bytes: handle.qr_png_bytes(),
    })
}

/// Roll a fresh TOFU nonce and rebuild the pairing URL + QR. Errors
/// surface as a string so the bridge doesn't need to expose a
/// project-wide error type to Dart.
pub fn regenerate_local_pairing() -> Result<(), String> {
    match local_pair::local_pair_info() {
        Some(handle) => handle.regenerate_pairing(),
        None => Err("embedded daemon not registered".to_string()),
    }
}
