//! Simple length-prefixed framing for the Iroh bidi stream.
//!
//! Wire format: `[1 byte type][4 bytes BE length][N bytes payload]`.
//!
//! Types:
//! - `0x00` — PTY data (raw bytes, either direction). This includes
//!   keyboard input, paste payloads, and terminal mouse protocol
//!   escape sequences; the daemon deliberately does not parse them.
//! - `0x01` — JSON control message (UTF-8; see [`Control`])
//! - `0x02` — JSON worker reply (UTF-8; see [`WorkerReply`]).
//!   Daemon → client only. One variant per core-extracted worker that
//!   the daemon forwards to the client. Unknown variants MUST be
//!   ignored by clients so older clients keep working as we add
//!   workers.
//!
//! Used by both the server ([`super::transport_iroh`]) and the client
//! smoke test (`bin/iroh-client.rs`). See [[docs/architecture/transport-abstraction]].

// TerminalRestoreStatus is defined below; core re-exports it from here.

use serde::{Deserialize, Serialize};

pub const TY_DATA: u8 = 0x00;
pub const TY_CONTROL: u8 = 0x01;
pub const TY_WORKER_REPLY: u8 = 0x02;

/// Wire layout inside every [`TY_DATA`] frame payload (both
/// daemon→client PTY output and client→daemon user input).
///
///   +--2--+-- section_id (utf8) --+--2--+-- tab_id (utf8) --+-- pty bytes --+
///   | len |          ...          | len |        ...        |     ...       |
///   +-----+-----------------------+-----+-------------------+---------------+
///
/// `u16` lengths are big-endian. Both ids are short (≤80 bytes in
/// practice: `SectionId`'s store key + a small integer tab id), so
/// a 2-byte length is comfortable headroom and never crowds the
/// 64 KiB [`MAX_FRAME_BYTES`] cap.
///
/// Before #138 the payload was the raw PTY bytes alone and the
/// daemon's `AttachState` resolved "which tab does this belong to"
/// from session state — which races when the client switches
/// attach mid-stream, silently rendering old-tab bytes into the
/// new tab. Tagging every frame with `(section_id, tab_id)` lets
/// the receiver demux authoritatively and drop stale frames
/// without guessing.
pub fn encode_pty_data(section_id: &str, tab_id: &str, bytes: &[u8]) -> Vec<u8> {
    // `u16::MAX` is 64 KiB — either id overflowing it would blow
    // the frame cap anyway, so we can safely truncate the length
    // to 16 bits; guard with a debug assert so tests surface any
    // pathological id before a release build silently ships bad
    // wire bytes.
    debug_assert!(
        section_id.len() <= u16::MAX as usize && tab_id.len() <= u16::MAX as usize,
        "pty data id exceeds wire length budget"
    );
    let section = section_id.as_bytes();
    let tab = tab_id.as_bytes();
    let mut out = Vec::with_capacity(2 + section.len() + 2 + tab.len() + bytes.len());
    out.extend_from_slice(&(section.len() as u16).to_be_bytes());
    out.extend_from_slice(section);
    out.extend_from_slice(&(tab.len() as u16).to_be_bytes());
    out.extend_from_slice(tab);
    out.extend_from_slice(bytes);
    out
}

/// Inverse of [`encode_pty_data`]. Returns
/// `Some((section_id, tab_id, pty_bytes))` on well-formed input,
/// `None` when the header can't be read (malformed / legacy
/// untagged frame). Callers decode and either route to the tagged
/// tab or drop the frame; logging the drop is the caller's
/// responsibility so per-transport context (peer id, frame count)
/// can be included.
pub fn decode_pty_data(payload: &[u8]) -> Option<(String, String, Vec<u8>)> {
    if payload.len() < 2 {
        return None;
    }
    let sec_len = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    let after_sec_header = 2 + sec_len;
    if payload.len() < after_sec_header + 2 {
        return None;
    }
    let section_id = std::str::from_utf8(&payload[2..after_sec_header])
        .ok()?
        .to_string();
    let tab_len =
        u16::from_be_bytes([payload[after_sec_header], payload[after_sec_header + 1]]) as usize;
    let after_tab_header = after_sec_header + 2 + tab_len;
    if payload.len() < after_tab_header {
        return None;
    }
    let tab_id = std::str::from_utf8(&payload[after_sec_header + 2..after_tab_header])
        .ok()?
        .to_string();
    let bytes = payload[after_tab_header..].to_vec();
    Some((section_id, tab_id, bytes))
}

#[cfg(test)]
mod pty_data_wire_tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let bytes = vec![0x00, 0x01, 0x02, 0xff, b'h', b'i'];
        let encoded = encode_pty_data("proj-a:section-1", "7", &bytes);
        let (sec, tab, payload) = decode_pty_data(&encoded).expect("decode roundtrip");
        assert_eq!(sec, "proj-a:section-1");
        assert_eq!(tab, "7");
        assert_eq!(payload, bytes);
    }

    #[test]
    fn encode_empty_bytes() {
        let encoded = encode_pty_data("s", "t", &[]);
        let (sec, tab, payload) = decode_pty_data(&encoded).expect("decode");
        assert_eq!(sec, "s");
        assert_eq!(tab, "t");
        assert!(payload.is_empty());
    }

    #[test]
    fn decode_rejects_short_header() {
        assert!(decode_pty_data(&[]).is_none());
        assert!(decode_pty_data(&[0x00]).is_none());
        // Claims sec_len=10 but only 2 bytes of payload after
        // the length field — decoder must refuse rather than
        // read past the buffer.
        assert!(decode_pty_data(&[0x00, 0x0a, b'a', b'b']).is_none());
    }

    #[test]
    fn decode_rejects_short_tab_header() {
        // Well-formed section header, but truncates before the
        // tab-length bytes.
        let mut buf = Vec::new();
        buf.extend_from_slice(&(1_u16).to_be_bytes());
        buf.push(b's');
        // No tab-length follows.
        assert!(decode_pty_data(&buf).is_none());
    }

    #[test]
    fn decode_rejects_non_utf8_ids() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(1_u16).to_be_bytes());
        buf.push(0xff); // invalid utf8
        buf.extend_from_slice(&(0_u16).to_be_bytes());
        assert!(decode_pty_data(&buf).is_none());
    }
}

/// Reject any frame larger than this. 4 MiB comfortably fits a
/// realistic [`WorkerReply::ProjectList`] (tens of projects +
/// repos + branches in one JSON payload; an 18-project / 7-repo
/// desktop already serialises to ~220 KiB after the on-wire
/// `RepoSummary` addition in #134) while staying well below QUIC
/// stream-budget thresholds on any reasonable link. PTY chunks and
/// resize JSON stay tiny; the cap's job is bounding what a
/// compromised paired peer can make the daemon allocate per frame,
/// not squeezing the projection. Bumped from 64 KiB in #TODO
/// (mobile couldn't receive its own initial ProjectList because
/// the projection exceeded the old cap).
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// Top-level envelope for every type=1 control frame. Carries a
/// `request_id` so the client can correlate the daemon's reply
/// against the originating call without relying on stream ordering
/// — once `ojm.2..8` land 20+ verbs flying in parallel, ordering
/// alone won't disambiguate.
///
/// Why an envelope rather than a `request_id` field on every
/// `Control` variant:
///   - `Control` already uses `#[serde(tag = "type")]` for its
///     variant discriminator. A separate envelope keeps the
///     correlation field out of the per-variant struct shape, so
///     adding a new domain variant in a sibling task is a one-line
///     change and serde's tag-flatten rules don't have to be
///     re-checked per variant.
///   - The wire cost is one extra `"request_id":N,` JSON pair per
///     frame — negligible against the 1-byte type + 4-byte length
///     header that already precedes the JSON.
///
/// `request_id == 0` is reserved for **push frames** the daemon
/// emits unsolicited (PTY bytes for an attached tab, future
/// project-tree refresh broadcasts, etc.). Clients MUST NOT use 0
/// as a request id when issuing calls — the dispatch table in the
/// Dart layer treats id 0 as "this is not a reply to anyone."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlEnvelope {
    pub request_id: u64,
    #[serde(flatten)]
    pub control: Control,
}

/// Top-level envelope for every type=2 worker-reply frame. Mirrors
/// [`ControlEnvelope`]: `request_id` matches the
/// `ControlEnvelope.request_id` of the call this is replying to,
/// or `0` for daemon-pushed frames that nobody asked for.
///
/// `#[serde(flatten)]` on `reply` keeps the on-wire JSON shape flat
/// — `{"request_id": 17, "kind": "project_list", "projects": [...]}`
/// — so the existing `serde(tag = "kind")` discriminator on
/// `WorkerReply` still works without nesting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerReplyEnvelope {
    pub request_id: u64,
    #[serde(flatten)]
    pub reply: WorkerReply,
}

/// Sentinel `request_id` value reserved for daemon-pushed
/// (unsolicited) frames. Clients filter on this rather than
/// matching against the request_id ↦ Completer table.
#[allow(dead_code)] // used by callers; the smoke-test bin compiles frame.rs in isolation
pub const PUSH_REQUEST_ID: u64 = 0;

