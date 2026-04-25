//! FRB-exposed entry point to boot the embedded daemon in-process.
//!
//! Called from the Flutter desktop's `main()` once at startup,
//! before any `local_connect()` reaches the FFI. Mobile platforms
//! never call this — they connect to remote daemons over iroh.
//!
//! See [`crate::embedded_daemon`] for the boot sequence; this is a
//! one-line FRB shim.

use crate::embedded_daemon;

/// Spawns the embedded iroh daemon on a dedicated OS thread and
/// registers its `RegistryState` + pair info with the bridge so
/// `LocalSession` and the pair-mobile FRB API can read from them.
/// Idempotent — a second call is a no-op.
pub fn boot_embedded_daemon() -> Result<(), String> {
    embedded_daemon::boot()
}
