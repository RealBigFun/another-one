//! Bookkeeping state for the live terminal surface.
//!
//! Separates the GPUI-free half of per-tab terminal state —
//! recent-output ring buffer, last-error string, child-process
//! tracking, in-flight launch flags — from the GPUI-coupled
//! `LiveTerminalRuntime` (which owns the `alacritty_terminal::Term`,
//! the cached `TerminalSurfaceSnapshot` with its `gpui::Font` /
//! `Hsla` refs, and related rendering state).
//!
//! The desktop `AnotherOneApp` owns a `TerminalManager`; it still owns
//! the live-runtime map (`live_terminal_runtimes`) and the
//! render-snapshot map separately because those carry GPUI types that
//! can't travel to core. A future PR will carve the pure bookkeeping
//! off `LiveTerminalRuntime` and move it here too, leaving desktop
//! with a thin render wrapper.
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

/// Hard cap on the per-tab recent-output buffer. Used by
/// [`TerminalManager::append_recent_output`]; anything over this size
/// gets truncated from the front on a unicode-safe boundary. Matches
/// the `TERMINAL_RECENT_OUTPUT_LIMIT` that used to live on
/// `AnotherOneApp`.
pub const RECENT_OUTPUT_LIMIT: usize = 16 * 1024;

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

    // ---- recent_output -------------------------------------------------

    /// Append bytes to the tab's recent-output buffer, truncating from
    /// the front on a unicode-safe boundary if we exceed
    /// [`RECENT_OUTPUT_LIMIT`].
    pub fn append_recent_output(&mut self, key: &TerminalRuntimeKey, bytes: &[u8]) {
        let entry = self.recent_output.entry(key.clone()).or_default();
        // Append as-UTF-8-lossy so an occasional byte-split multibyte
        // doesn't cause a panic. The sampler caller already operates on
        // pre-decoded strings in practice, but this belt-and-suspenders
        // is cheap.
        entry.push_str(&String::from_utf8_lossy(bytes));
        if entry.len() > RECENT_OUTPUT_LIMIT {
            let min_start = entry.len() - RECENT_OUTPUT_LIMIT;
            let start = entry
                .char_indices()
                .map(|(idx, _)| idx)
                .find(|&idx| idx >= min_start)
                .unwrap_or(entry.len());
            entry.drain(..start);
        }
    }

    /// Drop the tab's recent-output buffer entirely.
    pub fn clear_recent_output(&mut self, key: &TerminalRuntimeKey) {
        self.recent_output.remove(key);
    }

    pub fn recent_output_for(&self, key: &TerminalRuntimeKey) -> Option<&str> {
        self.recent_output.get(key).map(String::as_str)
    }

    // ---- errors --------------------------------------------------------

    pub fn set_error(&mut self, key: TerminalRuntimeKey, message: String) {
        self.errors.insert(key, message);
    }

    pub fn clear_error(&mut self, key: &TerminalRuntimeKey) {
        self.errors.remove(key);
    }

    pub fn error_for(&self, key: &TerminalRuntimeKey) -> Option<&str> {
        self.errors.get(key).map(String::as_str)
    }

    // ---- processes -----------------------------------------------------

    pub fn insert_process(&mut self, key: TerminalRuntimeKey, process: TrackedProcess) {
        self.processes.insert(key, process);
    }

    pub fn remove_process(&mut self, key: &TerminalRuntimeKey) -> Option<TrackedProcess> {
        self.processes.remove(key)
    }

    pub fn process_for(&self, key: &TerminalRuntimeKey) -> Option<&TrackedProcess> {
        self.processes.get(key)
    }

    // ---- pending launches ---------------------------------------------

    pub fn mark_pending(&mut self, key: TerminalRuntimeKey) {
        self.pending_launches.insert(key);
    }

    pub fn clear_pending(&mut self, key: &TerminalRuntimeKey) {
        self.pending_launches.remove(key);
    }

    pub fn is_pending(&self, key: &TerminalRuntimeKey) -> bool {
        self.pending_launches.contains(key)
    }

    // ---- composite cleanup --------------------------------------------

    /// Drop every entry keyed by `key`: recent output, last error,
    /// tracked process, pending-launch flag. Called when a tab is
    /// being torn down so no stale map entries linger.
    pub fn remove_all_for(&mut self, key: &TerminalRuntimeKey) {
        self.recent_output.remove(key);
        self.errors.remove(key);
        self.processes.remove(key);
        self.pending_launches.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section::SectionId;

    fn key(tab: &str) -> TerminalRuntimeKey {
        TerminalRuntimeKey {
            section_id: SectionId::new("proj", "main"),
            tab_id: tab.to_string(),
        }
    }

    #[test]
    fn recent_output_trims_to_limit() {
        let mut mgr = TerminalManager::new();
        let k = key("t1");
        // Append 2x the limit's worth.
        let chunk = "a".repeat(RECENT_OUTPUT_LIMIT);
        mgr.append_recent_output(&k, chunk.as_bytes());
        mgr.append_recent_output(&k, chunk.as_bytes());
        let out = mgr.recent_output_for(&k).unwrap();
        assert!(out.len() <= RECENT_OUTPUT_LIMIT);
    }

    #[test]
    fn remove_all_for_clears_every_axis() {
        let mut mgr = TerminalManager::new();
        let k = key("t1");
        mgr.append_recent_output(&k, b"hello");
        mgr.set_error(k.clone(), "boom".into());
        mgr.mark_pending(k.clone());
        mgr.insert_process(
            k.clone(),
            TrackedProcess {
                pid: 42,
                key: "k".into(),
                label: "l".into(),
                project_key: "p".into(),
                project_label: "P".into(),
                task_key: "t".into(),
                task_label: "T".into(),
                icon_path: "x",
            },
        );

        mgr.remove_all_for(&k);

        assert!(mgr.recent_output_for(&k).is_none());
        assert!(mgr.error_for(&k).is_none());
        assert!(!mgr.is_pending(&k));
        assert!(mgr.process_for(&k).is_none());
    }

    #[test]
    fn recent_output_trims_on_unicode_boundary() {
        let mut mgr = TerminalManager::new();
        let k = key("t1");
        // Multibyte chars right at the limit boundary — verify no panic
        // and the trimmed start lands on a char boundary (otherwise
        // `String` would UTF-8-reject).
        let long = "🙂".repeat(RECENT_OUTPUT_LIMIT); // 4 bytes per char
        mgr.append_recent_output(&k, long.as_bytes());
        let out = mgr.recent_output_for(&k).unwrap();
        assert!(out.len() <= RECENT_OUTPUT_LIMIT);
        // Round-trip parse — will blow up if trim misaligned.
        let _ = out.chars().count();
    }
}