/// Client → daemon session-control messages (type=1 frames). Payload
/// is JSON, wrapped in a [`ControlEnvelope`] that carries the
/// `request_id`. Server → client control is not currently used (the
/// daemon pushes data via `0x00` and worker replies via `0x02`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    /// Ask the daemon to send its full project tree as a
    /// [`WorkerReply::ProjectList`] frame (projects → tasks → tabs).
    /// The embedded (desktop) daemon projects straight off the
    /// running `AnotherOneApp`; the standalone sandbox returns a
    /// synthetic tree with one task + one tab.
    ListProjects,
    /// Subscribe to the live PTY byte stream for `(section_id,
    /// tab_id)`. The daemon forwards the stream as a series of
    /// [`TY_DATA`] frames until either the session closes or
    /// another `AttachTab` / `DetachTab` arrives — at most one
    /// attachment per session.
    ///
    /// **Deprecated:** snapshot-aware viewers should use
    /// [`Control::TerminalSubscribe`] instead. The byte-stream
    /// path stays load-bearing for in-process MCP `tab_output`
    /// consumers; once that path is split off (post-Phase 5b),
    /// this verb is removed. See
    /// `docs/designs/01-daemon-canonical-terminal.md`.
    #[deprecated(
        since = "0.2.2",
        note = "use Control::TerminalSubscribe; design 01 / #158. Removed in Phase 5b cutover."
    )]
    AttachTab { section_id: String, tab_id: String },
    /// Stop forwarding PTY bytes for the currently-attached tab.
    /// Idempotent if nothing is attached.
    ///
    /// **Deprecated:** see [`Control::AttachTab`]; pair retired
    /// together via [`Control::TerminalUnsubscribe`].
    #[deprecated(
        since = "0.2.2",
        note = "use Control::TerminalUnsubscribe; design 01 / #158."
    )]
    DetachTab,
    /// Resize the currently-attached tab's PTY. Silently no-ops
    /// when nothing is attached.
    ///
    /// **Deprecated:** snapshot-aware viewers express size hints
    /// through their subscription; resize for the byte-stream
    /// path retires alongside [`Control::AttachTab`].
    #[deprecated(
        since = "0.2.2",
        note = "size hints belong on TerminalSubscribe; design 01 / #158. Removed in Phase 5b cutover."
    )]
    TabResize { cols: u16, rows: u16 },
    /// Refresh the daemon's liveness tracking for this viewer.
    /// Clients send one periodically (every few seconds) while
    /// they have an attached tab. When the daemon stops seeing
    /// heartbeats from a viewer, a sweep removes the viewer from
    /// `active_viewers` + `viewer_focus` and recomputes the PTY's
    /// effective viewport — so a backgrounded / killed phone
    /// stops holding the desktop's PTY at a small phone viewport
    /// after connection drop. No-op when nothing is attached (the
    /// viewer has no viewport claim to refresh); see also
    /// [`AttachTab`], [`DetachTab`] for the lifecycle pair.
    Heartbeat,
    /// Ask the daemon to launch the task's tab as a live PTY if it
    /// isn't running. If already running, no-op. After this call,
    /// [`AttachTab`] will succeed. Both the desktop GUI and mobile
    /// are equal citizens in launching — neither is a "master" that
    /// gates the other.
    LaunchTab { section_id: String, tab_id: String },
    /// Add an on-disk project directory to the daemon's project
    /// store. Heavy `prepare_project` work runs on a background
    /// thread on the daemon side so the iroh writer task isn't
    /// blocked. Successful inserts reply with
    /// [`WorkerReply::CreateProjectAck`] carrying the post-mutation
    /// project snapshot — the issuing client updates its tree from
    /// the reply directly without a follow-up `ListProjects` round
    /// trip (mutator inline-snapshot contract). A path that the
    /// store already knows replies with [`WorkerReply::Err`] of
    /// kind [`ErrKind::Internal`] (this is the rare "user added the
    /// same dir twice" case; not worth a dedicated `err_kind`).
    CreateProject { path: String },
    /// Remove a project from the daemon's store by id. Cascades to
    /// the project's tasks + terminal sections via
    /// [`another_one_core::project_store::ProjectStore::remove_project`].
    /// Idempotent — passing an unknown id is a silent no-op on the
    /// store side, but the daemon still replies with
    /// [`WorkerReply::DeleteProjectAck`] echoing the id so the issuer
    /// can drop any stale UI rows.
    DeleteProject { project_id: String },
    /// List the merged repo-scoped + global custom actions for `project_id`,
    /// in the same order the desktop's titlebar split-button dropdown
    /// renders. Empty list when the project is unknown — matches
    /// `ProjectStore::project_actions` behaviour. Reply:
    /// [`WorkerReply::ProjectActionsAck`].
    ListProjectActions { project_id: String },
    /// Snapshot of agents the user has enabled on this host plus
    /// the id of the one they've picked as default. Drives the
    /// new-task modal's agent multi-select. Reply:
    /// [`WorkerReply::EnabledAgentsAck`].
    ReadEnabledAgents,
    /// Submit the new-task modal over the shared wire. The daemon
    /// decides whether this becomes a direct task or a worktree task
    /// based on `worktree_mode`, resolves the initial launch config
    /// from `agent_ids`, persists the task + initial section/tab, and
    /// queues the first PTY launch. Reply:
    /// [`WorkerReply::CreateTaskAck`].
    CreateTask {
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_ids: Vec<String>,
        branch_mode_existing: bool,
        worktree_mode: bool,
    },
    /// Append one agent tab (or plain shell when `agent_id` is
    /// empty) to an existing task section, make it active, and queue
    /// its PTY launch. Reply: [`WorkerReply::CreateTabAck`].
    CreateTab {
        section_id: String,
        agent_id: String,
    },
    /// Persist the active tab for a section. Does not itself launch
    /// or attach — the client's existing selection/attach path owns
    /// that. Reply: [`WorkerReply::SetActiveTabAck`].
    SetActiveTab { section_id: String, tab_id: String },
    /// Remove one tab from a section and tear down its live PTY if
    /// present. Reply: [`WorkerReply::DeleteTabAck`].
    DeleteTab { section_id: String, tab_id: String },
    /// Flip one section tab's `pinned` flag and return its new value.
    /// Set the pinned state of a section tab to a specific value.
    /// Reply: [`WorkerReply::SetTabPinnedAck`].
    SetTabPinned {
        section_id: String,
        tab_id: String,
        pinned: bool,
    },
    /// Full agent registry — every agent in `core::agents::AGENTS`
    /// paired with its per-host enabled flag, default flag, and
    /// launch-args list. Drives the Settings → Agents page. Reply:
    /// [`WorkerReply::AgentSettingsAck`].
    ReadAgentSettings,
    /// Toggle one agent's enabled flag in the daemon host config.
    /// Reply: [`WorkerReply::SetAgentEnabledAck`].
    SetAgentEnabled { agent_id: String, enabled: bool },
    /// Mark an enabled agent as the daemon host's default. Reply:
    /// [`WorkerReply::SetDefaultAgentAck`].
    SetDefaultAgent { agent_id: String },
    /// Replace one agent's launch-args list. Empty `args` clears
    /// the override. Reply: [`WorkerReply::SetAgentLaunchArgsAck`].
    SetAgentLaunchArgs { agent_id: String, args: Vec<String> },
    /// Snapshot of every detected Open-In app on the daemon host plus
    /// its enabled flag. Drives Settings → Open In. Reply:
    /// [`WorkerReply::OpenInSettingsAck`].
    ReadOpenInSettings,
    /// Toggle one Open-In app's enabled flag on the daemon host.
    /// Reply: [`WorkerReply::SetOpenInAppEnabledAck`].
    SetOpenInAppEnabled { app_id: String, enabled: bool },
    /// Launch `project_id` in `app_id` on the daemon host and persist
    /// the chosen app as the preferred default when the spawn
    /// succeeds. Reply: [`WorkerReply::OpenProjectInAppAck`].
    OpenProjectInApp { project_id: String, app_id: String },
    /// Run one custom action inside `section_id`'s task: appends a
    /// fresh `PersistedTerminalTab`, queues a launch, and (for
    /// shell actions) records the command bytes the daemon writes
    /// once the PTY is up.
    ///
    /// **Single-shot Ack**: the
    /// reply carries only the new `tab_id` so the caller can
    /// `AttachTab` and watch the action's PTY output flow over the
    /// existing data-frame pipeline. There's no streaming
    /// per-step progress channel here; if a future iteration ever
    /// needs one, it lands as a separate `Control::Subscribe` verb
    /// per the foundation task's push-channel hatch (`request_id == 0`).
    /// This matches the existing desktop action runner shape 1:1:
    /// the GPUI desktop has lived with single-shot for the life of
    /// the feature without a streaming requirement surfacing. Reply:
    /// [`WorkerReply::RunProjectActionAck`].
    RunProjectAction {
        project_id: String,
        section_id: String,
        action_id: String,
    },
    /// Upsert one custom action for `project_id`, optionally saving a
    /// global copy instead of a repo-scoped one. Reply:
    /// [`WorkerReply::SetProjectActionAck`].
    SetProjectAction {
        project_id: String,
        action: ProjectActionWire,
        save_global_copy: bool,
    },
    /// Delete one custom action by id from the repo-scoped or global
    /// registry. Reply: [`WorkerReply::DeleteProjectActionAck`].
    DeleteProjectAction {
        project_id: String,
        action_id: String,
    },
    /// TOFU (trust-on-first-use) pairing handshake. Sent as the very
    /// first control frame by an unknown peer whose `NodeId` is NOT
    /// in the daemon's `paired_peers` allowlist. If the daemon's
    /// current pair nonce (regenerated at boot + on allowlist reset)
    /// matches `pair_token`, the peer's `NodeId` is appended to the
    /// allowlist, the nonce is consumed (cleared), and the session
    /// proceeds. Any mismatch closes the connection with
    /// `anotherone/unpaired`. Already-paired peers skip this frame
    /// entirely; sending it is a no-op for them.
    ///
    /// `pair_token` is the hex-encoded 128-bit nonce from the
    /// `pair=<hex>` query parameter on the pairing URL. A `None`
    /// (or missing) token from an unpaired peer is an
    /// unrecoverable rejection — we never auto-pair without proof
    /// the user scanned the current QR.
    ///
    /// `protocol_version` is the wire version the client speaks
    /// (see [`super::transport_iroh::PROTOCOL_VERSION`]). The daemon
    /// rejects mismatches with the
    /// `anotherone/incompatible-version` close reason instead of
    /// letting serde explode on the first unknown variant. Older
    /// (v0) daemons / clients on the previous ALPN won't reach this
    /// frame because iroh refuses the ALPN handshake before any
    /// stream opens — the in-band field is the belt-and-braces guard
    /// for any future transport (e.g. an iroh proxy that strips
    /// ALPN).
    ///
    /// `#[serde(default)]` lets a daemon decoding a Hello from an
    /// older client treat the missing field as `0` and surface the
    /// version mismatch cleanly rather than failing the decode
    /// itself.
    Hello {
        pair_token: Option<String>,
        #[serde(default)]
        protocol_version: u32,
    },
    /// Create a worktree task on `project_id`. Spawns a fresh git
    /// worktree from `source_branch` (the new branch is named after
    /// the slugified `task_name`), prepares the project, and inserts
    /// both the worktree project and the task into the daemon's
    /// store. Reply is [`WorkerReply::CreateWorktreeTaskAck`] carrying the
    /// inline post-mutation [`TaskSummary`] so the issuer can update
    /// its tree without a `ListProjects` follow-up.
    ///
    /// `agent_provider == None` launches a plain shell on the new
    /// task's first tab; any concrete provider selects the
    /// corresponding agent CLI.
    ///
    /// Heavy filesystem work (git worktree + prepare_project) runs
    /// on a worker thread inside the daemon — clients can expect
    /// tens of seconds before the reply arrives.
    CreateWorktreeTask {
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_provider: Option<AgentProvider>,
    },
    /// Rename a task. Empty / whitespace-only names are rejected
    /// daemon-side. Reply is [`WorkerReply::SetTaskNameAck`] with the
    /// post-rename inline `TaskSummary` and a `changed` flag — an
    /// unknown id or no-op rename returns `changed = false`.
    SetTaskName { task_id: String, new_name: String },
    /// Pin or unpin a task. Pinned tasks float to the top of their
    /// project's task list. Reply is [`WorkerReply::SetTaskPinnedAck`]
    /// with the inline `TaskSummary` and a `changed` flag (idempotent
    /// re-set returns `false`).
    SetTaskPinned { task_id: String, pinned: bool },
    /// Remove a task (and its terminal sections) from the daemon's
    /// store. The on-disk worktree branch is left untouched — same
    /// semantics as the desktop side. Reply is
    /// [`WorkerReply::DeleteTaskAck`] with `removed = true` if a task
    /// was deleted, `false` for an unknown id (idempotent).
    DeleteTask { project_id: String, task_id: String },
    /// Persist the desktop's per-section terminal-tab snapshot to
    /// the daemon. Routes to either `update_task_tabs` (when the
    /// section belongs to a task — `task_id` set on the wire
    /// `section_id`) or `set_terminal_section` (project pages /
    /// standalone shells). The desktop GUI fires this on every
    /// terminal-state change (tab pinned, title updated, restore
    /// status changed, etc.) so connected mobile sessions see the
    /// new tab metadata via the broadcast push.
    ///
    /// `persisted` is an opaque JSON-serialised
    /// `core::project_store::PersistedSectionState` — daemon-proto
    /// stays free of the section-state structural shape; both
    /// clients deserialize via `serde_json::from_value` into the
    /// canonical core type.
    ///
    /// Reply is [`WorkerReply::SetSectionStateAck`].
    SetSectionState {
        section_id: String,
        persisted: serde_json::Value,
    },
    /// Update the user's last-active-section pointer so a
    /// reconnect or restart restores focus to the same section
    /// across both clients. `None` clears the pointer (no section
    /// active).
    ///
    /// Reply is [`WorkerReply::SetLastActiveSectionAck`].
    SetLastActiveSection { section_id: Option<String> },
    /// Toggle whether the projects sidebar shows per-task git
    /// metadata (lines added/removed, last-commit relative). Mirrors
    /// `core::project_store::UiState::show_sidebar_git_metadata`.
    /// Reply is [`WorkerReply::SetSidebarGitMetadataVisibleAck`].
    SetSidebarGitMetadataVisible { visible: bool },
    /// Switch the app-wide theme preference. `mode_id` is the
    /// lowercase variant name from `core::project_store::ThemeMode`
    /// (`"light"` / `"dark"` / `"system"`). Daemon-proto stays
    /// free of the enum shape by passing it as a string; the
    /// daemon decodes via `serde_json::from_value`. Reply is
    /// [`WorkerReply::SetThemeModeAck`]. Routed through the daemon (not
    /// a direct client-side `ProjectStore::save`) so every paired
    /// client sees the new theme via the next projection instead
    /// of each client persisting a divergent local copy.
    SetThemeMode { mode_id: String },
    /// Update the per-repo "default commit action" pin (commit vs.
    /// commit-and-push) — drives which button is the primary on the
    /// titlebar's Commit dropdown. Mirrors
    /// `core::project_store::UiState::repo_default_commit_actions`.
    /// `action` is the string id of the variant (`"commit"`,
    /// `"commit-and-push"`). Reply is [`WorkerReply::SetRepoDefaultCommitActionAck`].
    SetRepoDefaultCommitAction { repo_id: String, action: String },
    /// Persist a new branch override on a worktree task — the
    /// desktop renames the on-disk branch via git, then calls this
    /// to update the task's record. Reply is [`WorkerReply::SetTaskBranchAck`].
    SetTaskBranch {
        task_id: String,
        target_project_id: String,
        branch_name: String,
    },
    /// Persist the user's expanded-project / collapsed-project
    /// state. `expanded_repo_ids` is the full set; the daemon
    /// replaces its store wholesale. Reply is [`WorkerReply::SetExpandedReposAck`].
    SetExpandedRepos { expanded_repo_ids: Vec<String> },
    /// Update the LLM settings for the AI commit-message generator.
    /// `settings` is an opaque JSON-serialised
    /// `core::project_store::GitActionLlmSettings`. Reply is
    /// [`WorkerReply::SetGitCommitLlmAck`].
    SetGitCommitLlm { settings: serde_json::Value },
    /// Same as `SetGitCommitLlm` but for the PR-generation LLM.
    /// Reply is [`WorkerReply::SetGitPrLlmAck`].
    SetGitPrLlm { settings: serde_json::Value },
    /// Branch names available on `project_id`'s git repo. Powers the
    /// new-task modal's source-branch dropdown. Reply is
    /// [`WorkerReply::ProjectBranchesAck`] with an empty list when
    /// the project id is unknown.
    ReadProjectBranches { project_id: String },
    /// Default branch the new-task modal seeds for `project_id`.
    /// Reply is [`WorkerReply::ReadPrimaryBranchAck`] with `None` when
    /// the project has no current branch (fresh repo).
    ReadPrimaryBranch { project_id: String },
    /// User's preferred default commit action (`"commit"` or
    /// `"commit-and-push"`) for the active project's root repo.
    /// Reply is [`WorkerReply::ReadRepoDefaultCommitActionAck`] with
    /// `None` when no preference has been recorded — UI defaults to
    /// `"commit"` in that case.
    ReadRepoDefaultCommitAction { project_id: String },
    /// Snapshot the active project's branch metadata: current branch
    /// name + ahead / behind counts. Powers the titlebar git-actions
    /// split-button's primary-action selection (Push when ahead, Pull
    /// when behind, Fetch otherwise — Commit comes from the
    /// changes-vs-clean side via `ReadChangedFiles`).
    ///
    /// Reads through `core::project_store::read_project_git_state` with
    /// `include_metadata=true` on the daemon's project root path.
    /// Reply is [`WorkerReply::ActiveGitStateAck`] with a `None`
    /// payload when the project id is unknown.
    ReadActiveGitState { project_id: String },
    /// Working-tree changes for `project_id`. Powers the right
    /// sidebar's Changes pane. Reply is
    /// [`WorkerReply::ChangedFilesAck`] with a `None` payload when
    /// the project id is unknown.
    ReadChangedFiles { project_id: String },
    /// Resolve `project_id`'s GitHub remote URL via
    /// [`another_one_core::git_actions::find_github_repo_url`]. Reply
    /// is [`WorkerReply::ProjectGithubUrlAck`] with `None` when the
    /// project id is unknown, has no `origin`, or `origin` isn't
    /// github.com.
    ReadProjectGithubUrl { project_id: String },
    /// Recent commits on `project_id`'s current branch, capped at
    /// `limit` entries. Powers the right sidebar's Commits pane.
    /// Reply is [`WorkerReply::RecentCommitsAck`] with `None` when
    /// the project id is unknown.
    ReadRecentCommits { project_id: String, limit: u32 },
    /// Per-commit file-change list for the right sidebar's expandable
    /// Commits rows. Reply is [`WorkerReply::CommitFileChangesAck`]
    /// with `None` for an unknown project. Errors propagate as
    /// [`WorkerReply::Err`] (commit pruned, etc.).
    ReadCommitFileChanges {
        project_id: String,
        commit_id: String,
    },
    /// Snapshot the resolved branch settings for `project_id`'s
    /// root project — configured + effective values for default and
    /// default-target branches plus the available branch list.
    /// Reply is [`WorkerReply::BranchSettingsAck`] with `None` when
    /// the project is unknown or has no repo metadata.
    ReadBranchSettings { project_id: String },
    /// Update the configured default branch or default-target branch
    /// for `project_id`'s root project. `field` must be one of
    /// `"default-branch"` / `"default-target-branch"`. `branch_name`
    /// of `None` clears the override. Reply is
    /// [`WorkerReply::SetBranchSettingAck`] with `changed=true` when
    /// the persisted store actually changed; bad fields or
    /// unavailable branches surface as [`WorkerReply::Err`].
    ///
    /// Bundled with the read verbs (rather than ojm.5's git
    /// mutations) because it operates on the same git-config-shaped
    /// state. Review economy.
    SetBranchSetting {
        project_id: String,
        field: String,
        branch_name: Option<String>,
    },
    /// `another-one-ojm.5` — stage one changed file via `git add -A`.
    /// `original_path` is set only on rename/copy entries — git needs
    /// both source and destination to resolve the rename pair. Reply
    /// is [`WorkerReply::StageChangedFileAck`] carrying the post-
    /// mutation `changed_files` snapshot so the issuing client can
    /// refresh the right-sidebar Changes pane in the same round-trip
    /// (per the inline-snapshot contract above).
    StageChangedFile {
        project_id: String,
        path: String,
        original_path: Option<String>,
    },
    /// `another-one-ojm.5` — unstage one changed file via
    /// `git restore --staged` (with `git reset HEAD` fallback for
    /// pre-2.23 git, mirroring `core::unstage_changed_file`). Same
    /// rename-pair contract as [`Self::StageChangedFile`]. Reply is
    /// [`WorkerReply::UnstageChangedFileAck`].
    UnstageChangedFile {
        project_id: String,
        path: String,
        original_path: Option<String>,
    },
    /// `another-one-ojm.5` — `git add -A` on the project root: stage
    /// every change in one shot. Reply is
    /// [`WorkerReply::StageAllChangesAck`].
    StageAllChanges { project_id: String },
    /// `another-one-ojm.5` — unstage every staged change in one shot
    /// (`git restore --staged -- .` with `git reset HEAD -- .`
    /// fallback). Reply is [`WorkerReply::UnstageAllChangesAck`].
    UnstageAllChanges { project_id: String },
    /// `another-one-ojm.5` — discard one file's working-tree changes.
    /// Untracked files are deleted from disk; tracked files are
    /// restored from HEAD via `git restore` (with checkout fallback
    /// for older git, mirroring `core::revert_changed_file`). The
    /// `untracked` flag is passed through verbatim so the daemon
    /// picks the right code path. Destructive — UI gates this
    /// behind a confirm. Reply is
    /// [`WorkerReply::DiscardChangedFileAck`].
    DiscardChangedFile {
        project_id: String,
        path: String,
        untracked: bool,
        original_path: Option<String>,
    },
    /// `another-one-ojm.5` — discard a whole snapshot of changed
    /// files in one round-trip. The caller provides the current
    /// `changed_files` list so the daemon can avoid re-reading git
    /// state between per-path reverts. Reply is
    /// [`WorkerReply::DiscardAllChangesAck`] carrying the final
    /// post-mutation snapshot plus any per-path failures.
    DiscardAllChanges {
        project_id: String,
        files: Vec<ChangedFileWire>,
    },
    /// `another-one-ojm.5` — run a titlebar git action against
    /// `project_id`. `action_id` is the verbatim string the
    /// titlebar split-button emits: `"commit"`, `"commit-and-push"`,
    /// `"undo-last-commit"`, `"fetch"`, `"pull"`, `"push"`,
    /// `"force-push"`, `"create-pr"`, `"create-draft-pr"`. Reply is
    /// [`WorkerReply::RunToolbarGitActionAck`] carrying the
    /// `outcome` so the UI can surface the toast + decide whether to
    /// invalidate the changed-files / git-state providers.
    RunToolbarGitAction {
        project_id: String,
        action_id: String,
    },
    /// `another-one-ojm.5` — create a branch from HEAD on
    /// `project_id`. `use_current_task = true` swaps the current
    /// checkout in place; `false` cuts a fresh worktree (with
    /// `migrate_changes` controlling whether uncommitted changes
    /// move to it). Reply is [`WorkerReply::CreateBranchAck`]
    /// carrying the new task's `section_id` for the worktree case
    /// (empty string in current-task mode — the caller's UI just
    /// dismisses the modal).
    CreateBranch {
        project_id: String,
        branch_name: String,
        use_current_task: bool,
        migrate_changes: bool,
    },
    /// `another-one-ojm.5` — spawn a review task targeting a PR.
    /// Clones the PR's `head_branch` into a worktree, prepares the
    /// project, inserts the task, optionally launches the configured
    /// agent CLI for the task. Reply is
    /// [`WorkerReply::CreateReviewTaskAck`] carrying the new task's
    /// `section_id` so the issuing client navigates to it.
    CreateReviewTask {
        project_id: String,
        pull_request_number: u64,
        head_branch: String,
        agent_provider: Option<AgentProvider>,
    },
    /// Resolve the latest pull-request status for `project_id`'s
    /// current branch — drives the titlebar's "Create PR" / "Open PR"
    /// pill enabledness on every connected client. Reply variant
    /// is [`WorkerReply::ReadPullRequestStatusAck`]; `status: None`
    /// covers both "no PR for the branch" and "unknown project".
    /// Hard failures (gh CLI missing, network error) come back as
    /// [`WorkerReply::Err`] instead.
    ReadPullRequestStatus { project_id: String },
    /// Read the CI checks attached to `project_id`'s current PR —
    /// drives the right-sidebar Checks pane. Reply variant is
    /// [`WorkerReply::PullRequestChecksAck`] with a three-state
    /// payload: `Some(list)` = PR exists (list may be empty),
    /// `None` = no PR or unknown project. gh CLI / network failures
    /// come back as [`WorkerReply::Err`] so the UI can surface a
    /// toast rather than rendering a silent empty state.
    ReadPullRequestChecks { project_id: String },
    /// Fetch open pull requests for `project_id` filtered by
    /// `filter_index` (0=all, 1=needs my review, 2=author:@me,
    /// 3=draft) plus an optional free-text `query` (GitHub search
    /// syntax). Powers the project page's Open PRs section. Reply
    /// variant is [`WorkerReply::ListProjectPullRequestsAck`]; `prs:
    /// None` covers the unknown-project case. gh CLI / auth /
    /// network failures arrive as [`WorkerReply::Err`].
    ListProjectPullRequests {
        project_id: String,
        filter_index: u32,
        query: String,
    },
    /// Settings → Git Actions: snapshot the commit + PR LLM scripts
    /// (resolved-current text plus a `using_default` flag per script).
    /// Reply: [`WorkerReply::GitActionScriptsAck`].
    ReadGitActionScripts,
    /// Settings → Git Actions: replace the commit-message generation
    /// script. Empty / matching the default reverts to the built-in
    /// template. Reply: [`WorkerReply::SetGitCommitScriptAck`] with
    /// the post-mutation `changed` flag (per the inline-snapshot
    /// contract).
    SetGitCommitScript { script: String },
    /// Settings → Git Actions: drop the commit-script override, revert
    /// to the built-in default. Reply:
    /// [`WorkerReply::ResetGitCommitScriptAck`].
    ResetGitCommitScript,
    /// Settings → Git Actions: replace the PR title/body generation
    /// script. Reply: [`WorkerReply::SetGitPrScriptAck`].
    SetGitPrScript { script: String },
    /// Settings → Git Actions: drop the PR-script override. Reply:
    /// [`WorkerReply::ResetGitPrScriptAck`].
    ResetGitPrScript,
    /// Settings → Keybindings: snapshot every shortcut action paired
    /// with its current + default binding. Reply:
    /// [`WorkerReply::ShortcutSettingsAck`].
    ReadShortcutSettings,
    /// Settings → Keybindings: set / clear one shortcut binding.
    /// Empty `binding` clears the action (it becomes inert).
    /// `action_id` is the kebab-case id (`new-task`, `cycle-projects`,
    /// etc.); the daemon returns
    /// [`WorkerReply::Err`] with [`ErrKind::UnknownId`] when it
    /// doesn't recognise the id. Reply on success:
    /// [`WorkerReply::SetShortcutBindingAck`].
    SetShortcutBinding { action_id: String, binding: String },
    /// Settings → Keybindings: reset one shortcut to its built-in
    /// default. Reply: [`WorkerReply::ResetShortcutBindingAck`].
    ResetShortcutBinding { action_id: String },
    /// Settings → MCP: snapshot the catalog + on-disk registry. Reply:
    /// [`WorkerReply::McpSettingsAck`].
    ReadMcpSettings,
    /// Settings → MCP: add one catalog entry to the registry. No-op
    /// when `catalog_id` isn't a known catalog id or the entry's
    /// already in the registry. Reply:
    /// [`WorkerReply::McpAddFromCatalogAck`].
    McpAddFromCatalog { catalog_id: String },
    /// Settings → MCP: toggle one entry's enabled flag for one
    /// provider. `provider_id` is kebab-case (`claude-code`,
    /// `cursor-agent`, etc.) — unknown ids surface as
    /// [`WorkerReply::Err`] with [`ErrKind::UnknownId`]. Runs
    /// `sync_all` on success so the harness's native config picks
    /// up the change. Reply on success:
    /// [`WorkerReply::McpToggleAck`].
    McpToggle {
        entry_id: String,
        provider_id: String,
        enabled: bool,
    },
    /// Settings → MCP: remove one entry from the registry. Runs
    /// `sync_all` on success. Reply: [`WorkerReply::McpRemoveAck`].
    McpRemove { entry_id: String },
    /// Re-run the daemon-side `gh auth status` probe. Fire-and-forget
    /// from the client's PoV — the new status is published through
    /// the next `UiSnapshot.gh_auth_status` projection rather than
    /// inline on the reply, so the same code path that delivers the
    /// boot-time result also delivers Recheck results. Reply:
    /// [`WorkerReply::RecheckGhAuthAck`]. See #156.
    RecheckGhAuth,
    // ── Daemon-canonical Term (design 01) ─────────────────────
    //
    // Phase 1 declares these verbs; the daemon dispatch arms reply
    // with [`WorkerReply::Err`] [`ErrKind::Internal`] until the
    // Term-task implementation lands
    // (`docs/designs/01-daemon-canonical-terminal.md` Phase 2+).
    /// Subscribe this viewer to grid frames for `(section_id,
    /// tab_id)`. The daemon emits a `TerminalFrame::Full` immediately
    /// (or on Term-task ready) and then steady-state frames paced at
    /// or below `max_fps` per viewer. `since_seq = None` means "first
    /// subscription"; passing a stale seq forces a fresh `Full` if
    /// the daemon no longer holds the diff history. Replaces the
    /// `AttachTab` byte-stream attachment for snapshot-aware
    /// viewers; the byte path stays available in-process for MCP.
    /// Reply: [`WorkerReply::TerminalSubscribeAck`].
    TerminalSubscribe {
        section_id: String,
        tab_id: String,
        max_fps: u8,
        #[serde(default)]
        since_seq: Option<u64>,
    },
    /// Stop pacing frames for `(section_id, tab_id)` to this viewer.
    /// Idempotent. Reply: [`WorkerReply::TerminalUnsubscribeAck`].
    TerminalUnsubscribe { section_id: String, tab_id: String },
    /// Fetch a slice of the daemon's scrollback history for
    /// `(section_id, tab_id)`. Snapshots ship only a small backbuffer
    /// (2× viewport); the rest is on-demand to keep frame size
    /// bounded. Reply: [`WorkerReply::TerminalScrollback`].
    TerminalReadScrollback {
        section_id: String,
        tab_id: String,
        range: ScrollbackRange,
    },
    /// Run a literal-or-regex match across the full grid (viewport +
    /// scrollback) of `(section_id, tab_id)`. Replaces the
    /// client-side grid walk in today's `terminal_runtime.rs`. Reply:
    /// [`WorkerReply::TerminalSearch`].
    TerminalSearch {
        section_id: String,
        tab_id: String,
        request: TerminalSearchRequest,
    },
    /// Forward keystrokes / paste payloads / mouse-protocol bytes to
    /// the PTY backing `(section_id, tab_id)`. Replaces
    /// `Session::push_data` for snapshot-mode viewers. Reply:
    /// [`WorkerReply::TerminalInputAck`].
    TerminalInput {
        section_id: String,
        tab_id: String,
        bytes: Vec<u8>,
    },
}

