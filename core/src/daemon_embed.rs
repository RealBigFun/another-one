//! Headless half of the embedded iroh daemon host.
//!
//! Desktop is GPUI-only — no ambient tokio runtime — so booting the
//! `daemon-sandbox` library requires bringing our own runtime. The
//! parts of that wiring that don't touch GPUI live here so other UI
//! shells (the future Flutter app) can reuse them.
//!
//! What lives here:
//!
//! * [`RegistryState`] — shared state the registry trait object reads
//!   (projects, live broadcast senders, live writers, pending resize
//!   requests). Wrapped in an `Arc<Mutex<…>>` so the daemon's tokio
//!   tasks can query it without cx access; the UI side mutates the
//!   same mutex on every `TerminalLaunchReply::Launched` /
//!   `…::Terminated` / tab-close.
//! * [`TabResizeRequest`] / [`TabLaunchRequest`] — pure data records
//!   the registry trait impl pushes onto `RegistryState` for the UI
//!   render tick to drain.
//! * [`daemon_paths`] / [`paired_peers_path`] — XDG-rooted on-disk
//!   path resolution for the daemon's identity + TOFU allowlist.
//! * [`key_from_wire`] — small helper that parses a wire
//!   `(section_id, tab_id)` pair into a [`TerminalRuntimeKey`].
//!
//! What does *not* live here (stays in `desktop/src/daemon_host.rs`):
//!
//! * The `DaemonRegistry` trait impl itself — its trait + the
//!   wire-summary types (`ProjectSummary`, `TabSummary`, etc.) come
//!   from `daemon-sandbox`, which already depends on
//!   `another-one-core`; moving the impl here would create a cycle.
//! * The `tokio::runtime::Runtime` + `run_endpoint` boot sequence —
//!   same reason; it imports `daemon_sandbox::run_endpoint` and
//!   `daemon_sandbox::transport_mcp`.
//! * The `spawn(...)` wrapper that returns the
//!   `mpsc::Receiver<EndpointHandle>` — `EndpointHandle` is a
//!   `daemon-sandbox` type, and the receiver is consumed by the
//!   GPUI render tick on `AnotherOneApp`.
//!
//! Resize is intentionally *not* executed on the tokio thread: the
//! live `MasterPty` lives inside the UI's terminal runtime. Instead,
//! `tab_resize` enqueues a [`TabResizeRequest`] on the state struct,
//! and the UI render tick drains it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::agents::TerminalRestoreStatus;
use crate::process::TrackedProcess;
use crate::project_store::ProjectStore;
use crate::resource_usage::{ResourceUsageSampler, ResourceUsageSnapshot};
use crate::section::SectionId;
use crate::terminal_types::TerminalRuntimeKey;

const TERMINAL_REPLAY_MAX_BYTES: usize = 512 * 1024;
const TERMINAL_REPLAY_MAX_CHUNKS: usize = 128;

/// `viewer_id` used for the in-process desktop view. Stable across the
/// app's lifetime; the app exits before it would ever need to disconnect.
pub const DESKTOP_LOCAL_VIEWER_ID: &str = "desktop-local";

