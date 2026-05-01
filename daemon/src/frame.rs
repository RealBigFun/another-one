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

use another_one_core::agents::TerminalRestoreStatus;
use anyhow::Context;
use serde::{Deserialize, Serialize};

pub const TY_DATA: u8 = 0x00;
pub const TY_CONTROL: u8 = 0x01;
pub const TY_WORKER_REPLY: u8 = 0x02;

/// Reject any frame larger than this. 64 KiB is comfortably more than
/// any real PTY chunk (readers use 4 KiB buffers) or resize JSON payload
/// (~40 bytes), so there is no legitimate reason for a peer to announce
/// a larger frame. Keeping the cap tight limits how much a compromised
/// paired peer can make the daemon allocate per frame.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

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
    /// Legacy resize for the standalone sandbox shell. On embedded
    /// (desktop-hosted) daemons, use [`Control::TabResize`] after
    /// [`Control::AttachTab`] — that routes the resize to the
    /// specific tab's PTY.
    Resize { cols: u16, rows: u16 },
    /// Legacy: ask the daemon to spawn `git_refresh` for a literal
    /// path. Preserved for backward compat with clients built before
    /// the projects/tasks/tabs protocol. New clients call
    /// [`Control::ListProjects`] then [`Control::AttachTab`].
    WatchProject { project_path: String },
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
    AttachTab { section_id: String, tab_id: String },
    /// Stop forwarding PTY bytes for the currently-attached tab.
    /// Idempotent if nothing is attached.
    DetachTab,
    /// Resize the currently-attached tab's PTY. Silently no-ops
    /// when nothing is attached.
    TabResize { cols: u16, rows: u16 },
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
    /// [`WorkerReply::ProjectAdded`] carrying the post-mutation
    /// project snapshot — the issuing client updates its tree from
    /// the reply directly without a follow-up `ListProjects` round
    /// trip (mutator inline-snapshot contract). A path that the
    /// store already knows replies with [`WorkerReply::Err`] of
    /// kind [`ErrKind::Internal`] (this is the rare "user added the
    /// same dir twice" case; not worth a dedicated `err_kind`).
    AddProject { path: String },
    /// Remove a project from the daemon's store by id. Cascades to
    /// the project's tasks + terminal sections via
    /// [`another_one_core::project_store::ProjectStore::remove_project`].
    /// Idempotent — passing an unknown id is a silent no-op on the
    /// store side, but the daemon still replies with
    /// [`WorkerReply::ProjectRemoved`] echoing the id so the issuer
    /// can drop any stale UI rows.
    RemoveProject { project_id: String },
    /// Snapshot of the host's "Open In" config — installed-and-enabled
    /// apps + the user's preferred default. Drives the mobile titlebar
    /// split-button's primary icon + the chevron dropdown. Reading it
    /// remotely is fine (display-only); the actual app launch
    /// (`open_project_in_app`) stays host-local — see the comment on
    /// `connection.dart::openProjectInApp` for why. Reply:
    /// [`WorkerReply::OpenInStateAck`].
    OpenInState,
    /// List the merged project + global custom actions for `project_id`,
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
    /// [`WorkerReply::SubmitNewTaskAck`].
    SubmitNewTask {
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_ids: Vec<String>,
        branch_mode_existing: bool,
        worktree_mode: bool,
    },
    /// Append one agent tab (or plain shell when `agent_id` is
    /// empty) to an existing task section, make it active, and queue
    /// its PTY launch. Reply: [`WorkerReply::AddAgentToSectionAck`].
    AddAgentToSection {
        section_id: String,
        agent_id: String,
    },
    /// Persist the active tab for a section. Does not itself launch
    /// or attach — the client's existing selection/attach path owns
    /// that. Reply: [`WorkerReply::ActivateSectionTabAck`].
    ActivateSectionTab { section_id: String, tab_id: String },
    /// Remove one tab from a section and tear down its live PTY if
    /// present. Reply: [`WorkerReply::CloseSectionTabAck`].
    CloseSectionTab { section_id: String, tab_id: String },
    /// Flip one section tab's `pinned` flag and return its new value.
    /// Reply: [`WorkerReply::ToggleSectionTabPinnedAck`].
    ToggleSectionTabPinned { section_id: String, tab_id: String },
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
    /// **Single-shot Ack** (resolved per ojm.7's bd body): the
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
    /// global copy instead of a project-local one. Reply:
    /// [`WorkerReply::SaveProjectActionAck`].
    SaveProjectAction {
        project_id: String,
        action: ProjectActionWire,
        save_global_copy: bool,
    },
    /// Delete one custom action by id from the project or global
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
    /// store. Reply is [`WorkerReply::TaskCreated`] carrying the
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
    /// daemon-side. Reply is [`WorkerReply::TaskRenamed`] with the
    /// post-rename inline `TaskSummary` and a `changed` flag — an
    /// unknown id or no-op rename returns `changed = false`.
    RenameTask { task_id: String, new_name: String },
    /// Pin or unpin a task. Pinned tasks float to the top of their
    /// project's task list. Reply is [`WorkerReply::TaskPinned`]
    /// with the inline `TaskSummary` and a `changed` flag (idempotent
    /// re-set returns `false`).
    SetTaskPinned { task_id: String, pinned: bool },
    /// Remove a task (and its terminal sections) from the daemon's
    /// store. The on-disk worktree branch is left untouched — same
    /// semantics as the desktop side. Reply is
    /// [`WorkerReply::TaskRemoved`] with `removed = true` if a task
    /// was deleted, `false` for an unknown id (idempotent).
    RemoveTask { project_id: String, task_id: String },
    /// Compute the canonical branch slug for a free-text input.
    /// Powers the Create Branch modal's live `Branch: …` preview.
    /// Pure function — no project state involved. Reply is
    /// [`WorkerReply::SlugifyBranchNameAck`] with the slug.
    SlugifyBranchName { name: String },
    /// Branch names available on `project_id`'s git repo. Powers the
    /// new-task modal's source-branch dropdown. Reply is
    /// [`WorkerReply::ProjectBranchesAck`] with an empty list when
    /// the project id is unknown.
    ReadProjectBranches { project_id: String },
    /// Default branch the new-task modal seeds for `project_id`.
    /// Reply is [`WorkerReply::PrimaryBranchAck`] with `None` when
    /// the project has no current branch (fresh repo).
    PrimaryBranchForProject { project_id: String },
    /// User's preferred default commit action (`"commit"` or
    /// `"commit-and-push"`) for the active project's root repo.
    /// Reply is [`WorkerReply::RepoDefaultCommitActionAck`] with
    /// `None` when no preference has been recorded — UI defaults to
    /// `"commit"` in that case.
    RepoDefaultCommitAction { project_id: String },
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
    /// [`WorkerReply::ToolbarActionOutcomeAck`] carrying the
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
    /// is [`WorkerReply::PullRequestStatusAck`]; `status: None`
    /// covers both "no PR for the branch" and "unknown project".
    /// Hard failures (gh CLI missing, network error) come back as
    /// [`WorkerReply::Err`] instead.
    FindPullRequestStatus { project_id: String },
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
    /// variant is [`WorkerReply::ProjectPullRequestsAck`]; `prs:
    /// None` covers the unknown-project case. gh CLI / auth /
    /// network failures arrive as [`WorkerReply::Err`].
    FindProjectPullRequests {
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
}