// ── Push vs pull contract for state mutations ────────────────────
//
// The shipped reality is a **hybrid** — both inline replies AND a
// state-change push pump are active. This block describes what
// each channel is for; see also #122 (push-pump introduction),
// #134 (debounce + RepoSummary added), #137 (this reconciliation).
//
// ## Channel 1: inline-snapshot mutator replies (original ojm.1 design)
//
//   Domain mutator verbs return a `WorkerReply::*` variant whose
//   payload contains the changed entity (`CreateProjectAck { project:
//   ProjectSummary }`, `SetTaskNameAck { task: TaskSummary }`, etc.).
//   The issuing client can splice the result into its tree from
//   the reply directly without a follow-up `ListProjects`
//   round-trip.
//
//   Status today: **dead code in practice.** Every GUI mutator
//   path (`app/src/app.rs::dispatch_*`) uses
//   `dispatch_fire_and_forget`, which logs errors but does not
//   consume the reply's inline snapshot. All clients converge
//   their UI via Channel 2 instead.
//
//   Why keep the variants: they're cheap bytes on the wire, the
//   infrastructure is the foundation for the per-domain session
//   migrations (jbk / qa3 / cwn / fzw / kek sub-issues), and
//   opting in to them later is purely additive. If a mutator
//   variant ever becomes authoritative for a client, it must also
//   carry `repo: Option<RepoSummary>` (see #134) — otherwise that
//   client silently drops branch catalog data on the mutation.
//
// ## Channel 2: state-change push pump (added #122, debounced #134)
//
//   `daemon::dispatch::serve_session_with_attach` spawns a task
//   that subscribes to `DaemonRegistry::subscribe_state_changes()`
//   and pushes a fresh `WorkerReply::ProjectList { projects, repos,
//   ui }` with `request_id == 0` to the peer after any state-change
//   burst. A 50 ms quiet-window debouncer collapses bursts so the
//   peer sees at most ~20 Hz even when the registry signals dozens
//   of mutations per second under load.
//
//   This is what **every client relies on today** — including
//   desktop-paired-to-itself through the in-memory transport, and
//   mobile over iroh. Both absorb via `ProjectStore::absorb_projection`,
//   which rehydrates the full tree + repo catalog from the wire
//   (per #134).
//
//   Cost: one projection per state-change burst per connected
//   session. For the typical 2-peer case (desktop + phone) that's
//   fine; scaling to many peers would want scoped pushes ("repo X
//   changed") driven off a typed payload on the state-change
//   broadcast. Filed follow-ups for that sit beyond #137.
//
// ## Rule of thumb for new mutator verbs
//
//   - Return an inline-snapshot `WorkerReply::*` variant. Carry
//     enough context that a future consumer can splice without
//     another round-trip — include `repo: Option<RepoSummary>`
//     when the mutation touches the repo catalog. Channel 1 bytes
//     are effectively free and the variant is there if we ever
//     decide to drop Channel 2 or want Channel 1 to be
//     authoritative on a bandwidth-sensitive path.
//   - Call `DaemonRegistry::notify_state_changed` so Channel 2
//     fires too. That's what clients consume today.
//   - Reader verbs return the projection the caller asked for and
//     do not touch Channel 2.
//
//   If a future feature needs *scoped* cross-client live updates
//   (e.g. "phone follows desktop's commit panel in real time"), it
//   lands as an opt-in `Control::Subscribe { topic }` verb that
//   pushes targeted `WorkerReply::*` frames with `request_id == 0`
//   instead of piggybacking on the ProjectList pump.