/// Shared state the UI thread writes and the daemon's tokio tasks
/// read. Everything behind one `Mutex` because contention is
/// negligible at PTY-launch rates (tens per session), whereas keeping
/// projects/broadcasts/writers in sync would require multiple locks
/// to be held in order and is fragile to refactor later.
pub struct RegistryState {
    /// Snapshot of the desktop's projects/tasks/tabs, refreshed from
    /// the UI's project store on every mutation. The daemon's
    /// `ListProjects` handler reads directly from this snapshot so it
    /// doesn't need to post work back to the UI thread.
    pub project_store: ProjectStore,
    /// Per-tab PTY output broadcast senders, cloned from the
    /// launcher's `PreparedTerminalRuntime::output_broadcast`. Mobile
    /// `AttachTab` subscribes to the matching sender.
    pub broadcasts: HashMap<TerminalRuntimeKey, broadcast::Sender<Vec<u8>>>,
    /// Bounded tail of raw PTY output per tab. A remote/client attach
    /// gets this replay before live bytes so output emitted during the
    /// launch/attach race is not lost.
    pub terminal_replay: HashMap<TerminalRuntimeKey, TerminalReplayBuffer>,
    /// Last replay sequence each viewer has already observed for a
    /// tab. This prevents a tab switch from re-sending the full replay
    /// into a client-side terminal engine that is intentionally kept
    /// alive across tab mounts.
    pub viewer_output_cursors: HashMap<(String, TerminalRuntimeKey), u64>,
    /// Per-tab master-PTY writer handles shared with the live
    /// terminal runtime. Mobile keystrokes flow through these
    /// exactly like desktop keystrokes do.
    pub writers: HashMap<TerminalRuntimeKey, Arc<Mutex<Box<dyn Write + Send>>>>,
    /// Resize requests queued by the daemon thread; drained on the
    /// UI render tick where the live terminal runtime's `resize` is
    /// safe to call.
    pub pending_resizes: Vec<TabResizeRequest>,
    /// Per-tab set of currently-attached viewers and the viewport
    /// size each wants. The PTY for a tab is resized to the **min**
    /// across the viewer entries here so a wide desktop window can't
    /// make the PTY too wide for a phone to render. A viewer
    /// appears in at most one tab's map at a time (switching
    /// focused tabs clears the prior entry); leaving the session
    /// clears every entry for that viewer.
    pub active_viewers: HashMap<TerminalRuntimeKey, HashMap<String, (u16, u16)>>,
    /// Tracks which tab each viewer currently has in focus — used to
    /// clear their prior entry when they switch or detach.
    pub viewer_focus: HashMap<String, TerminalRuntimeKey>,
    /// Last effective size applied to each tab's PTY; avoids
    /// re-enqueueing identical resize requests on every keystroke.
    pub effective_sizes: HashMap<TerminalRuntimeKey, (u16, u16)>,
    /// Tab-launch requests from any client (mobile). Drained on the
    /// UI render tick, where the task's persisted `launch_config`
    /// is resolved from the project store and the PTY is spawned via
    /// `spawn_terminal_launch`. Desktop sidebar clicks go through a
    /// different path today for legacy reasons; both produce the same
    /// end state (a live entry in `broadcasts` + `writers`).
    pub pending_tab_launches: Vec<TabLaunchRequest>,
    /// Tab-termination requests from daemon-side mutators (close-tab,
    /// future task/section removal cleanup). Drained on the PTY
    /// thread, where dropping the live runtime safely SIGHUPs the
    /// child and clears registry bookkeeping for the key.
    pub pending_tab_terminations: Vec<TerminalRuntimeKey>,
    /// Keys currently mid-spawn. Populated when either path
    /// (daemon-queued mobile LaunchTab **or** desktop sidebar click)
    /// kicks off a `spawn_terminal_launch`; cleared on
    /// `TerminalLaunchReply::Launched` / `Failed` / tab close. The
    /// daemon checks this to dedupe — earlier builds only checked
    /// `pending_tab_launches` + `broadcasts`, which left a window
    /// between "spawn kicked off" and "Launched reply observed"
    /// where a second LaunchTab would spawn a duplicate PTY.
    pub in_flight_launches: HashSet<TerminalRuntimeKey>,
    /// Bytes to write to a tab's PTY immediately after its first
    /// `Launched` reply. Used by custom-action shell runs: the Flutter
    /// (or future bridge) caller queues a launch + records the
    /// command line here; GPUI's drain on `Launched` writes the bytes
    /// once and removes the entry so a re-launch doesn't replay them.
    pub pending_post_launch_input: HashMap<TerminalRuntimeKey, Vec<u8>>,
    /// Per-tab tracked processes — populated by the bridge's PTY drain
    /// on `Launched` (with project / task labels resolved through the
    /// store) and removed on `Exited` / `Failed`. Fed into
    /// [`ResourceUsageSampler::sample`] every tick so the resource
    /// indicator's tree groups CPU + memory by project → task →
    /// session, mirroring the GPUI desktop's `ResourceIndicator`.
    pub tracked_processes: HashMap<TerminalRuntimeKey, TrackedProcess>,
    /// Hierarchical CPU + memory sampler. Holds previous-tick CPU
    /// samples internally for delta computation; we keep one instance
    /// in the registry rather than rebuilding it each call so the
    /// CPU% values aren't stuck at 0 every poll.
    pub resource_sampler: ResourceUsageSampler,
}

impl RegistryState {
    pub fn new(project_store: ProjectStore) -> Self {
        Self {
            project_store,
            broadcasts: HashMap::new(),
            terminal_replay: HashMap::new(),
            viewer_output_cursors: HashMap::new(),
            writers: HashMap::new(),
            pending_resizes: Vec::new(),
            pending_tab_launches: Vec::new(),
            pending_tab_terminations: Vec::new(),
            in_flight_launches: HashSet::new(),
            active_viewers: HashMap::new(),
            viewer_focus: HashMap::new(),
            effective_sizes: HashMap::new(),
            pending_post_launch_input: HashMap::new(),
            tracked_processes: HashMap::new(),
            resource_sampler: ResourceUsageSampler::default(),
        }
    }