// ── Push vs pull contract for state mutations ────────────────────
//
// Foundation task `another-one-ojm.1` locks in the **inline-snapshot
// reply** model (option (a) of the two we considered):
//
//   Domain mutator replies — `ProjectAdded`, `TaskRenamed`,
//   `BranchCreated`, etc. — carry an inline `ProjectSummary`
//   (or scoped projection of it) so the issuing client updates
//   its tree from the reply directly. **No** separate
//   `ListProjects` round-trip is required after a successful
//   mutation.
//
// The rejected alternative (b) was: mutator replies return only an
// Ack, and the daemon pushes a fresh `WorkerReply::ProjectList` with
// `request_id == 0` to every connected client on every state change.
//
// Why (a):
//   - Single round-trip per mutation. Mobile-on-cellular cares.
//   - The mutating client's UI converges first (it gets the snapshot
//     synchronously with the reply). Other clients can still
//     subscribe to a separate push channel later — but adding that
//     subscription is purely additive over (a). Going the other way
//     (a → b) would be a wire break.
//   - Bandwidth: today the desktop is the only mutator and the
//     paired phone is the only other client. Pushing the full
//     `ProjectList` to every connected client on every state change
//     is wasted bytes for the typical 2-peer case. (a) limits the
//     post-mutation traffic to the issuer.
//   - Simpler daemon: no broadcast bookkeeping, no "which client
//     cares about project X" filter logic. Each `Control::*` mutator
//     handler emits one reply and is done.
//
// Domain children (`ojm.2..8`) follow this rule:
//   - Mutator verbs return a `WorkerReply::*` variant whose payload
//     contains the changed entity (or the full project summary if
//     the change cascades). Names like
//     `ProjectAdded { project: ProjectSummary }`,
//     `TaskRenamed { task: TaskSummary }`.
//   - Reader verbs return the projection the caller asked for, no
//     side-effects on other clients.
//   - If a future feature needs cross-client live updates (e.g.
//     "phone follows desktop's commit panel in real time"), it
//     lands as a separate opt-in `Control::Subscribe { topic }`
//     verb that pushes targeted `WorkerReply::*` frames with
//     `request_id == 0`. Today nothing in the GPUI desktop's UX
//     requires that, and YAGNI applies.

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
    /// Response to [`Control::ListProjects`]. Order matches the
    /// desktop sidebar's `project_order`; worktrees of a root are
    /// emitted as their own entries rather than nested children
    /// (the mobile UI can still group them by `repo_id` if it
    /// wants a tree rendering later).
    ProjectList { projects: Vec<ProjectSummary> },
    /// Response to [`Control::AddProject`] on success. Carries the
    /// inline snapshot of the freshly-inserted project so the
    /// issuing client can splice it into its tree without a
    /// follow-up `ListProjects` (see the "Push vs pull" block above
    /// for the contract). On a duplicate path or a `prepare_project`
    /// failure the daemon emits [`WorkerReply::Err`] instead.
    ProjectAdded { project: ProjectSummary },
    /// Response to [`Control::RemoveProject`]. Echoes the id so the
    /// issuer can drop the matching tree row even if its local
    /// cache had already been pruned. Idempotent on the daemon
    /// side: an unknown id still produces this reply rather than an
    /// `Err`.
    ProjectRemoved { project_id: String },
    /// Reply to [`Control::OpenInState`]. `state.enabled_apps` is in
    /// canonical `OpenInAppKind::all()` order; `preferred_app_id`
    /// is `None` when no Open-In app is detected on the host.
    OpenInStateAck { state: OpenInStateWire },
    /// Reply to [`Control::ListProjectActions`]. Empty `actions` is a
    /// valid result (unknown project, or project with no custom
    /// actions configured) — clients render the empty state rather
    /// than treating it as an error.
    ProjectActionsAck { actions: Vec<ProjectActionWire> },
    /// Reply to [`Control::ReadEnabledAgents`]. `view.agents` is in
    /// the canonical `core::agents::AGENTS` order — clients render
    /// without re-sorting.
    EnabledAgentsAck { view: EnabledAgentsViewWire },
    /// Reply to [`Control::SubmitNewTask`]. `section_id` is the
    /// persisted section the caller should focus; its initial tab is
    /// always `"0"`.
    SubmitNewTaskAck { section_id: String },
    /// Reply to [`Control::AddAgentToSection`]. `tab_id` is the
    /// freshly-minted tab that was appended and made active.
    AddAgentToSectionAck { tab_id: String },
    /// Reply to [`Control::ActivateSectionTab`].
    ActivateSectionTabAck,
    /// Reply to [`Control::CloseSectionTab`]. `active_tab_id` is the
    /// section's new active tab after removal, or empty when the
    /// section is now tabless.
    CloseSectionTabAck { active_tab_id: String },
    /// Reply to [`Control::ToggleSectionTabPinned`].
    ToggleSectionTabPinnedAck { pinned: bool },
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
    /// Reply to [`Control::SaveProjectAction`].
    SaveProjectActionAck,
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
    TaskCreated {
        project_id: String,
        task: TaskSummary,
    },
    /// Reply to [`Control::RenameTask`]. `changed` is `false` for an
    /// unknown id or a no-op rename — in that case `task` is the
    /// pre-existing snapshot (or absent if the id was unknown).
    TaskRenamed {
        changed: bool,
        task: Option<TaskSummary>,
    },
    /// Reply to [`Control::SetTaskPinned`]. `changed` is `false` for
    /// an idempotent re-set of the same value, or an unknown id.
    /// `task` is the post-mutation snapshot when the task exists.
    TaskPinned {
        changed: bool,
        task: Option<TaskSummary>,
    },
    /// Reply to [`Control::RemoveTask`]. `removed` is `false` for an
    /// unknown id (idempotent). `project_id` echoes the request so
    /// the issuer can prune the right project subtree without
    /// re-deriving it.
    TaskRemoved {
        project_id: String,
        task_id: String,
        removed: bool,
    },
    /// Reply to [`Control::SlugifyBranchName`].
    SlugifyBranchNameAck { slug: String },
    /// Reply to [`Control::ReadProjectBranches`]. Empty list for
    /// unknown projects.
    ProjectBranchesAck { branches: Vec<String> },
    /// Reply to [`Control::PrimaryBranchForProject`]. `None` when
    /// the project has no current branch yet.
    PrimaryBranchAck { branch: Option<String> },
    /// Reply to [`Control::RepoDefaultCommitAction`]. `action ==
    /// None` means the user hasn't recorded a preference; UI
    /// defaults to `"commit"`.
    RepoDefaultCommitActionAck { action: Option<String> },
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
    ToolbarActionOutcomeAck { outcome: ToolbarActionOutcome },
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
    /// Reply to [`Control::FindPullRequestStatus`]. `status: None`
    /// when the project has no open PR for its current branch (or
    /// the project id is unknown). Mutator-snapshot rules don't
    /// apply — this is a pure read.
    PullRequestStatusAck { status: Option<PullRequestStatus> },
    /// Reply to [`Control::ReadPullRequestChecks`]. Three-state
    /// payload mirrors the GPUI desktop's
    /// `core::git_actions::find_pull_request_checks` contract:
    ///   * `Some(list)` — PR exists, here are its check rows.
    ///   * `None` — no PR for the branch, or unknown project id.
    /// gh CLI / network failures come back as [`WorkerReply::Err`].
    PullRequestChecksAck { checks: Option<Vec<Check>> },
    /// Reply to [`Control::FindProjectPullRequests`]. `prs: None`
    /// covers the unknown-project case; gh CLI / auth / network
    /// failures arrive as [`WorkerReply::Err`].
    ProjectPullRequestsAck {
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
}