/// Worker replies (type=2 frames). Payload is JSON. Daemon → client
/// only.
///
/// Each variant is a lossy projection of one `core::*_service`
/// worker's reply type. We deliberately do *not* derive Serialize on
/// the core reply types themselves — those structs are shaped for the
/// desktop's GPUI state, with nested `Result<_, String>` and internal
/// metadata the mobile UI doesn't need. This wire type is the curated
/// subset we commit to as a public protocol.
///
/// Wire-compat rules:
/// - `#[serde(tag = "kind")]` — every message carries its discriminator,
///   so new variants can be added without renumbering.
/// - New variants: clients built before the variant existed hit
///   serde's "unknown variant" error. To stay forwards-compatible,
///   clients SHOULD decode into a shape that tolerates unknown
///   variants (e.g., decode to `serde_json::Value` first, then try
///   `WorkerReply`). The current Flutter client just logs-and-ignores
///   unknown frame *types* (via the `0x02` discriminator itself), so
///   until it upgrades to variant-awareness, the daemon should only
///   emit variants the contemporaneous client supports. Track client
///   capability out of band (ALPN version bump or a hello frame) when
///   we move beyond this slice.
/// - Mutators carry an inline state snapshot — see the "Push vs
///   pull" comment block immediately above.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerReply {
    /// Per-verb acks for fire-and-forget verbs whose effects are
    /// observable elsewhere (e.g. `AttachTab` — bytes flow on the
    /// events stream; `LaunchTab` — runtime appears in the
    /// registry; `DetachTab`/`TabResize` — state updates the next
    /// pull observes). Each is a unit variant; clients await
    /// `Session::call` to avoid leaking a pending_calls entry.
    #[deprecated(
        since = "0.2.2",
        note = "pair of Control::AttachTab; use TerminalSubscribeAck. Design 01 / #158."
    )]
    AttachTabAck,
    #[deprecated(
        since = "0.2.2",
        note = "pair of Control::DetachTab; use TerminalUnsubscribeAck. Design 01 / #158."
    )]
    DetachTabAck,
    #[deprecated(
        since = "0.2.2",
        note = "pair of Control::TabResize; subscriptions carry size hints. Design 01 / #158."
    )]
    TabResizeAck,
    HeartbeatAck,
    LaunchTabAck,
    SetSectionStateAck,
    SetLastActiveSectionAck,
    SetSidebarGitMetadataVisibleAck,
    SetThemeModeAck,
    SetRepoDefaultCommitActionAck,
    SetTaskBranchAck,
    SetExpandedReposAck,
    SetGitCommitLlmAck,
    SetGitPrLlmAck,
    /// Reply to [`Control::RecheckGhAuth`]. Empty Ack — the actual
    /// status flows through the next `UiSnapshot.gh_auth_status`
    /// projection.
    RecheckGhAuthAck,
    /// Response to [`Control::ListProjects`]. Order matches the
    /// desktop sidebar's `project_order`; worktrees of a root are
    /// emitted as their own entries rather than nested children
    /// (the mobile UI can still group them by `repo_id` if it
    /// wants a tree rendering later).
    ProjectList {
        projects: Vec<ProjectSummary>,
        /// Per-repo metadata (branches, common dir) keyed by
        /// `RepoSummary::id`. Wire-additive: older daemons omit the
        /// field entirely and clients fall back to the empty-repo
        /// behaviour they had before. Without this, `absorb_projection`
        /// on the client side silently wiped locally-resolved branch
        /// data on every daemon push — see the sidebar-flicker
        /// regression captured around #125.
        #[serde(default)]
        repos: Vec<RepoSummary>,
        /// Per-user UI state — wire-additive on existing daemons
        /// (defaults to `UiSnapshot::default()` when missing).
        #[serde(default)]
        ui: UiSnapshot,
    },
    /// Response to [`Control::CreateProject`] on success. Carries the
    /// inline snapshot of the freshly-inserted project so the
    /// issuing client can splice it into its tree without a
    /// follow-up `ListProjects` (see the "Push vs pull" block above
    /// for the contract). On a duplicate path or a `prepare_project`
    /// failure the daemon emits [`WorkerReply::Err`] instead.
    CreateProjectAck { project: ProjectSummary },
    /// Response to [`Control::DeleteProject`]. Echoes the id so the
    /// issuer can drop the matching tree row even if its local
    /// cache had already been pruned. Idempotent on the daemon
    /// side: an unknown id still produces this reply rather than an
    /// `Err`.
    DeleteProjectAck { project_id: String },
    /// **Daemon-pushed** (request_id == 0) notification that a live
    /// PTY attachment just died on the server side — the daemon's
    /// forwarder task broke out of its broadcast recv loop because
    /// the per-tab channel lagged past its capacity and the client
    /// missed chunks. Clients must re-issue [`Control::AttachTab`]
    /// to get a clean replay + fresh VT state; no other recovery
    /// is possible since the in-band byte stream is desynced.
    ///
    /// Scope: only emitted on `broadcast::RecvError::Lagged`. The
    /// `RecvError::Closed` case (PTY runtime dropped) is
    /// deliberately *not* signalled here because reattaching would
    /// fail with `unknown tab` — the client finds out about the
    /// exit via the existing `TabClosed` push instead.
    ///
    /// `reason` is a short human-readable string for logs/toast;
    /// clients should not parse it. See #53.
    ///
    /// **Deprecated:** AttachDropped fires on the byte-stream path
    /// only. Snapshot-aware viewers recover from a `seq` gap by
    /// requesting a fresh `Full`; nothing equivalent fires for
    /// snapshot subscribers. Retired alongside
    /// [`Control::AttachTab`].
    #[deprecated(
        since = "0.2.2",
        note = "byte-stream-only signal; snapshot viewers recover via seq gap. Design 01 / #158."
    )]
    AttachDropped {
        section_id: String,
        tab_id: String,
        reason: String,
    },
    /// Reply to [`Control::ListProjectActions`]. Empty `actions` is a
    /// valid result (unknown project, or project with no custom
    /// actions configured) — clients render the empty state rather
    /// than treating it as an error.
    ProjectActionsAck { actions: Vec<ProjectActionWire> },
    /// Reply to [`Control::ReadEnabledAgents`]. `view.agents` is in
    /// the canonical `core::agents::AGENTS` order — clients render
    /// without re-sorting.
    EnabledAgentsAck { view: EnabledAgentsViewWire },
    /// Reply to [`Control::CreateTask`]. `section_id` is the
    /// persisted section the caller should focus; its initial tab is
    /// always `"0"`.
    CreateTaskAck { section_id: String },
    /// Reply to [`Control::CreateTab`]. `tab_id` is the
    /// freshly-minted tab that was appended and made active.
    CreateTabAck { tab_id: String },
    /// Reply to [`Control::SetActiveTab`].
    SetActiveTabAck,
    /// Reply to [`Control::DeleteTab`]. `active_tab_id` is the
    /// section's new active tab after removal, or empty when the
    /// section is now tabless.
    DeleteTabAck { active_tab_id: String },
    /// Reply to [`Control::SetTabPinned`].
    SetTabPinnedAck { pinned: bool },
    /// Reply to [`Control::ReadAgentSettings`]. `view.agents`
    /// contains every agent in `core::agents::AGENTS` (canonical
    /// order) regardless of enabled state, so the Settings →
    /// Agents page can render rows for every agent at once.
    AgentSettingsAck { view: AgentSettingsViewWire },
    /// Reply to [`Control::SetAgentEnabled`].
    SetAgentEnabledAck { changed: bool },
    /// Reply to [`Control::SetDefaultAgent`].
    SetDefaultAgentAck { changed: bool },
    /// Reply to [`Control::SetAgentLaunchArgs`].
    SetAgentLaunchArgsAck { changed: bool },
    /// Reply to [`Control::ReadOpenInSettings`].
    OpenInSettingsAck { view: OpenInSettingsViewWire },
    /// Reply to [`Control::SetOpenInAppEnabled`].
    SetOpenInAppEnabledAck,
    /// Reply to [`Control::OpenProjectInApp`].
    OpenProjectInAppAck,
    /// Reply to [`Control::RunProjectAction`]. `tab_id` is the
    /// freshly-minted uuid for the spawned tab; the caller
    /// follows up with `Control::AttachTab` (or relies on the
    /// active-tab-changed event the desktop UI emits) to start
    /// receiving the action's PTY output.
    RunProjectActionAck { tab_id: String },
    /// Reply to [`Control::SetProjectAction`].
    SetProjectActionAck,
    /// Reply to [`Control::DeleteProjectAction`].
    DeleteProjectActionAck { deleted: bool },
    /// Uniform per-request failure frame. The daemon emits this in
    /// place of dropping the connection when a verb fails — keeps
    /// the channel open for other in-flight requests on the same
    /// session.
    ///
    /// `kind` is a small machine-classifiable enum (see [`ErrKind`])
    /// so clients can branch on the failure mode (retry on transient
    /// `internal`, surface auth UI on `unauthorised`, etc.) without
    /// pattern-matching on free-form `message` strings. `message`
    /// carries the human-readable detail and is logged / surfaced
    /// in toasts.
    ///
    /// Future domain children (`ojm.2..8`) emit `Err` instead of
    /// closing the connection on their own failure paths.
    Err {
        /// Pre-filled by `send_worker_reply`'s envelope wrapper, so
        /// callers don't have to thread it twice. Kept here for
        /// wire shape — payload is `{"kind":"err","request_id":N,"message":"...","err_kind":"..."}`
        /// after `#[serde(flatten)]` from `WorkerReplyEnvelope`.
        /// (Note the field name `err_kind` to avoid colliding with
        /// the envelope's outer `kind` discriminator.)
        message: String,
        #[serde(rename = "err_kind")]
        kind: ErrKind,
    },
    /// Reply to [`Control::CreateWorktreeTask`]. Carries the inline
    /// post-mutation [`TaskSummary`] plus the `project_id` it was
    /// inserted under so the issuer can locate the task in its
    /// project tree without a follow-up `ListProjects`.
    CreateWorktreeTaskAck {
        project_id: String,
        task: TaskSummary,
    },
    /// Reply to [`Control::SetTaskName`]. `changed` is `false` for an
    /// unknown id or a no-op rename — in that case `task` is the
    /// pre-existing snapshot (or absent if the id was unknown).
    SetTaskNameAck {
        changed: bool,
        task: Option<TaskSummary>,
    },
    /// Reply to [`Control::SetTaskPinned`]. `changed` is `false` for
    /// an idempotent re-set of the same value, or an unknown id.
    /// `task` is the post-mutation snapshot when the task exists.
    SetTaskPinnedAck {
        changed: bool,
        task: Option<TaskSummary>,
    },
    /// Reply to [`Control::DeleteTask`]. `removed` is `false` for an
    /// unknown id (idempotent). `project_id` echoes the request so
    /// the issuer can prune the right project subtree without
    /// re-deriving it.
    DeleteTaskAck {
        project_id: String,
        task_id: String,
        removed: bool,
    },
    /// Reply to [`Control::ReadProjectBranches`]. Empty list for
    /// unknown projects.
    ProjectBranchesAck { branches: Vec<String> },
    /// Reply to [`Control::ReadPrimaryBranch`]. `None` when
    /// the project has no current branch yet.
    ReadPrimaryBranchAck { branch: Option<String> },
    /// Reply to [`Control::ReadRepoDefaultCommitAction`]. `action ==
    /// None` means the user hasn't recorded a preference; UI
    /// defaults to `"commit"`.
    ReadRepoDefaultCommitActionAck { action: Option<String> },
    /// Reply to [`Control::ReadActiveGitState`]. `state == None`
    /// when the project id is unknown — UI shows the empty state
    /// rather than surfacing an error.
    ActiveGitStateAck { state: Option<ActiveGitStateWire> },
    /// Reply to [`Control::ReadChangedFiles`]. `files == None` when
    /// the project id is unknown.
    ChangedFilesAck { files: Option<Vec<ChangedFileWire>> },
    /// Reply to [`Control::ReadProjectGithubUrl`]. `url == None`
    /// when the project is untracked, has no `origin`, or `origin`
    /// isn't a github.com URL.
    ProjectGithubUrlAck { url: Option<String> },
    /// Reply to [`Control::ReadRecentCommits`]. `view == None` when
    /// the project id is unknown. Errors propagate as
    /// [`WorkerReply::Err`].
    RecentCommitsAck { view: Option<RecentCommitsWire> },
    /// Reply to [`Control::ReadCommitFileChanges`]. `files == None`
    /// when the project id is unknown.
    CommitFileChangesAck {
        files: Option<Vec<BranchCompareFileWire>>,
    },
    /// Reply to [`Control::ReadBranchSettings`]. `settings == None`
    /// when the project is unknown or lacks repo metadata.
    BranchSettingsAck {
        settings: Option<ResolvedBranchSettingsWire>,
    },
    /// Reply to [`Control::SetBranchSetting`]. `changed=true` iff
    /// the persisted store actually changed.
    SetBranchSettingAck { changed: bool },
    /// `another-one-ojm.5` — ack for [`Control::StageChangedFile`].
    /// Carries the post-mutation `changed_files` snapshot inline so
    /// the issuing client refreshes the right-sidebar Changes pane
    /// without a follow-up `ReadChangedFiles` round-trip — see the
    /// "Push vs pull" contract block above. Empty list means the
    /// working tree is clean after the stage.
    StageChangedFileAck { changed_files: Vec<ChangedFileWire> },
    /// `another-one-ojm.5` — ack for [`Control::UnstageChangedFile`].
    /// Same inline-snapshot semantics as [`Self::StageChangedFileAck`].
    UnstageChangedFileAck { changed_files: Vec<ChangedFileWire> },
    /// `another-one-ojm.5` — ack for [`Control::StageAllChanges`].
    /// Inline post-mutation snapshot.
    StageAllChangesAck { changed_files: Vec<ChangedFileWire> },
    /// `another-one-ojm.5` — ack for [`Control::UnstageAllChanges`].
    UnstageAllChangesAck { changed_files: Vec<ChangedFileWire> },
    /// `another-one-ojm.5` — ack for [`Control::DiscardChangedFile`].
    DiscardChangedFileAck { changed_files: Vec<ChangedFileWire> },
    /// `another-one-ojm.5` — ack for [`Control::DiscardAllChanges`].
    /// Returns the final `changed_files` snapshot after the batch plus
    /// any per-path failures the caller should surface.
    DiscardAllChangesAck {
        changed_files: Vec<ChangedFileWire>,
        failures: Vec<String>,
    },
    /// `another-one-ojm.5` — ack for [`Control::RunToolbarGitAction`].
    /// Carries the `ToolbarActionOutcome` (toast + warning/refresh
    /// flags) the issuing client uses to render its snackbar and
    /// invalidate the active git-state / changed-files providers.
    RunToolbarGitActionAck { outcome: ToolbarActionOutcome },
    /// `another-one-ojm.5` — ack for [`Control::CreateBranch`].
    /// `section_id` is the new worktree task's section id (empty
    /// string for the current-task branch-swap case) so the issuing
    /// client navigates to it directly. The post-mutation project
    /// tree refresh rides along as `projects` per the inline-snapshot
    /// contract — the mobile UI repaints the projects drawer
    /// without a follow-up `ListProjects` round-trip.
    CreateBranchAck {
        section_id: String,
        projects: Vec<ProjectSummary>,
    },
    /// `another-one-ojm.5` — ack for [`Control::CreateReviewTask`].
    /// Same inline-snapshot semantics as [`Self::CreateBranchAck`].
    CreateReviewTaskAck {
        section_id: String,
        projects: Vec<ProjectSummary>,
    },
    /// Reply to [`Control::ReadPullRequestStatus`]. `status: None`
    /// when the project has no open PR for its current branch (or
    /// the project id is unknown). Mutator-snapshot rules don't
    /// apply — this is a pure read.
    ReadPullRequestStatusAck { status: Option<PullRequestStatus> },
    /// Reply to [`Control::ReadPullRequestChecks`]. Three-state
    /// payload mirrors the GPUI desktop's
    /// `core::git_actions::find_pull_request_checks` contract:
    ///   * `Some(list)` — PR exists, here are its check rows.
    ///   * `None` — no PR for the branch, or unknown project id.
    ///     gh CLI / network failures come back as [`WorkerReply::Err`].
    PullRequestChecksAck { checks: Option<Vec<Check>> },
    /// Reply to [`Control::ListProjectPullRequests`]. `prs: None`
    /// covers the unknown-project case; gh CLI / auth / network
    /// failures arrive as [`WorkerReply::Err`].
    ListProjectPullRequestsAck {
        prs: Option<Vec<ProjectPagePullRequest>>,
    },
    /// Reply to [`Control::ReadGitActionScripts`].
    GitActionScriptsAck { view: GitActionScriptsView },
    /// Reply to [`Control::SetGitCommitScript`]. Inline-snapshot per
    /// the mutator contract: the `changed` flag is the post-mutation
    /// state so the issuing client doesn't need a follow-up read to
    /// know whether anything moved.
    SetGitCommitScriptAck { changed: bool },
    /// Reply to [`Control::ResetGitCommitScript`].
    ResetGitCommitScriptAck { changed: bool },
    /// Reply to [`Control::SetGitPrScript`].
    SetGitPrScriptAck { changed: bool },
    /// Reply to [`Control::ResetGitPrScript`].
    ResetGitPrScriptAck { changed: bool },
    /// Reply to [`Control::ReadShortcutSettings`].
    ShortcutSettingsAck { view: ShortcutSettingsView },
    /// Reply to [`Control::SetShortcutBinding`].
    SetShortcutBindingAck,
    /// Reply to [`Control::ResetShortcutBinding`].
    ResetShortcutBindingAck,
    /// Reply to [`Control::ReadMcpSettings`].
    McpSettingsAck { view: McpSettingsView },
    /// Reply to [`Control::McpAddFromCatalog`].
    McpAddFromCatalogAck,
    /// Reply to [`Control::McpToggle`].
    McpToggleAck,
    /// Reply to [`Control::McpRemove`].
    McpRemoveAck,
    // ── Daemon-canonical Term (design 01) ─────────────────────
    /// Reply to [`Control::TerminalSubscribe`]. The first
    /// `TerminalFrame` push for this `(section_id, tab_id)` may
    /// arrive before, after, or interleaved with this ack — viewers
    /// must not assume an ordering between the call reply and the
    /// initial pushed frame.
    TerminalSubscribeAck,
    /// Reply to [`Control::TerminalUnsubscribe`]. After this ack the
    /// daemon emits no further frames for the `(viewer, section_id,
    /// tab_id)` triple, but a frame already in flight may arrive
    /// after the ack — viewers should drop frames whose
    /// `(section_id, tab_id)` they no longer hold a subscription
    /// for.
    TerminalUnsubscribeAck,
    /// Reply to [`Control::TerminalReadScrollback`].
    TerminalScrollback {
        section_id: String,
        tab_id: String,
        reply: TerminalScrollbackReply,
    },
    /// Reply to [`Control::TerminalSearch`].
    TerminalSearch {
        section_id: String,
        tab_id: String,
        reply: TerminalSearchReply,
    },
    /// Reply to [`Control::TerminalInput`]. Empty ack — effects flow
    /// through the next pushed frame for the tab.
    TerminalInputAck,
    /// **Daemon-pushed** (request_id == 0). One grid frame for a
    /// subscribed viewer. The transport layer demuxes this into
    /// `daemon_transport::SessionEvent::TerminalFrame` so consumers
    /// don't pattern-match `WorkerReply` variants on the hot path.
    TerminalFrame {
        section_id: String,
        tab_id: String,
        frame: TerminalFrame,
    },
    /// **Daemon-pushed**. Title bar text changed for `(section_id,
    /// tab_id)`. Independent of the frame stream so viewers can
    /// react regardless of frame cadence (or whether they're even
    /// subscribed for frames — unfocused tabs still want title
    /// updates in the sidebar).
    TerminalTitle {
        section_id: String,
        tab_id: String,
        title: String,
    },
    /// **Daemon-pushed**. Title was reset to the default for
    /// `(section_id, tab_id)`.
    TerminalResetTitle { section_id: String, tab_id: String },
    /// **Daemon-pushed**. Bell rang for `(section_id, tab_id)`.
    /// Renderer surfaces it (visual flash, dock badge).
    TerminalBell { section_id: String, tab_id: String },
    /// **Daemon-pushed**. The daemon spawned a fresh PTY for
    /// `(section_id, tab_id)`. Carries the OS process id so callers
    /// (resource indicator, debugger attach) can reference it. Phase 4
    /// of design 01 / #158.
    TerminalLaunched {
        section_id: String,
        tab_id: String,
        /// `None` when `portable_pty` couldn't read the child pid
        /// (Windows shells, sub-shells we don't track). Renderers
        /// treat None as "unknown pid" and skip resource
        /// attribution rather than blocking the launch ack.
        process_id: Option<u32>,
    },
    /// **Daemon-pushed**. The daemon-owned PTY for `(section_id,
    /// tab_id)` exited. `success` mirrors `ExitStatus::success()`;
    /// `code` mirrors `ExitStatus::code()` when the child exited via
    /// a numeric code (None on signal-killed children, in which case
    /// `success` is false). Renderers surface a closed-tab state.
    TerminalExited {
        section_id: String,
        tab_id: String,
        success: bool,
        code: Option<i32>,
    },
}

