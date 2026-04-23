//! Bookkeeping state for the live terminal surface.
//!
//! Separates the GPUI-free half of per-tab terminal state — the
//! recent-output ring buffer, last-error string, child-process
//! tracking, in-flight launch flags — from the GPUI-coupled
//! `LiveTerminalRuntime` (which owns the `alacritty_terminal::Term`,
//! the cached `TerminalSurfaceSnapshot` with its `gpui::Font` /
//! `Hsla` refs, and related rendering state).
//!
//! Data-only on purpose: desktop call sites poke the maps directly.
//! This is a structural move from scattered fields on
//! `AnotherOneApp` into one container, not (yet) an encapsulation
//! story — helpers and invariants will grow organically as more of
//! Phase 1 lands and `LiveTerminalRuntime` itself carves apart.
//!
//! # Not thread-safe
//!
//! All writes come from the main GPUI thread. Background launcher
//! threads in `terminal_launch` communicate results via `mpsc` — they
//! never hold a reference to this struct — so there is no need for
//! `Arc<Mutex<_>>` around the maps.

use std::collections::{HashMap, HashSet};

use crate::process::TrackedProcess;
use crate::terminal_types::TerminalRuntimeKey;

/// Aggregates per-tab terminal state that doesn't touch GPUI.
#[derive(Debug, Default)]
pub struct TerminalManager {
    /// Ring-ish buffer of the last ~16 KiB of UTF-8 output per tab.
    /// Used to detect restore failures (e.g. "conversation not found"
    /// messages) and for diagnostics.
    pub recent_output: HashMap<TerminalRuntimeKey, String>,

    /// Last launch/exit error string per tab, if any. Cleared when a
    /// fresh launch succeeds.
    pub errors: HashMap<TerminalRuntimeKey, String>,

    /// Child PIDs (plus labeling metadata) for active agent sessions.
    /// Fed into the resource-usage sampler so the UI can show per-tab
    /// CPU and memory.
    pub processes: HashMap<TerminalRuntimeKey, TrackedProcess>,

    /// Tabs with an in-flight launch that hasn't reported back yet.
    /// Drives the "launching…" placeholder in the UI.
    pub pending_launches: HashSet<TerminalRuntimeKey>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self::default()
    }
}
