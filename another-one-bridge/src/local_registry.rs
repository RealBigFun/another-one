//! Process-wide handoff for the embedded daemon's `RegistryState`.
//!
//! `LocalSession` (in `api/local_session.rs`) needs read access to
//! the in-process daemon's project tree + tab state, but the
//! daemon's lifecycle is owned by the host binary — Phase 6 will be
//! the future Flutter desktop launcher; today's GPUI desktop already
//! constructs one. This module is the seam: the host binary calls
//! [`set_local_registry`] once at boot, and `LocalSession` consults
//! [`local_registry`] on each `local_connect`.
//!
//! Living outside `api/` so flutter_rust_bridge's codegen doesn't
//! try to scan or expose any of it. The setter is intentionally
//! Rust-only — Dart never holds an `Arc<Mutex<RegistryState>>`.

use std::sync::{Arc, Mutex, OnceLock};

use another_one_core::daemon_embed::RegistryState;

static LOCAL_REGISTRY: OnceLock<Arc<Mutex<RegistryState>>> = OnceLock::new();

/// Register the in-process daemon's `RegistryState` so subsequent
/// [`crate::api::local_session::local_connect`] calls can find it.
///
/// Call exactly once at host-binary startup, before any Dart
/// `local_connect()` reaches Rust. `OnceLock` semantics: a second
/// call is silently dropped and the first registration sticks.
/// Re-registering doesn't currently make sense because the daemon
/// thread keeps a strong reference to the state it was constructed
/// with — replacing the global wouldn't actually swap what the
/// daemon sees.
pub fn set_local_registry(registry: Arc<Mutex<RegistryState>>) {
    let _ = LOCAL_REGISTRY.set(registry);
}

/// Borrow the registered registry, if any. Returns `None` until
/// [`set_local_registry`] has been called — which `LocalSession`
/// surfaces to Dart as a clear error rather than silently
/// returning empty data.
///
/// `#[allow(dead_code)]` until the first `LocalSession` method
/// actually reads from the registry — landing in the next commit
/// that replaces `synthetic_project_list` with a real
/// `ProjectStore` flatten.
#[allow(dead_code)]
pub(crate) fn local_registry() -> Option<&'static Arc<Mutex<RegistryState>>> {
    LOCAL_REGISTRY.get()
}