/// Wire mirror of the active git-state view and the underlying
/// `core::project_store::ProjectGitState`. Carries the metadata the
/// titlebar's idle-primary-action selection needs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActiveGitStateWire {
    pub current_branch: Option<String>,
    pub ahead_count: u32,
    pub behind_count: u32,
}

/// Wire mirror of one commit file-change row.
/// One entry per file changed inside a commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchCompareFileWire {
    pub path: String,
    pub original_path: Option<String>,
    /// Single git status char ('A', 'M', 'D', 'R', 'C', 'T') as a
    /// 1-char string.
    pub status: String,
    pub additions: i32,
    pub deletions: i32,
}

/// Wire mirror of resolved project branch settings. Powers the project
/// page's Configuration panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedBranchSettingsWire {
    pub root_project_id: String,
    pub available_branches: Vec<String>,
    pub configured_default_branch: Option<String>,
    pub effective_default_branch: Option<String>,
    pub configured_default_target_branch: Option<String>,
    pub effective_default_target_branch: Option<String>,
}

/// Wire mirror of one recent commit row. Carries
/// pre-computed display strings — the daemon does the rendering work
/// (chrono is already a dep there) so the UI doesn't need a
/// humanise-duration package on the client side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitWire {
    pub id: String,
    pub short_id: String,
    pub subject: String,
    pub author_name: String,
    pub authored_relative: String,
}

/// Wire mirror of the recent commits view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentCommitsWire {
    pub current_branch: Option<String>,
    pub has_more: bool,
    pub commits: Vec<CommitWire>,
}

/// Wire mirror of one changed-file row. Carries
/// the raw `git status` chars + diff counts; UI maps them to glyphs
/// per the desktop's existing `changed_file_status_*` tables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangedFileWire {
    pub path: String,
    pub original_path: Option<String>,
    pub staged_additions: i32,
    pub staged_deletions: i32,
    pub unstaged_additions: i32,
    pub unstaged_deletions: i32,
    /// Single-char index status, encoded as a 1-char `String` so
    /// JSON wire remains uniform across clients.
    pub index_status: String,
    /// Single-char worktree status, same encoding as
    /// `index_status`.
    pub worktree_status: String,
    pub untracked: bool,
}

/// Lossy wire projection of
/// `core::git_actions::ToolbarActionOutcome`. Same field shape as the
/// client-side toolbar action outcome; `warning` distinguishes the
/// snackbar palette and `refresh_git_state` tells the issuing client
/// to invalidate the active changed-files / git-state providers after
/// the call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolbarActionOutcome {
    pub toast_message: String,
    pub warning: bool,
    pub refresh_git_state: bool,
}

/// Lossy wire projection of `core::git_actions::PullRequestStatus`.
/// One row per project that has an open PR for its current branch;
/// drives the titlebar's PR pill state across desktop + mobile
/// (Create vs Open vs Draft, plus the disabled state on the Git
/// Actions dropdown).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestStatus {
    pub number: u64,
    pub url: String,
    pub state: PullRequestState,
}

/// Mirror of `core::git_actions::PullRequestState`. Wire-serialised
/// as lowercase strings — UI maps each to a chip palette + the
/// titlebar PR pill copy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

/// Lossy wire projection of `core::git_actions::PullRequestCheck`.
/// One row per CI check on the project's current PR; drives the
/// right-sidebar Checks pane on every connected client. Bucket is
/// already classified server-side so mobile doesn't have to
/// re-derive the colour mapping from the freeform `state` string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub state: String,
    pub bucket: CheckBucket,
    pub description: Option<String>,
    pub link: Option<String>,
    pub duration_text: Option<String>,
}

/// Mirror of `core::git_actions::PullRequestCheckBucket`. Wire form
/// is snake_case; clients render glyph + colour off this.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckBucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
}

/// Lossy wire projection of `core::git_actions::ProjectPagePullRequest`.
/// One entry per row in the project page's Open PRs section.
/// `review_required` / `review_requested_to_me` / `created_by_me`
/// are pre-derived on the daemon so mobile doesn't need to
/// re-implement the filter-index logic that gates each row's chip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPagePullRequest {
    pub number: u64,
    pub url: String,
    pub title: String,
    pub branch: String,
    pub author: String,
    pub lines_added: i32,
    pub lines_removed: i32,
    pub draft: bool,
    pub review_required: bool,
    pub review_requested_to_me: bool,
    pub created_by_me: bool,
    pub state: PullRequestState,
}

/// Coarse classification of a daemon-side failure. Keep small —
/// callers branch on this in UI code, so adding a variant is a
/// commitment to render it. Most failures fall into `internal` (an
/// unexpected error worth logging) or `unsupported` (the daemon is
/// older than the client and doesn't know this verb).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrKind {
    /// The verb referenced an `id` (project/task/tab/section) the
    /// daemon doesn't recognise. Typically a stale client cache
    /// after the user removed something on another peer; clients
    /// should refresh their view rather than retrying the call.
    UnknownId,
    /// The daemon doesn't speak this verb yet — likely an older
    /// daemon paired with a newer client. The client can degrade
    /// gracefully (hide the offending UI affordance) until the
    /// host upgrades.
    Unsupported,
    /// The daemon recognises the verb but the calling peer isn't
    /// authorised to use it (e.g. read-only viewer trying to
    /// mutate). Reserved for the multi-peer authz model that
    /// lands after the foundation; today this is unreachable.
    Unauthorised,
    /// Any other failure — disk full, command spawn failed, git
    /// returned non-zero with stderr we don't classify. Treat as
    /// transient and retryable.
    Internal,
}

/// Lossy wire projection of `core::project_store::Project`, with
/// nested `tasks` + `tabs` so one `ListProjects` response tells the
/// mobile UI everything it needs to render its home drawer + each
/// project's task list without follow-up round-trips.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    /// Absolute path on the daemon host. Read-only on the wire —
    /// mobile never dereferences this, the desktop does all FS work.
    pub path: String,
    pub kind: ProjectKind,
    /// Last-observed current branch from the ProjectStore's
    /// `checkout.current_branch`; may be `None` if never read.
    pub current_branch: Option<String>,
    pub tasks: Vec<TaskSummary>,
    /// Repo grouping id mirroring `core::project_store::Project::repo_id`
    /// — same-repo worktrees share this. Wire-additive; older daemons
    /// leave it empty and clients fall back to grouping by `id`.
    #[serde(default)]
    pub repo_id: String,
    /// `core::project_store::Project::worktree_name`. Wire-additive.
    #[serde(default)]
    pub worktree_name: Option<String>,
    /// Opaque JSON-serialised `core::project_store::ProjectCheckoutState`.
    /// Daemon-proto keeps this opaque so the project schema can
    /// evolve in `core` without forcing a `daemon-proto` bump for
    /// every checkout-state extension. Clients deserialize via
    /// `serde_json::from_value`. `None` for older daemons that
    /// didn't carry the field.
    #[serde(default)]
    pub checkout: Option<serde_json::Value>,
    /// Opaque JSON-serialised `core::project_store::ProjectBranchSettings`.
    /// Same opaque-pass-through pattern as `checkout`.
    #[serde(default)]
    pub branch_settings: Option<serde_json::Value>,
    /// Compatibility fallback for older projections that carried
    /// non-global custom actions on the project row. New daemons
    /// store repo-scoped actions on [`RepoSummary::actions`].
    #[serde(default)]
    pub actions: serde_json::Value,
}

/// Wire mirror of `core::project_store::RepoRecord`. Carried
/// alongside [`ProjectSummary`] on [`WorkerReply::ProjectList`] so
/// the sidebar's repo-grouping data (branch catalog, common git
/// dir) survives the daemon → client projection roundtrip.
///
/// Before this struct existed, `absorb_projection` on the client
/// side had to synthesise one bare `RepoRecord` per project from
/// wire fields alone — branches, `common_dir`, and the committed
/// branch order were all lost. Desktop absorbs its own projection
/// back through the paired in-memory session on every state
/// change, so the lossy roundtrip silently wiped locally-resolved
/// branch metadata dozens of times per second. See the
/// sidebar-flicker regression surfaced by the #125 watchdog.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoSummary {
    pub id: String,
    /// Absolute git common dir on the daemon host. `None` when the
    /// registry hasn't resolved it yet (freshly added repo with no
    /// git inspection run).
    #[serde(default)]
    pub common_dir: Option<String>,
    /// Opaque JSON-serialised `Vec<core::project_store::ProjectAction>`.
    /// Non-global custom actions scoped to this resolved local git repo.
    #[serde(default)]
    pub actions: serde_json::Value,
    /// Branch names in the user-visible sort order the desktop
    /// sidebar already computed. Clients render in this exact order
    /// so the same repo looks identical on both sides.
    #[serde(default)]
    pub branch_order: Vec<String>,
    /// Per-branch metadata, one entry per `branch_order` name.
    /// Emitted as a flat list rather than a map for stable wire
    /// ordering; clients rebuild the HashMap on absorb.
    #[serde(default)]
    pub branches: Vec<RepoBranchSummary>,
}

/// Wire mirror of `core::project_store::RepoBranchRecord`. Carries
/// the branch-level data the sidebar needs for its row renderer
/// (ahead/behind, last-commit text, default-branch flag). Explicit
/// fields rather than an opaque JSON passthrough because the shape
/// is stable and mobile's sidebar decides layout from these
/// numbers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoBranchSummary {
    pub name: String,
    #[serde(default)]
    pub last_commit_relative: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub ahead_count: usize,
    #[serde(default)]
    pub behind_count: usize,
}