    /// Take one resource-usage sample. Walks every PID under the host
    /// UI process plus every tracked PTY child, deltas CPU times
    /// against the previous tick, and groups the result into a
    /// project → task → session tree.
    pub fn sample_resource_usage(&mut self, app_pid: u32) -> ResourceUsageSnapshot {
        let tracked = self.tracked_processes.values().cloned().collect::<Vec<_>>();
        self.resource_sampler.sample(app_pid, &tracked)
    }

    /// Mark a live tab unusable after a write-side PTY failure. This
    /// clears daemon-visible runtime state immediately and queues a
    /// termination so the PTY drain drops its `PreparedTerminalRuntime`
    /// handle on the next tick.
    pub fn fail_tab_io(
        &mut self,
        key: &TerminalRuntimeKey,
        message: impl Into<String>,
        details: impl Into<String>,
    ) {
        self.broadcasts.remove(key);
        self.terminal_replay.remove(key);
        self.writers.remove(key);
        self.in_flight_launches.remove(key);
        self.pending_tab_launches
            .retain(|request| request.key != *key);
        self.pending_resizes.retain(|request| request.key != *key);
        self.pending_post_launch_input.remove(key);
        self.tracked_processes.remove(key);
        self.active_viewers.remove(key);
        self.effective_sizes.remove(key);
        self.viewer_output_cursors
            .retain(|(_, cursor_key), _| cursor_key != key);
        self.viewer_focus
            .retain(|_, focused_key| focused_key != key);
        if !self
            .pending_tab_terminations
            .iter()
            .any(|queued| queued == key)
        {
            self.pending_tab_terminations.push(key.clone());
        }
        let section_key = key.section_id.store_key();
        self.project_store.set_tab_restore_status(
            &section_key,
            &key.tab_id,
            TerminalRestoreStatus::Failed,
            Some(message.into()),
            Some(details.into()),
        );
    }

    /// Recompute the min-across-viewers size for `key` and, if it
    /// changed since the last effective size, enqueue a resize for
    /// the UI render tick to apply. Returns the effective size so
    /// callers can log / debug — not otherwise used.
    pub fn recompute_effective_size(&mut self, key: &TerminalRuntimeKey) -> Option<(u16, u16)> {
        let viewers = self.active_viewers.get(key)?;
        if viewers.is_empty() {
            return None;
        }
        let (cols, rows) = viewers
            .values()
            .fold((u16::MAX, u16::MAX), |(c, r), (vc, vr)| {
                (c.min(*vc), r.min(*vr))
            });
        let effective = (cols.max(1), rows.max(1));
        if self.effective_sizes.get(key).copied() == Some(effective) {
            return Some(effective);
        }
        self.effective_sizes.insert(key.clone(), effective);
        self.pending_resizes.push(TabResizeRequest {
            key: key.clone(),
            cols: effective.0,
            rows: effective.1,
        });
        Some(effective)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TerminalReplayChunk {
    sequence: u64,
    bytes: Vec<u8>,
}

/// Bounded raw PTY output tail for a live tab.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalReplayBuffer {
    chunks: VecDeque<TerminalReplayChunk>,
    bytes: usize,
    next_sequence: u64,
}

impl TerminalReplayBuffer {
    pub fn push(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        self.bytes = self.bytes.saturating_add(bytes.len());
        self.chunks.push_back(TerminalReplayChunk {
            sequence: self.next_sequence,
            bytes,
        });
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.evict_excess();
    }

    pub fn replay_after(&self, last_seen: Option<u64>) -> Vec<Vec<u8>> {
        self.chunks
            .iter()
            .filter(|chunk| match last_seen {
                Some(sequence) => chunk.sequence > sequence,
                None => true,
            })
            .map(|chunk| chunk.bytes.clone())
            .collect()
    }

    pub fn tail_bytes(&self, tail: usize) -> (Vec<u8>, bool) {
        let tail = tail.min(TERMINAL_REPLAY_MAX_BYTES);
        if tail == 0 {
            return (Vec::new(), self.bytes > 0);
        }

        let mut remaining = tail;
        let mut chunks = Vec::new();
        for chunk in self.chunks.iter().rev() {
            if remaining == 0 {
                break;
            }
            let bytes = if chunk.bytes.len() <= remaining {
                chunk.bytes.clone()
            } else {
                chunk.bytes[chunk.bytes.len() - remaining..].to_vec()
            };
            remaining = remaining.saturating_sub(bytes.len());
            chunks.push(bytes);
        }
        chunks.reverse();

        let len = chunks.iter().map(Vec::len).sum();
        let mut bytes = Vec::with_capacity(len);
        for chunk in chunks {
            bytes.extend(chunk);
        }
        let truncated_head = self.bytes > bytes.len();
        (bytes, truncated_head)
    }

    pub fn latest_sequence(&self) -> Option<u64> {
        self.next_sequence.checked_sub(1)
    }

