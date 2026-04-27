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

static LOCAL_REGISTRY: OnceLock<Mutex<Option<Arc<Mutex<RegistryState>>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Arc<Mutex<RegistryState>>>> {
    LOCAL_REGISTRY.get_or_init(|| Mutex::new(None))
}

/// Register the in-process daemon's `RegistryState` so subsequent
/// [`crate::api::local_session::local_connect`] calls can find it.
///
/// Called when the embedded daemon host creates its registry. This
/// replaces any previous handle so explicit shutdown/hot restart can
/// drop stale state before installing a fresh daemon registry.
pub fn set_local_registry(registry: Arc<Mutex<RegistryState>>) {
    let mut guard = slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(registry);
}

pub fn clear_local_registry() {
    let mut guard = slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

/// Clone the registered registry, if any. Returns `None` until
/// [`set_local_registry`] has been called.
pub(crate) fn local_registry() -> Option<Arc<Mutex<RegistryState>>> {
    slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}