/// Wire mirror of the active git-state view and the underlying
/// `core::project_store::ProjectGitState`. Carries the metadata the
/// titlebar's idle-primary-action selection needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// Lossy wire projection of `core::project_store::Task`. Contains
/// enough for the mobile task page to render the tab strip and
/// request an attach; no live PTY state.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// Lossy wire projection of
/// `core::project_store::PersistedTerminalTab`.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// Mirror of `core::project_store::ProjectKind`. Wire-serialised as
/// lowercase strings: `"root"` / `"worktree"`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    Root,
    Worktree,
}

/// Mirror of `core::agents::AgentProviderKind`. Wire-serialised as
/// snake_case: `"claude_code"` / `"codex"` / `"cursor_agent"` etc.
/// `Shell` is the catch-all for tabs launched without an agent
/// provider set (plain PTY).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProvider {
    ClaudeCode,
    CursorAgent,
    Codex,
    Pi,
    Gemini,
    OpenCode,
    Amp,
    RovoDev,
    Forge,
    Shell,
}

/// Wire projection of `another_one_core::open_in::OpenInAppKind`
/// pre-hydrated with the display strings the mobile UI renders. The
/// daemon resolves them once at projection time so the wire payload
/// is one round-trip — mobile never needs to re-derive label/icon
/// from the id.
///
/// Field-for-field compatible with the client DTO shape so adapters
/// can decode the wire payload without a mapping layer per field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenInAppWire {
    /// Stable id matching `OpenInAppKind::id()` — `"cursor"`,
    /// `"zed"`, `"vscode"`, `"file-manager"`.
    pub id: String,
    pub label: String,
    pub description: String,
    pub icon_path: String,
}

