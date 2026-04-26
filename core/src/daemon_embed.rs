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
//! * The `TerminalRegistry` trait impl itself — its trait + the
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

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::project_store::ProjectStore;
use crate::section::SectionId;
use crate::terminal_types::TerminalRuntimeKey;

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
}

impl RegistryState {
    pub fn new(project_store: ProjectStore) -> Self {
        Self {
            project_store,
            broadcasts: HashMap::new(),
            writers: HashMap::new(),
            pending_resizes: Vec::new(),
            pending_tab_launches: Vec::new(),
            in_flight_launches: HashSet::new(),
            active_viewers: HashMap::new(),
            viewer_focus: HashMap::new(),
            effective_sizes: HashMap::new(),
            pending_post_launch_input: HashMap::new(),
        }
    }

    /// Recompute the min-across-viewers size for `key` and, if it
    /// changed since the last effective size, enqueue a resize for
    /// the UI render tick to apply. Returns the effective size so
    /// callers can log / debug — not otherwise used.
    pub fn recompute_effective_size(
        &mut self,
        key: &TerminalRuntimeKey,
    ) -> Option<(u16, u16)> {
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