    fn evict_excess(&mut self) {
        while self.chunks.len() > TERMINAL_REPLAY_MAX_CHUNKS
            || self.bytes > TERMINAL_REPLAY_MAX_BYTES
        {
            let Some(chunk) = self.chunks.pop_front() else {
                self.bytes = 0;
                return;
            };
            self.bytes = self.bytes.saturating_sub(chunk.bytes.len());
        }
    }
}

/// A "please launch this tab" ask from a remote client. Same shape
/// as the sidebar-click path on the desktop would produce, minus the
/// GUI-level affordances (active-page toggling, etc.).
#[derive(Clone, Debug)]
pub struct TabLaunchRequest {
    pub key: TerminalRuntimeKey,
}

/// A pending tab resize request from a mobile client. The daemon's
/// `tab_resize` impl pushes one of these onto
/// `RegistryState.pending_resizes`; the UI thread drains them on the
/// render tick and forwards to the live terminal runtime's `resize`.
#[derive(Clone, Debug)]
pub struct TabResizeRequest {
    pub key: TerminalRuntimeKey,
    pub cols: u16,
    pub rows: u16,
}

/// Parse a wire `section_id` (a `SectionId::store_key()`) + `tab_id`
/// into a `TerminalRuntimeKey`. Returns `None` if the section key is
/// malformed — the daemon will treat the tab as unknown.
pub fn key_from_wire(section_id: &str, tab_id: &str) -> Option<TerminalRuntimeKey> {
    let section = SectionId::from_store_key(section_id)?;
    Some(TerminalRuntimeKey {
        section_id: section,
        tab_id: tab_id.to_string(),
    })
}

/// On-disk paths the daemon needs at boot. Resolved under
/// `…/another-one/daemon/` so an embedded daemon (running alongside
/// the regular AnotherOne config) doesn't collide with a standalone
/// `daemon-sandbox` running on the same machine.
pub struct DaemonPaths {
    pub secret_key: PathBuf,
    pub paired_peers: PathBuf,
}

/// Public accessor for the allowlist path so the "Pair mobile" modal's
/// reset button can unlink it. Thin wrapper; same resolution as the
/// daemon uses at boot.
pub fn paired_peers_path() -> anyhow::Result<PathBuf> {
    Ok(daemon_paths()?.paired_peers)
}

/// Resolve the on-disk paths for the daemon's identity + TOFU
/// allowlist. Mirrors the sandbox binary's resolution logic, but
/// roots the directory under `…/another-one/daemon/` so an embedded
/// daemon (running alongside the regular AnotherOne config) doesn't
/// collide with a standalone `daemon-sandbox` running on the same
/// machine.
pub fn daemon_paths() -> anyhow::Result<DaemonPaths> {
    let base = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else {
        let home = std::env::var("HOME")
            .map_err(|_| anyhow::anyhow!("HOME is unset — can't locate daemon config dir"))?;
        PathBuf::from(home).join(".config")
    };
    let dir = base.join("another-one").join("daemon");
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow::anyhow!("create daemon dir {}: {e}", dir.display()))?;
    Ok(DaemonPaths {
        secret_key: dir.join("secret_key"),
        paired_peers: dir.join("paired_peers"),
    })
}

#[cfg(test)]
mod tests {
    use super::{TerminalReplayBuffer, TERMINAL_REPLAY_MAX_CHUNKS};

    #[test]
    fn terminal_replay_returns_only_chunks_after_cursor() {
        let mut replay = TerminalReplayBuffer::default();
        replay.push(b"first".to_vec());
        let first_sequence = replay.latest_sequence();
        replay.push(b"second".to_vec());

        assert_eq!(
            replay.replay_after(None),
            vec![b"first".to_vec(), b"second".to_vec()]
        );
        assert_eq!(
            replay.replay_after(first_sequence),
            vec![b"second".to_vec()]
        );
    }

    #[test]
    fn terminal_replay_evicts_old_chunks() {
        let mut replay = TerminalReplayBuffer::default();
        for index in 0..(TERMINAL_REPLAY_MAX_CHUNKS + 1) {
            replay.push(vec![index as u8]);
        }

        assert_eq!(replay.replay_after(None).len(), TERMINAL_REPLAY_MAX_CHUNKS);
        assert_eq!(replay.replay_after(None).first().cloned(), Some(vec![1]));
    }

    #[test]
    fn terminal_replay_tail_bytes_reports_truncation() {
        let mut replay = TerminalReplayBuffer::default();
        replay.push(b"hello ".to_vec());
        replay.push(b"world".to_vec());

        let (bytes, truncated) = replay.tail_bytes(5);

        assert_eq!(bytes, b"world");
        assert!(truncated);
    }
}
