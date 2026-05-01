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

const RECENT_OUTPUT_LIMIT_BYTES: usize = 16 * 1024;

fn trim_recent_output(buffer: &mut String) {
    if buffer.len() <= RECENT_OUTPUT_LIMIT_BYTES {
        return;
    }

    let mut drain_to = buffer.len() - RECENT_OUTPUT_LIMIT_BYTES;
    while drain_to < buffer.len() && !buffer.is_char_boundary(drain_to) {
        drain_to += 1;
    }
    buffer.drain(..drain_to);
}

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

    pub fn append_recent_output(&mut self, key: &TerminalRuntimeKey, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let buffer = self.recent_output.entry(key.clone()).or_default();
        buffer.push_str(&text);
        trim_recent_output(buffer);
    }

    pub fn clear_recent_output(&mut self, key: &TerminalRuntimeKey) {
        self.recent_output.remove(key);
    }

    pub fn mark_launch_started(&mut self, key: TerminalRuntimeKey) {
        self.pending_launches.insert(key.clone());
        self.errors.remove(&key);
        self.processes.remove(&key);
        self.clear_recent_output(&key);
    }

    pub fn mark_launch_succeeded(
        &mut self,
        key: TerminalRuntimeKey,
        process: Option<TrackedProcess>,
    ) {
        self.pending_launches.remove(&key);
        self.errors.remove(&key);
        if let Some(process) = process {
            self.processes.insert(key, process);
        } else {
            self.processes.remove(&key);
        }
    }

    pub fn mark_launch_failed(&mut self, key: TerminalRuntimeKey, error: String) {
        self.pending_launches.remove(&key);
        self.processes.remove(&key);
        self.errors.insert(key, error);
    }

    pub fn remove_session(&mut self, key: &TerminalRuntimeKey) {
        self.pending_launches.remove(key);
        self.processes.remove(key);
        self.errors.remove(key);
        self.recent_output.remove(key);
    }

    pub fn rename_session_key(
        &mut self,
        old_key: &TerminalRuntimeKey,
        new_key: TerminalRuntimeKey,
    ) {
        if let Some(process) = self.processes.remove(old_key) {
            self.processes.insert(new_key.clone(), process);
        }
        if self.pending_launches.remove(old_key) {
            self.pending_launches.insert(new_key.clone());
        }
        if let Some(output) = self.recent_output.remove(old_key) {
            self.recent_output.insert(new_key.clone(), output);
        }
        if let Some(error) = self.errors.remove(old_key) {
            self.errors.insert(new_key, error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section::SectionId;

    fn key(tab_id: &str) -> TerminalRuntimeKey {
        TerminalRuntimeKey {
            section_id: SectionId::new("project", "branch"),
            tab_id: tab_id.to_string(),
        }
    }

    #[test]
    fn launch_success_clears_pending_error_and_tracks_process() {
        let key = key("tab");
        let process = TrackedProcess {
            pid: 42,
            key: "proc".into(),
            label: "Agent".into(),
            project_key: "project".into(),
            project_label: "Project".into(),
            task_key: "task".into(),
            task_label: "Task".into(),
            icon_path: "icon.svg",
        };
        let mut manager = TerminalManager::new();
        manager.mark_launch_started(key.clone());
        manager.mark_launch_failed(key.clone(), "failed".into());
        manager.mark_launch_started(key.clone());
        manager.mark_launch_succeeded(key.clone(), Some(process));

        assert!(!manager.pending_launches.contains(&key));
        assert!(!manager.errors.contains_key(&key));
        assert!(manager.processes.contains_key(&key));
    }

    #[test]
    fn recent_output_is_trimmed_on_utf8_boundary() {
        let key = key("tab");
        let mut manager = TerminalManager::new();
        manager.append_recent_output(&key, "é".repeat(RECENT_OUTPUT_LIMIT_BYTES).as_bytes());

        let output = manager
            .recent_output
            .get(&key)
            .expect("output should exist");
        assert!(output.len() <= RECENT_OUTPUT_LIMIT_BYTES);
        assert!(output.is_char_boundary(0));
    }
}