/// Result of the daemon-side `gh auth status` probe. The daemon owns
/// the fork/exec; clients render from the projection. `tag`/`content`
/// serialisation keeps the JSON form identical to a Rust enum and
/// lets us add variants (e.g. an explicit `Error { message }`) later
/// without breaking older clients — unknown variants will fail decode
/// and clients will fall back to `None` (= "unknown, don't paint").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum GhAuthStatusWire {
    /// Probe in flight. Set when the daemon kicks the worker off
    /// at boot or on Recheck so clients can show a spinner button
    /// instead of the stale answer.
    Checking,
    /// `gh auth status` exited 0.
    Authenticated,
    /// `gh` was found on `$PATH` but `auth status` exited non-zero.
    NotAuthenticated,
    /// `gh` was not found on the daemon's `$PATH`.
    GhMissing,
}

/// Daemon-side resource-usage projection: the daemon's own RSS/CPU
/// plus the per-PTY tree it owns. Replaces the client's local
/// `/proc/self/status` poll — on desktop the numbers were correct
/// by coincidence (client+daemon are co-located), but on mobile the
/// phone was rendering its own RSS, which has nothing to do with
/// the paired desktop daemon. The daemon publishes; clients render.
/// See #156. Wire-additive: older daemons omit the field, clients
/// treat as `None` and surface no metrics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DaemonResourceUsageWire {
    #[serde(default)]
    pub total_cpu_percent: f32,
    #[serde(default)]
    pub total_memory_bytes: u64,
    #[serde(default)]
    pub ram_share_percent: f32,
    /// Number of logical CPU cores on the daemon host. Used by clients
    /// to normalize `cpu_percent` values (which are per-core) into a
    /// 0–100% fraction of total system CPU capacity. Defaults to 1 so
    /// older clients that don't send this field degrade gracefully.
    #[serde(default = "default_cpu_core_count")]
    pub cpu_core_count: u16,
    #[serde(default)]
    pub session_count: usize,
    /// The daemon-host process row (CPU/RSS for the binary that
    /// owns the PTYs + iroh endpoint + project store).
    #[serde(default)]
    pub app: DaemonResourceUsageRowWire,
    /// Per-project aggregation of tracked PTY processes.
    #[serde(default)]
    pub projects: Vec<DaemonResourceUsageProjectWire>,
}

fn default_cpu_core_count() -> u16 {
    1
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DaemonResourceUsageRowWire {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub cpu_percent: f32,
    #[serde(default)]
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DaemonResourceUsageProjectWire {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub cpu_percent: f32,
    #[serde(default)]
    pub memory_bytes: u64,
    #[serde(default)]
    pub tasks: Vec<DaemonResourceUsageTaskWire>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DaemonResourceUsageTaskWire {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub cpu_percent: f32,
    #[serde(default)]
    pub memory_bytes: u64,
    #[serde(default)]
    pub sessions: Vec<DaemonResourceUsageSessionWire>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DaemonResourceUsageSessionWire {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub label: String,
    /// `&'static str` asset path on the desktop (e.g.
    /// `"assets/icons/icons__codex-ai.svg"`); transmitted as a
    /// String so the wire shape stays platform-agnostic.
    #[serde(default)]
    pub icon_path: String,
    #[serde(default)]
    pub cpu_percent: f32,
    #[serde(default)]
    pub memory_bytes: u64,
}

/// Per-user UI state mirrored from `core::project_store::UiState`.
/// Wire-additive on `WorkerReply::ProjectList` so both clients render
/// the same expand/pin/focus state from the daemon's projection
/// instead of each maintaining their own.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiSnapshot {
    /// Repo IDs the user has expanded in the projects sidebar.
    #[serde(default)]
    pub expanded_repo_ids: Vec<String>,
    /// `(project_id, task_id)` of tasks pinned to the top of the
    /// sidebar.
    #[serde(default)]
    pub pinned_task_ids: Vec<(String, String)>,
    /// Last section the user focused — used to restore focus on
    /// reconnect.
    #[serde(default)]
    pub last_active_section_id: Option<String>,
    /// Whether the desktop's left sidebar is open. Mobile uses this
    /// hint to start in the right mobile-view nav state on reconnect.
    #[serde(default)]
    pub left_sidebar_open: bool,
    /// Whether the sidebar shows per-task git metadata (lines added
    /// / removed, last commit relative). Mirrors
    /// `core::project_store::UiState::show_sidebar_git_metadata`.
    /// Wire-additive.
    #[serde(default)]
    pub show_sidebar_git_metadata: bool,
    /// Opaque JSON-serialised `HashMap<ShortcutAction, ShortcutBinding>`
    /// from `UiState::shortcuts`. Daemon-proto stays free of the
    /// shortcut enum/binding shapes by passing them through.
    #[serde(default)]
    pub shortcuts: Option<serde_json::Value>,
    /// Opaque JSON-serialised `HashMap<String, AgentLaunchArgs>` from
    /// `UiState::agent_launch_args_overrides`.
    #[serde(default)]
    pub agent_launch_args_overrides: Option<serde_json::Value>,
    /// App-wide theme preference (`"light"` / `"dark"` /
    /// `"system"`). Carried as a string to keep daemon-proto free
    /// of the `ThemeMode` enum shape. `None` = older daemon / no
    /// preference stored (client should fall back to OS).
    /// Wire-additive.
    #[serde(default)]
    pub theme_mode: Option<String>,
    /// `UiState::default_agent_id`. Wire-additive.
    #[serde(default)]
    pub default_agent_id: Option<String>,
    /// `UiState::enabled_agents` — the user-configured allowlist of
    /// agent ids. `None` means "all agents enabled" (the default);
    /// `Some(set)` is the explicit allowlist. Carried as a Vec on
    /// the wire for stable JSON ordering.
    #[serde(default)]
    pub enabled_agents: Option<Vec<String>>,
    /// Agent ids whose executable the **daemon** can actually
    /// invoke. Populated by the daemon's own `$PATH` + filesystem
    /// probe — NOT the client's. This is the single source of
    /// truth for "which agents can be launched" because the
    /// daemon is the process that fork/execs them. Clients
    /// render/filter UI from this list instead of probing their
    /// own filesystem (a mobile client has no `claude` / `codex`
    /// binary locally, but the paired desktop daemon does, and
    /// the phone should offer every agent the desktop can run).
    /// Empty list = no agents installed on the daemon host.
    /// Wire-additive; older daemons omit the field and clients
    /// should treat that as "unknown, don't filter".
    #[serde(default)]
    pub available_agent_ids: Option<Vec<String>>,
    /// `OpenInAppKind` ids the **daemon** can actually launch on
    /// its host. Populated by the daemon's platform-specific
    /// `is_open_in_app_available` probe — NOT the client's. The
    /// daemon is the process that fork/execs the editors, so it's
    /// the only correct probe target. Mobile clients have none of
    /// these binaries locally; the paired daemon's list is what
    /// they render. `None` = older daemon / probe hasn't run yet;
    /// clients should fall back to local detection so a desktop
    /// connected to an older daemon still works. Wire-additive.
    #[serde(default)]
    pub available_open_in_apps: Option<Vec<String>>,
    /// Result of the daemon's `gh auth status` probe. The desktop
    /// client used to fork/exec `gh` itself, but the architectural
    /// rule is the daemon owns host-process probes — clients (mobile
    /// or desktop) just render this projection. `None` = older
    /// daemon / probe hasn't completed; treat as "unknown, don't
    /// surface the overlay yet". Wire-additive (#156).
    #[serde(default)]
    pub gh_auth_status: Option<GhAuthStatusWire>,
    /// Live RSS/CPU sample from the daemon host. Replaces the
    /// resource-indicator widget's old client-side `/proc/self`
    /// probe. The daemon owns the PTYs / iroh endpoint / project
    /// store, so the daemon's RSS is what the user actually cares
    /// about; mobile clients used to display their phone's RSS,
    /// which was meaningless. `None` on older daemons or before
    /// the first sample lands; clients should render an empty
    /// indicator. Wire-additive (#156).
    #[serde(default)]
    pub daemon_resource_usage: Option<DaemonResourceUsageWire>,
    /// Opaque JSON-serialised
    /// `HashMap<OpenInAppKind, OpenInAppKindState>` from
    /// `UiState::open_in_apps`.
    #[serde(default)]
    pub open_in_apps: Option<serde_json::Value>,
    /// `UiState::preferred_open_in_app`. Carried as the kind's id
    /// string ("cursor", "zed", …); `None` when no preference set.
    #[serde(default)]
    pub preferred_open_in_app: Option<String>,
    /// `UiState::git_commit_generation_script`. Wire-additive.
    #[serde(default)]
    pub git_commit_generation_script: Option<String>,
    /// `UiState::git_pr_generation_script`. Wire-additive.
    #[serde(default)]
    pub git_pr_generation_script: Option<String>,
    /// Opaque JSON-serialised
    /// `Option<core::settings::AgentSettingsLlm>` for the git-commit
    /// generation LLM. Daemon-proto carries it opaquely so we don't
    /// need a wire mirror for `AgentSettingsLlm`.
    #[serde(default)]
    pub git_commit_generation_llm: Option<serde_json::Value>,
    /// Opaque JSON-serialised same shape as `git_commit_generation_llm`
    /// but for the PR-generation flow.
    #[serde(default)]
    pub git_pr_generation_llm: Option<serde_json::Value>,
}

/// Lossy wire projection of `core::project_store::Task`. Contains
/// enough for the mobile task page to render the tab strip and
/// request an attach; no live PTY state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub name: String,
    /// Stable section id — half of the compound
    /// `TerminalRuntimeKey { section_id, tab_id }` used to address
    /// a live PTY.
    pub section_id: String,
    pub branch_name: String,
    pub active_tab_id: String,
    pub tabs: Vec<TabSummary>,
    /// Desktop UI pins tasks via `UiState::pinned_task_ids` so they
    /// sort to the top of the sidebar; mirrored on mobile so the
    /// projects-drawer rendering matches.
    pub pinned: bool,
    /// Human-readable "5 minutes ago" string for the task's branch
    /// last commit. Populated from `branch.last_commit_relative` on
    /// the desktop's `ProjectStore`. Empty when the project hasn't
    /// been git-refreshed yet, so callers can join with `•` and
    /// drop empty segments. Wire-additive: older daemons will
    /// `serde(default)` this to `""`.
    #[serde(default)]
    pub last_commit_relative: String,
    /// Lines added on the task's working-tree branch since its
    /// merge base. Populated from `branch.lines_added` (only set
    /// when the branch is the worktree's current branch — by
    /// definition true for AnotherOne tasks). Wire-additive,
    /// defaults to `0`.
    #[serde(default)]
    pub lines_added: i32,
    /// Lines removed on the task's working-tree branch since its
    /// merge base. Wire-additive, defaults to `0`.
    #[serde(default)]
    pub lines_removed: i32,
    /// Project id this task targets for branch / Open-In / git
    /// actions. Equals `root_project_id` for plain tasks, points at
    /// the worktree's own `Project` entry for worktree tasks. The
    /// titlebar's "Open In" + Git Actions + Custom Actions all
    /// resolve their working directory through this id (matches
    /// `core::project_store::Task::target_project_id`). Wire-
    /// additive — older daemons leave it empty, in which case
    /// callers fall back to the root project id.
    #[serde(default)]
    pub target_project_id: String,
    /// Working directory the daemon will use when spawning a tab
    /// under this task. Mirrors `core::project_store::Task::cwd`.
    /// String form for wire neutrality (paths cross platforms).
    /// Wire-additive, `None` means "fall back to the project root".
    #[serde(default)]
    pub cwd: Option<String>,
    /// `core::project_store::Task.next_tab_id` mirrored so a client
    /// reasoning about future tab IDs (or sorting tabs by creation
    /// order) doesn't have to recompute. Wire-additive.
    #[serde(default)]
    pub next_tab_id: usize,
    /// `core::project_store::Task::root_project_id`. The id of the
    /// task's owning *root* project (worktree tasks point at the
    /// worktree's own `Project` entry via `target_project_id`; this
    /// stays at the root). Wire-additive.
    #[serde(default)]
    pub root_project_id: String,
    /// Opaque JSON-serialised `core::project_store::TaskKind`. Carried
    /// opaquely so daemon-proto stays free of `TaskKind`'s nested
    /// shape (Direct vs. Worktree variants with payload).
    #[serde(default)]
    pub kind: Option<serde_json::Value>,
    /// `core::project_store::Task::worktree_project_id` — the id of
    /// the worktree-kind project this task uses, if it's a worktree
    /// task. `None` for direct tasks. Wire-additive.
    #[serde(default)]
    pub worktree_project_id: Option<String>,
    /// Opaque JSON-serialised `core::project_store::TaskWorktree`.
    /// Worktree tasks carry their workspace metadata here instead of
    /// requiring a top-level worktree Project in `ProjectSummary`.
    #[serde(default)]
    pub worktree: Option<serde_json::Value>,
}

/// Lossy wire projection of
/// `core::project_store::PersistedTerminalTab`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TabSummary {
    pub id: String,
    pub title: String,
    pub provider: Option<AgentProvider>,
    /// `true` iff the desktop has a live `LiveTerminalRuntime` for
    /// this tab right now. Persisted-but-not-launched tabs report
    /// `false` and an `AttachTab` for them returns no data.
    pub running: bool,
    /// User-pinned tabs stay resident across restarts on desktop;
    /// mobile shows a pin glyph on the chip and sorts them left.
    pub pinned: bool,
    /// User-overridden tab title. When `Some(_)`, prefer this over
    /// the auto-generated title field above (which tends to be the
    /// agent provider's default label).
    pub fixed_title: Option<String>,
    /// Persisted launch/restore state for this tab. `Failed` means
    /// the daemon could not spawn the PTY and the failure fields
    /// should be shown instead of silently attaching to nothing.
    #[serde(default)]
    pub restore_status: TerminalRestoreStatus,
    /// Short user-facing launch failure summary.
    #[serde(default)]
    pub failure_message: Option<String>,
    /// Longer diagnostic details from the PTY launcher, when
    /// available.
    #[serde(default)]
    pub failure_details: Option<String>,
    /// Opaque JSON-serialised `core::agents::TerminalLaunchConfig`.
    /// Daemon-proto keeps this opaque so the persisted-tab schema can
    /// evolve in `core` without requiring a `daemon-proto` bump.
    /// Clients deserialize via `serde_json::from_value` into the
    /// canonical core type. `None` for tabs that were never
    /// configured (legacy persisted tabs from before the field
    /// existed).
    #[serde(default)]
    pub launch_config: Option<serde_json::Value>,
}

/// Mirror of `core::project_store::ProjectKind`. Wire-serialised as
/// lowercase strings: `"root"` / `"worktree"`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    #[default]
    Root,
    Worktree,
}