/// Wire projection of the host's Open In state.
/// Mobile's titlebar uses `preferred_app_id` for its primary-action
/// icon and `enabled_apps` for the chevron dropdown. Actual app
/// launch stays host-local on the daemon (see `openProjectInApp`'s
/// docstring in `connection.dart`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenInStateWire {
    /// Apps offered in the dropdown, ordered as `OpenInAppKind::all()`
    /// declares them.
    pub enabled_apps: Vec<OpenInAppWire>,
    /// Id of the app the titlebar's primary action launches, or
    /// `None` when no app is enabled at all.
    pub preferred_app_id: Option<String>,
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

/// Reads one frame from an Iroh `RecvStream`. Returns `None` when the
/// peer has cleanly closed the send side.
pub async fn read_frame<R>(recv: &mut R) -> anyhow::Result<Option<(u8, Vec<u8>)>>
where
    R: ReadExactish + Unpin,
{
    let mut header = [0u8; 5];
    match recv.read_exactish(&mut header).await? {
        ReadOutcome::Closed => return Ok(None),
        ReadOutcome::Got => {}
    }
    let ty = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    anyhow::ensure!(
        len <= MAX_FRAME_BYTES,
        "frame too large: {len} bytes (max {MAX_FRAME_BYTES})"
    );
    let mut payload = vec![0u8; len];
    match recv.read_exactish(&mut payload).await? {
        ReadOutcome::Closed => anyhow::bail!("stream ended mid-frame"),
        ReadOutcome::Got => {}
    }
    Ok(Some((ty, payload)))
}

