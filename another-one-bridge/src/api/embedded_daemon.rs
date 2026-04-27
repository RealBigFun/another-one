//! FRB-exposed entry points for the embedded daemon.
//!
//! Two functions land on Dart:
//!
//!   * [`boot_embedded_daemon`] — spawn the daemon thread.
//!     Called from the Flutter desktop's `main()` once at startup,
//!     before any `iroh_connect`. Mobile platforms never call this
//!     — they connect to remote daemons over iroh.
//!   * [`loopback_session_addr`] — read the running daemon's iroh
//!     address so the desktop UI can construct an `IrohTransport`
//!     pointing at it (`another-one-ojm.9` loopback bootstrap).
//!     Returns `None` until the daemon thread has finished binding
//!     its endpoint; the Dart bootstrap polls until `Some` arrives.

use crate::embedded_daemon;
use crate::local_pair::local_pair_info;

/// Spawns the embedded iroh daemon on a dedicated OS thread and
/// registers its `RegistryState` + pair info with the bridge so the
/// pair-mobile FRB API and the loopback bootstrap can read from
/// them. Idempotent — a second call is a no-op.
pub fn boot_embedded_daemon() -> Result<(), String> {
    embedded_daemon::boot()
}

/// Shut down the embedded daemon endpoint and clear the local
/// registry/pairing handoffs. Idempotent; primarily used by hot
/// restart and tests.
pub fn shutdown_embedded_daemon() {
    embedded_daemon::shutdown();
}

/// Hex-encoded `EndpointId` + direct socket addresses + relay URLs
/// of the running embedded daemon. Drives the desktop's loopback
/// bootstrap: the UI's `DaemonConnection` is an `IrohTransport`
/// dialling this address, exercising the same wire surface mobile
/// uses.
pub struct LoopbackSessionAddr {
    pub endpoint_id: String,
    pub direct_addrs: Vec<String>,
    pub relay_urls: Vec<String>,
}

/// Snapshot of the embedded daemon's iroh address. Returns `None`
/// until [`boot_embedded_daemon`] has finished binding the endpoint
/// (typically a few hundred milliseconds after `boot`).
pub fn loopback_session_addr() -> Option<LoopbackSessionAddr> {
    let info = local_pair_info()?;
    Some(LoopbackSessionAddr {
        endpoint_id: info.endpoint_id(),
        direct_addrs: info.direct_addrs(),
        relay_urls: info.relay_urls(),
    })
}

/// Block until the embedded daemon has bound its iroh endpoint and
/// return its loopback address. Polls [`loopback_session_addr`] on
/// the bridge's tokio runtime so the FRB worker thread stays free.
/// Times out after `timeout_ms` (caller-supplied; the Dart bootstrap
/// passes 10 000 ms).
///
/// Errors:
///   * `boot_embedded_daemon` was never called (daemon thread isn't
///     running) — returns the timeout error after the deadline.
///   * The daemon thread failed before binding its endpoint — returns
///     the recorded startup error immediately instead of collapsing
///     it into a generic timeout.
pub async fn await_loopback_session_addr(timeout_ms: u32) -> Result<LoopbackSessionAddr, String> {
    use std::time::Duration;
    use std::time::Instant;

    let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
    loop {
        if let Some(addr) = loopback_session_addr() {
            return Ok(addr);
        }
        if let Some(error) = embedded_daemon::boot_error() {
            return Err(format!("embedded daemon boot failed: {error}"));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "embedded daemon did not bind within {timeout_ms} ms — \
                 boot_embedded_daemon may not have been called or the daemon \
                 thread may not have reached its endpoint bind"
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