/// Mirror of `core::agents::AgentProviderKind`. Wire-serialised as
/// snake_case: `"claude_code"` / `"codex"` / `"cursor_agent"` etc.
/// `Shell` is the catch-all for tabs launched without an agent
/// provider set (plain PTY).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProvider {
    ClaudeCode,
    CursorAgent,
    Codex,
    Pi,
    Droid,
    Gemini,
    OpenCode,
    Amp,
    RovoDev,
    Forge,
    /// Catch-all for tabs launched without an agent provider set.
    /// `Default` lands here so `..Default::default()` on test
    /// fixtures yields a benign "plain shell" tab.
    #[default]
    Shell,
}

/// Wire projection of one row from Settings → Open In.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenInAppSettingsRowWire {
    pub id: String,
    pub label: String,
    pub description: String,
    pub icon_path: String,
    pub enabled: bool,
}

/// Wire projection of the full Settings → Open In page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenInSettingsViewWire {
    pub available_apps: Vec<OpenInAppSettingsRowWire>,
}

/// Wire projection of `another_one_core::project_store::ProjectAction`.
/// Field-for-field compatible with the client project-action DTO so
/// adapters can decode the wire JSON without a mapping pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectActionWire {
    pub id: String,
    pub name: String,
    pub icon: ProjectActionIconWire,
    pub run_on_worktree_create: bool,
    pub scope: ProjectActionScopeWire,
    pub kind: ProjectActionKindWire,
}

/// Wire mirror of `core::project_store::ProjectActionIcon`. Stable
/// kebab-case ids match the GPUI on-disk format (`projects.json`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionIconWire {
    Play,
    Test,
    Lint,
    Configure,
    Build,
    Debug,
    Agent,
}

/// Wire mirror of `core::project_store::ProjectActionScope`. Project
/// rows render before global rows in the dropdown.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionScopeWire {
    Project,
    Global,
}

/// Wire mirror of `core::project_store::ProjectActionAccess`. Fed
/// through to the agent CLI's permission flag at run time —
/// `default` passes nothing extra, the other three map to
/// `--read-only`, `--workspace-write`, `--full-access`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionAccessWire {
    Default,
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

/// Wire mirror of `core::project_store::ProjectActionKind`. Tagged
/// union — `kind: "shell"` carries `command`, `kind: "agent"`
/// carries the prompt + provider-specific knobs.
///
/// This uses the externally-tagged shape expected by existing client
/// project-action decoders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectActionKindWire {
    Shell {
        command: String,
    },
    Agent {
        prompt: String,
        provider: AgentProvider,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        traits: Option<String>,
        #[serde(default)]
        mode: Option<String>,
        access: ProjectActionAccessWire,
    },
}

/// Wire projection of one entry in `another_one_core::agents::AGENTS`.
/// Field-for-field compatible with the client agent-summary DTO so
/// adapters can decode wire JSON directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummaryWire {
    /// Stable id used by `submit_new_task` and the agent settings
    /// verbs (`set_agent_enabled`, etc.).
    pub id: String,
    pub label: String,
    pub icon_path: String,
    pub provider: Option<AgentProvider>,
}

/// Wire projection of the enabled-agents view.
/// Pairs the enabled-agents list with the user's preferred default
/// (the chip the new-task modal pre-checks on open).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnabledAgentsViewWire {
    pub agents: Vec<AgentSummaryWire>,
    pub default_agent_id: Option<String>,
}

/// Wire projection of the agent settings row.
/// One row of the Settings → Agents page — label + icon paired with
/// per-host enabled / default flags and the per-agent launch-args
/// list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettingsRowWire {
    pub id: String,
    pub label: String,
    pub icon_path: String,
    pub provider: Option<AgentProvider>,
    pub enabled: bool,
    pub is_default: bool,
    pub launch_args: Vec<String>,
}

/// Wire projection of the agent settings view.
/// Drives the Settings → Agents page; rows are in the canonical
/// `core::agents::AGENTS` order so the page renders without
/// re-sorting after each toggle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettingsViewWire {
    pub agents: Vec<AgentSettingsRowWire>,
    pub default_agent_id: Option<String>,
}

// ── Settings → Git Actions wire types ────────────────────────────

/// Wire mirror of the client Git action scripts view.
/// Snapshot of both LLM scripts the Settings → Git Actions page edits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitActionScriptsView {
    pub commit_script: String,
    pub commit_using_default: bool,
    pub pr_script: String,
    pub pr_using_default: bool,
}

// ── Settings → Keybindings wire types ────────────────────────────

/// Wire mirror of the client shortcut settings row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutSettingsRow {
    /// Stable kebab-case action id (`new-task`, `cycle-projects`,
    /// etc.). Round-trips through [`Control::SetShortcutBinding`] and
    /// [`Control::ResetShortcutBinding`].
    pub id: String,
    pub label: String,
    /// Current binding string, e.g. `"cmd-shift-]"`. Empty when the
    /// action is intentionally cleared.
    pub current_binding: String,
    pub default_binding: String,
}

/// Wire mirror of the client shortcut settings view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutSettingsView {
    pub actions: Vec<ShortcutSettingsRow>,
}

// ── Settings → MCP wire types ────────────────────────────────────

/// Wire mirror of the client MCP source DTO.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpSourceDto {
    Catalog,
    Custom,
    BuiltInDaemon,
}

/// Wire mirror of the client MCP transport-kind DTO.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransportKindDto {
    Stdio,
    Http,
}

/// Wire mirror of the client MCP server DTO. One row of
/// the Settings → MCP page's registry section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDto {
    pub id: String,
    pub label: String,
    pub source: McpSourceDto,
    pub transport_kind: McpTransportKindDto,
    /// Provider ids (kebab-case: `claude-code`, `cursor-agent`, ...).
    pub enabled_for: Vec<String>,
}

/// Wire mirror of the client MCP catalog-entry DTO. One
/// row of the Settings → MCP page's catalog section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCatalogEntryDto {
    pub id: String,
    pub label: String,
    pub description: String,
    pub docs_url: String,
}

/// Wire mirror of the client MCP settings view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSettingsView {
    pub catalog_entries: Vec<McpCatalogEntryDto>,
    pub registry_entries: Vec<McpServerDto>,
    /// Providers whose last sync failed — UI tints their toggle red.
    /// Empty when there's no recorded sync error.
    pub sync_error_provider_ids: Vec<String>,
}

/// Lifecycle of a terminal restore attempt at desktop boot — emitted
/// to the wire so mobile clients can render the same "starting / ready
/// / failed" pip the desktop sidebar shows. Re-exported from
/// `another_one_core::agents` for desktop-side callers that already
/// reach for it through core.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalRestoreStatus {
    #[default]
    NotStarted,
    Launching,
    Ready,
    Failed,
}

// ── Terminal frames (daemon-canonical Term, design 01) ──────────
//
// Wire types backing the move of `alacritty_terminal::Term` ownership
// into the daemon. Phase 1 declares these types and their serde
// shape; no daemon code produces them yet (see
// `docs/designs/01-daemon-canonical-terminal.md` Phase 2+ for the
// emission path). Round-trip tests at the bottom of this module
// guarantee the wire shape stays stable while the producer is built.
//
// JSON is the only encoding the wire carries today; encoding a full
// grid as JSON is verbose but acceptable while no producer exists.
// Phase 8 of the design swaps `TerminalFrame::Full` to a packed
// encoding once iroh bandwidth or `Full` allocator pressure is
// measured.

/// One terminal cell. Mirrors the subset of
/// `alacritty_terminal::term::cell::Cell` the renderer needs;
/// purposely independent of alacritty's internal types so the wire
/// shape doesn't track upstream refactors.
///
/// All optional fields use `#[serde(default)]` so older snapshots
/// (without `underline_color`, `hyperlink`, or `zero_width`) decode
/// into the wire-default values rather than failing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridCell {
    /// The cell's primary character. Wide characters (CJK, etc.)
    /// occupy a `GridCell` plus one trailing spacer neighbour with
    /// `flags & WIDE_CHAR_SPACER` set; renderers skip spacers.
    pub ch: char,
    pub fg: GridColor,
    pub bg: GridColor,
    pub flags: GridCellFlags,
    /// Per-cell underline colour (SGR 58 / `4:N`). `Default` means
    /// "use the foreground colour", matching alacritty's
    /// `cell.underline_color()` returning `None`.
    #[serde(default = "GridColor::default_for_underline", skip_serializing_if = "GridColor::is_default")]
    pub underline_color: GridColor,
    /// OSC 8 hyperlink target. `None` when the cell isn't part of a
    /// hyperlinked run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hyperlink: Option<String>,
    /// Combining / zero-width characters that follow `ch` and render
    /// over the same cell (combining marks, ZWJ-joined emoji, etc.).
    /// Empty when there are no combiners; non-empty when alacritty's
    /// `cell.zerowidth()` returned a non-empty slice for this cell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zero_width: Vec<char>,
}

impl Default for GridCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: GridColor::Default,
            bg: GridColor::Default,
            flags: GridCellFlags::empty(),
            underline_color: GridColor::Default,
            hyperlink: None,
            zero_width: Vec::new(),
        }
    }
}

/// Either a 24-bit RGB triple, an indexed palette entry, or the
/// renderer's default. Named alacritty colors are resolved against
/// the active palette daemon-side before serializing so the wire
/// stays renderer-independent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GridColor {
    /// Renderer's default foreground / background. Distinct from
    /// indexed `0` because the renderer can theme it independently.
    Default,
    /// 24-bit truecolor.
    Rgb { r: u8, g: u8, b: u8 },
    /// Indexed palette colour; renderer maps to its active palette.
    Indexed { index: u8 },
}

impl GridColor {
    /// `serde(default = ...)` hook. Default underline colour is
    /// `Default`, which the renderer interprets as "track the
    /// foreground colour".
    pub const fn default_for_underline() -> Self {
        GridColor::Default
    }

    /// `serde(skip_serializing_if = ...)` hook. Lets the wire omit
    /// the field when it carries the wire-default value.
    pub const fn is_default(&self) -> bool {
        matches!(self, GridColor::Default)
    }
}

/// Per-cell rendering attributes. Bitfield kept dep-light (no
/// `bitflags!` crate): callers use the associated `u16` constants
/// with `contains` / `insert`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GridCellFlags(pub u16);

impl GridCellFlags {
    pub const INVERSE: u16 = 1 << 0;
    pub const BOLD: u16 = 1 << 1;
    pub const ITALIC: u16 = 1 << 2;
    pub const UNDERLINE: u16 = 1 << 3;
    pub const WRAPLINE: u16 = 1 << 4;
    pub const WIDE_CHAR: u16 = 1 << 5;
    pub const WIDE_CHAR_SPACER: u16 = 1 << 6;
    pub const DIM: u16 = 1 << 7;
    pub const HIDDEN: u16 = 1 << 8;
    pub const STRIKEOUT: u16 = 1 << 9;
    pub const LEADING_WIDE_CHAR_SPACER: u16 = 1 << 10;
    pub const DOUBLE_UNDERLINE: u16 = 1 << 11;
    pub const UNDERCURL: u16 = 1 << 12;
    pub const DOTTED_UNDERLINE: u16 = 1 << 13;
    pub const DASHED_UNDERLINE: u16 = 1 << 14;

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, bits: u16) -> bool {
        (self.0 & bits) == bits
    }

    pub fn insert(&mut self, bits: u16) {
        self.0 |= bits;
    }
}

/// One grid row. Producers may emit fewer cells than the snapshot's
/// `cols` (trailing default cells trimmed); renderers pad to `cols`
/// with `GridCell::default()`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridRow {
    pub cells: Vec<GridCell>,
}

/// A full visible-viewport snapshot plus a small backbuffer. Pushed
/// inside `TerminalFrame::Full`, used on subscription start and on
/// `seq`-gap recovery.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridSnapshot {
    pub cols: u16,
    pub rows: u16,
    /// Visible rows, top-to-bottom. Length equals `rows`.
    pub viewport: Vec<GridRow>,
    /// Recent scrollback rows, oldest-first, bounded by
    /// `2 × viewport_rows` to give momentum-scroll headroom without
    /// shipping full history (older rows ride
    /// `Control::TerminalReadScrollback`).
    pub backbuffer: Vec<GridRow>,
    pub cursor: CursorState,
    pub mode: ModeFlags,
    /// `0` means "viewing the live screen". Non-zero values are how
    /// many lines above the bottom the viewer is scrolled.
    pub scroll_offset: u32,
    /// Total scrollback rows the daemon currently retains for this
    /// tab (i.e. `term.grid().history_size()`). Lets viewers clamp a
    /// `scroll_offset` they want to advance to without round-tripping
    /// `Control::TerminalReadScrollback` first. `0` for fresh tabs
    /// or tabs whose history has been cleared.
    #[serde(default)]
    pub history_lines: u32,
}

/// One row's contents replacement, addressed by line number from the
/// top of the viewport. Used inside `TerminalFrame::Diff` so a
/// steady-state frame ships only the rows that changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowDelta {
    /// Viewport-relative line index, 0 = top.
    pub line: u16,
    pub cells: Vec<GridCell>,
}

/// Cursor position + presentation. Reported in viewport-relative
/// coordinates so the renderer doesn't need the scroll offset to
/// draw the caret.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorState {
    pub line: u16,
    pub col: u16,
    pub shape: CursorShape,
    pub visible: bool,
}

/// Caret presentation. Blinking variants signal the renderer to
/// drive a blink animation; non-blinking variants stay solid.
///
/// `HollowBlock` mirrors alacritty's outline-only block cursor
/// (rendered when the window is unfocused or DECSCUSR requested it
/// explicitly). Older clients that don't recognise the variant fall
/// back to `Block` via the `try_from` helper.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Beam,
    BlockBlinking,
    UnderlineBlinking,
    BeamBlinking,
    HollowBlock,
    HollowBlockBlinking,
}

/// Terminal mode flags the renderer cares about. Mouse mode bits
/// drive whether the input layer forwards mouse events to the PTY
/// (mouse-protocol) or treats them as native (selection, link
/// click).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeFlags {
    pub alt_screen: bool,
    pub bracketed_paste: bool,
    pub mouse_motion: bool,
    pub mouse_drag: bool,
    pub mouse_click: bool,
    pub sgr_mouse: bool,
    pub utf8_mouse: bool,
}

/// One frame the daemon pushes to a subscribed viewer. `Full` carries
/// a complete snapshot; `Diff` carries only the rows that changed
/// since the previous frame's `seq`. Wire-additive: today's daemon
/// emits only `Full`; `Diff` is reserved for Phase 8 of design 01
/// (deferred until measurement justifies it). Viewers must accept
/// both and request a fresh `Full` on any `seq` gap they observe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerminalFrame {
    Full {
        /// Monotonic per `(section_id, tab_id)` over the daemon's
        /// uptime. Resets when the Term task is recreated
        /// (PTY relaunch).
        seq: u64,
        /// `Arc` so the daemon's in-memory transport can fan out to
        /// multiple viewers without cloning the grid; iroh / UDS
        /// transports serialize the inner value as if it were owned.
        snapshot: std::sync::Arc<GridSnapshot>,
    },
    Diff {
        /// Must equal `previous_seq + 1`. Any other value (including
        /// a `Full` gap) requires the viewer to re-request a `Full`.
        seq: u64,
        rows_changed: Vec<RowDelta>,
        cursor: Option<CursorState>,
        mode: Option<ModeFlags>,
        scroll_offset: Option<u32>,
        /// Bell rang during this diff window. Renderer surfaces it
        /// (visual flash, dock badge), then the next diff resets to
        /// `false` if untriggered.
        bell: bool,
    },
}