/// Writes one frame to an Iroh `SendStream`.
pub async fn write_frame<W>(send: &mut W, ty: u8, payload: &[u8]) -> anyhow::Result<()>
where
    W: WriteAllAsync + Unpin,
{
    let mut header = [0u8; 5];
    header[0] = ty;
    header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    send.write_all_async(&header)
        .await
        .context("write header")?;
    send.write_all_async(payload)
        .await
        .context("write payload")?;
    Ok(())
}

// Tiny trait-shaped adapters so we can use these helpers with both iroh's
// send/recv streams and any future transport wrapper. Keeps the frame
// module transport-agnostic.

pub enum ReadOutcome {
    Got,
    Closed,
}

pub trait ReadExactish {
    fn read_exactish(
        &mut self,
        buf: &mut [u8],
    ) -> impl std::future::Future<Output = anyhow::Result<ReadOutcome>> + Send;
}

pub trait WriteAllAsync {
    fn write_all_async(
        &mut self,
        data: &[u8],
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}

impl ReadExactish for iroh::endpoint::RecvStream {
    async fn read_exactish(&mut self, buf: &mut [u8]) -> anyhow::Result<ReadOutcome> {
        let mut read = 0;
        while read < buf.len() {
            match self.read(&mut buf[read..]).await {
                Ok(Some(0)) | Ok(None) => {
                    return if read == 0 {
                        Ok(ReadOutcome::Closed)
                    } else {
                        Err(anyhow::anyhow!(
                            "stream closed mid-read after {read} of {} bytes",
                            buf.len()
                        ))
                    };
                }
                Ok(Some(n)) => {
                    read += n;
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(ReadOutcome::Got)
    }
}

impl WriteAllAsync for iroh::endpoint::SendStream {
    async fn write_all_async(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.write_all(data).await.map_err(Into::into)
    }
}