/// Range of historical rows to fetch via
/// `Control::TerminalReadScrollback`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrollbackRange {
    /// Lines above the live screen. `0` = the topmost line of the
    /// live screen, increasing into the past.
    pub start: u32,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalScrollbackReply {
    /// Returned rows oldest-first. May be shorter than
    /// `requested.count` when the request runs off the end of the
    /// daemon's history.
    pub rows: Vec<GridRow>,
    /// What range was actually returned. Use this rather than the
    /// requested range when laying out the rows.
    pub range_actual: ScrollbackRange,
}

/// How a `TerminalSearch` request matches characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalCaseFold {
    /// `aB` matches `aB` only.
    Sensitive,
    /// `aB` matches `Ab`, `aB`, `AB`, `ab`.
    #[default]
    Insensitive,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSearchRequest {
    pub pattern: String,
    /// `false` — `pattern` is a literal substring; `true` — regex
    /// (the daemon picks the regex engine, today: the `regex`
    /// crate).
    pub regex: bool,
    pub case_fold: TerminalCaseFold,
}

/// One match in a `TerminalSearch` reply. Coordinates are in the
/// daemon's live grid frame: `line` is signed because matches in
/// scrollback have negative line numbers (`-1` = first row above
/// the live screen).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridMatch {
    pub line: i64,
    pub start_col: u16,
    pub end_col: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSearchReply {
    pub matches: Vec<GridMatch>,
}

/// ALPN advertised by the daemon and required by every client. Version-
/// suffixed so future protocol breaks can be versioned cleanly
/// (`/1`, `/2`, …).
///
/// `/1` introduced:
///   - `Control::Hello { protocol_version }` — explicit in-band
///     version field so a peer that bypasses ALPN (e.g. via a proxy
///     that strips it) is still rejected with a deterministic close
///     reason rather than blowing up on the first unknown variant.
///   - `request_id` correlation on every Control / WorkerReply
///     envelope.
///   - `WorkerReply::Err { request_id, kind, message }` for
///     uniform per-request failure reporting.
pub const ALPN: &[u8] = b"anotherone/pty/1";

/// In-band protocol version carried in `Control::Hello`. Bumped in
/// lockstep with the ALPN suffix; mismatches close the connection
/// with `anotherone/incompatible-version`.
pub const PROTOCOL_VERSION: u32 = 1;

#[cfg(test)]
mod wire_roundtrip_tests {
    //! Serde round-trip coverage for the wire envelopes + a
    //! representative sample of `Control` / `WorkerReply` variants.
    //!
    //! Philosophy: we can't realistically snapshot-test every
    //! variant (the enum has 40+ arms and grows every sprint) and
    //! serde-derive already guarantees structural faithfulness,
    //! so exhaustive fixtures would just be restating the
    //! derive macro. What we **do** care about is:
    //!
    //!   - the `ControlEnvelope` / `WorkerReplyEnvelope` flatten
    //!     shape (request_id at top level, variant tag inline)
    //!   - back-compat fields (`#[serde(default)]` on optional
    //!     additions like `Hello::pair_token`,
    //!     `ProjectList::repos`)
    //!   - recently-added variants the integration risk is
    //!     highest on: `WorkerReply::AttachDropped` (#53),
    //!     `RepoSummary` / `RepoBranchSummary` (#134).
    //!
    //! Anything stricter than this belongs in the pre-release
    //! schema-snapshot workflow (#52's stretch item).

    use super::*;

    /// Assert a serde-shaped value survives a JSON round-trip
    /// byte-for-byte after re-serialization. Catches drift where
    /// a field's `#[serde(default)]` makes the round-trip fuzzy
    /// (value serializes, decodes to default, re-serializes
    /// different). Compares parsed JSON values rather than raw
    /// strings so field ordering in maps doesn't break the test.
    fn assert_json_roundtrip<T>(value: &T)
    where
        T: serde::Serialize + for<'de> serde::Deserialize<'de>,
    {
        let encoded = serde_json::to_string(value).expect("serialize");
        let decoded: T = serde_json::from_str(&encoded).expect("deserialize");
        let reencoded = serde_json::to_string(&decoded).expect("re-serialize");
        let lhs: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        let rhs: serde_json::Value = serde_json::from_str(&reencoded).unwrap();
        assert_eq!(lhs, rhs, "roundtrip changed JSON shape");
    }

    #[test]
    fn control_envelope_flattens_request_id_at_top_level() {
        let env = ControlEnvelope {
            request_id: 17,
            control: Control::ListProjects,
        };
        let json: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(json["request_id"], 17);
        assert_eq!(json["type"], "list_projects");
        assert_json_roundtrip(&env);
    }

    #[test]
    fn control_hello_roundtrips_with_and_without_pair_token() {
        // With token — freshly-paired client.
        let with = Control::Hello {
            pair_token: Some("abc123".into()),
            protocol_version: PROTOCOL_VERSION,
        };
        assert_json_roundtrip(&with);

        // Without token — re-connecting paired client that
        // already has an entry in the allowlist.
        let without = Control::Hello {
            pair_token: None,
            protocol_version: PROTOCOL_VERSION,
        };
        assert_json_roundtrip(&without);

        // Forward-compat: an older daemon that didn't know about
        // `pair_token` serialized Hello without the field. A
        // current daemon must still decode that as `pair_token: None`.
        let legacy_bytes = format!(r#"{{"type":"hello","protocol_version":{PROTOCOL_VERSION}}}"#);
        let decoded: Control = serde_json::from_str(&legacy_bytes).expect("legacy decode");
        match decoded {
            Control::Hello {
                pair_token: None, ..
            } => {}
            other => panic!("expected Hello with None pair_token, got {other:?}"),
        }
    }

    #[test]
    fn worker_reply_envelope_flattens_and_carries_kind() {
        let env = WorkerReplyEnvelope {
            request_id: 42,
            reply: WorkerReply::ProjectList {
                projects: Vec::new(),
                repos: Vec::new(),
                ui: UiSnapshot::default(),
            },
        };
        let json: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(json["request_id"], 42);
        assert_eq!(json["kind"], "project_list");
        assert_json_roundtrip(&env);
    }

    #[test]
    fn worker_reply_project_list_accepts_legacy_missing_repos() {
        // A daemon built before #134 shipped would emit a
        // ProjectList without the `repos` field. The current
        // `#[serde(default)]` must still decode it cleanly so a
        // mixed-build deployment doesn't error out on connect.
        let legacy = r#"{"kind":"project_list","projects":[],"ui":{}}"#;
        let decoded: WorkerReply = serde_json::from_str(legacy).expect("legacy decode");
        match decoded {
            WorkerReply::ProjectList { repos, ui, .. } => {
                assert!(repos.is_empty(), "repos must default to empty");
                assert!(ui.expanded_repo_ids.is_empty());
            }
            other => panic!("expected ProjectList, got {other:?}"),
        }
    }

    #[test]
    fn repo_summary_roundtrips_with_branches() {
        let repo = RepoSummary {
            id: "repo-a".into(),
            common_dir: Some("/tmp/checkouts/repo-a/.git".into()),
            actions: serde_json::json!([
                {
                    "id": "test",
                    "name": "Test",
                    "icon": "test",
                    "run_on_worktree_create": false,
                    "scope": "project",
                    "kind": { "kind": "shell", "command": "cargo test" }
                }
            ]),
            branch_order: vec!["main".into(), "feat/x".into()],
            branches: vec![
                RepoBranchSummary {
                    name: "main".into(),
                    last_commit_relative: "2 hours ago".into(),
                    is_default: true,
                    ahead_count: 0,
                    behind_count: 0,
                },
                RepoBranchSummary {
                    name: "feat/x".into(),
                    last_commit_relative: "5 minutes ago".into(),
                    is_default: false,
                    ahead_count: 3,
                    behind_count: 1,
                },
            ],
        };
        assert_json_roundtrip(&repo);
    }

    #[test]
    #[allow(deprecated)]
    fn worker_reply_attach_dropped_roundtrips() {
        // Recently added (#53). Pin the on-wire shape so a future
        // rename of the fields doesn't silently break mobile
        // auto-reattach.
        let reply = WorkerReply::AttachDropped {
            section_id: "proj-a:sec-1".into(),
            tab_id: "7".into(),
            reason: "broadcast lagged (128 chunks dropped)".into(),
        };
        let json: serde_json::Value = serde_json::to_value(&reply).unwrap();
        assert_eq!(json["kind"], "attach_dropped");
        assert_eq!(json["section_id"], "proj-a:sec-1");
        assert_eq!(json["tab_id"], "7");
        assert_json_roundtrip(&reply);
    }

    #[test]
    fn ui_snapshot_roundtrips_with_defaults_on_missing_fields() {
        // Every field on UiSnapshot is `#[serde(default)]` so an
        // older daemon that didn't carry a given field still
        // decodes under a current client. Exercise that by feeding
        // an empty JSON object.
        let decoded: UiSnapshot = serde_json::from_str("{}").expect("empty-object decode");
        assert!(decoded.expanded_repo_ids.is_empty());
        assert!(decoded.pinned_task_ids.is_empty());
        assert!(decoded.last_active_section_id.is_none());
        assert_json_roundtrip(&decoded);
    }

    #[test]
    fn push_request_id_sentinel_is_zero() {
        // The daemon uses request_id == 0 for unsolicited pushes
        // and the client-side router keys on it. Freezing the
        // value at the wire layer so nothing can drift it.
        assert_eq!(PUSH_REQUEST_ID, 0);
    }

    // ── Terminal frame wire (design 01) ─────────────────────

    fn small_grid_snapshot() -> GridSnapshot {
        GridSnapshot {
            cols: 3,
            rows: 1,
            viewport: vec![GridRow {
                cells: vec![
                    GridCell {
                        ch: 'h',
                        fg: GridColor::Default,
                        bg: GridColor::Default,
                        flags: GridCellFlags::empty(),
                        underline_color: GridColor::Default,
                        hyperlink: None,
                        zero_width: Vec::new(),
                    },
                    GridCell {
                        ch: 'i',
                        fg: GridColor::Rgb { r: 255, g: 0, b: 0 },
                        bg: GridColor::Indexed { index: 17 },
                        flags: GridCellFlags(GridCellFlags::BOLD | GridCellFlags::UNDERLINE),
                        underline_color: GridColor::Rgb { r: 0, g: 255, b: 0 },
                        hyperlink: Some("https://example.com".into()),
                        zero_width: vec!['\u{0301}'],
                    },
                    GridCell::default(),
                ],
            }],
            backbuffer: Vec::new(),
            cursor: CursorState {
                line: 0,
                col: 2,
                shape: CursorShape::Block,
                visible: true,
            },
            mode: ModeFlags {
                bracketed_paste: true,
                ..ModeFlags::default()
            },
            scroll_offset: 0,
            history_lines: 128,
        }
    }

    #[test]
    fn terminal_frame_full_roundtrips() {
        let frame = TerminalFrame::Full {
            seq: 42,
            snapshot: std::sync::Arc::new(small_grid_snapshot()),
        };
        assert_json_roundtrip(&frame);
    }

    #[test]
    fn terminal_frame_diff_roundtrips_with_optional_fields() {
        // Diff with all optional fields populated.
        let diff = TerminalFrame::Diff {
            seq: 43,
            rows_changed: vec![RowDelta {
                line: 0,
                cells: vec![GridCell {
                    ch: '!',
                    fg: GridColor::Default,
                    bg: GridColor::Default,
                    flags: GridCellFlags::empty(),
                    underline_color: GridColor::Default,
                    hyperlink: None,
                    zero_width: Vec::new(),
                }],
            }],
            cursor: Some(CursorState {
                line: 0,
                col: 1,
                shape: CursorShape::BeamBlinking,
                visible: true,
            }),
            mode: Some(ModeFlags::default()),
            scroll_offset: Some(0),
            bell: true,
        };
        assert_json_roundtrip(&diff);

        // And with every optional field omitted (steady-state
        // bytes-only update with no cursor / mode change).
        let bare = TerminalFrame::Diff {
            seq: 44,
            rows_changed: Vec::new(),
            cursor: None,
            mode: None,
            scroll_offset: None,
            bell: false,
        };
        assert_json_roundtrip(&bare);
    }

    #[test]
    fn terminal_subscribe_omits_since_seq_when_none() {
        let verb = Control::TerminalSubscribe {
            section_id: "proj-a:section-1".into(),
            tab_id: "7".into(),
            max_fps: 60,
            since_seq: None,
        };
        let json = serde_json::to_string(&verb).expect("serialize");
        // `since_seq: None` is `#[serde(default)]` on the field;
        // the round-trip must succeed both with and without the
        // key on the wire so older daemons that emit it as
        // `null` and newer ones that omit it both decode.
        let with_explicit_null =
            r#"{"type":"terminal_subscribe","section_id":"proj-a:section-1","tab_id":"7","max_fps":60,"since_seq":null}"#;
        let _: Control = serde_json::from_str(with_explicit_null).expect("explicit-null decode");
        let _: Control = serde_json::from_str(&json).expect("omit-default decode");
    }

    #[test]
    fn terminal_search_request_roundtrip_covers_case_fold() {
        for case in [TerminalCaseFold::Sensitive, TerminalCaseFold::Insensitive] {
            let req = TerminalSearchRequest {
                pattern: "^claude".into(),
                regex: true,
                case_fold: case,
            };
            assert_json_roundtrip(&req);
        }
    }

    #[test]
    fn worker_reply_terminal_frame_push_roundtrips() {
        let push = WorkerReply::TerminalFrame {
            section_id: "proj-a:section-1".into(),
            tab_id: "7".into(),
            frame: TerminalFrame::Full {
                seq: 1,
                snapshot: std::sync::Arc::new(GridSnapshot::default()),
            },
        };
        assert_json_roundtrip(&push);
    }

    #[test]
    fn grid_color_variants_serialize_with_kind_tag() {
        let value = GridColor::Rgb { r: 1, g: 2, b: 3 };
        let json = serde_json::to_value(value).expect("serialize");
        // Catch drift in the `#[serde(tag = "kind")]` shape — the
        // renderer reads `kind` to pick a branch.
        assert_eq!(json["kind"], "rgb");
        assert_eq!(json["r"], 1);
    }

    #[test]
    fn grid_cell_flags_are_transparent_u16() {
        // `#[serde(transparent)]` on `GridCellFlags` means the wire
        // is a bare number, not `{"0": N}`. Locks the shape so
        // future additions don't accidentally break older clients.
        let flags = GridCellFlags(GridCellFlags::BOLD | GridCellFlags::ITALIC);
        let json = serde_json::to_string(&flags).expect("serialize");
        assert_eq!(json, "6");
        let back: GridCellFlags = serde_json::from_str("6").expect("decode");
        assert_eq!(back, flags);
    }
}
