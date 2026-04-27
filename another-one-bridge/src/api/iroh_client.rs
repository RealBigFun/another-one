//! Iroh client exposed to Dart via flutter_rust_bridge.
//!
//! One `IrohSession` represents a live QUIC connection to a daemon that
//! speaks the `anotherone/pty/1` ALPN. Dart uses:
//!
//!   1. `iroh_connect(endpoint_id)` to dial.
//!   2. `session.send(bytes)` to deliver PTY input.
//!   3. `session.subscribe(sink)` to start receiving PTY output as a stream
//!      of `Vec<u8>` chunks.
//!   4. `session.close()` when finished.
//!
//! All iroh network work runs on a dedicated multi-thread tokio runtime
//! because FRB's default async executor is not a tokio runtime — iroh's
//! UDP sockets and internal actor tasks require tokio specifically, and
//! without this indirection `Endpoint::bind()` hangs forever on Android.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use anyhow::Context;
use flutter_rust_bridge::frb;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::frb_generated::StreamSink;
use iroh::dns::DnsResolver;
use iroh::endpoint::presets;
use iroh::endpoint::{RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey};

/// Where we persist this device's iroh secret key. Set by Dart on app
/// start via [`set_data_dir`] — typically the application-support
/// directory (`getApplicationSupportDirectory()` on Android/iOS).
///
/// Without a stable secret key, each app launch yields a fresh
/// EndpointId, which breaks TOFU pairing — every restart would be
/// treated as a new peer and get rejected by the daemon. Persisting
/// the key keeps the phone's identity stable across app restarts,
/// reinstalls-with-backup, etc.
static DATA_DIR: std::sync::OnceLock<std::sync::Mutex<Option<PathBuf>>> =
    std::sync::OnceLock::new();

fn data_dir_slot() -> &'static std::sync::Mutex<Option<PathBuf>> {
    DATA_DIR.get_or_init(|| std::sync::Mutex::new(None))
}

/// Must match the daemon's ALPN byte string. Bumped to `/1` alongside
/// the introduction of `protocol_version` in `Control::Hello`,
/// `request_id` correlation, and the uniform `WorkerReply::Err`
/// frame. Version-suffixed so a future protocol break can run
/// `/2`-speakers in parallel without a flag day.
const ALPN: &[u8] = b"anotherone/pty/1";

/// In-band protocol version sent inside the first `Control::Hello`.
/// Mirror of `daemon_sandbox::transport_iroh::PROTOCOL_VERSION`.
const PROTOCOL_VERSION: u32 = 1;

// Frame wire format, matching daemon-sandbox/src/frame.rs:
//   [1 byte type][4 bytes BE length][N bytes payload]
const TY_DATA: u8 = 0x00;
const TY_CONTROL: u8 = 0x01;
const TY_WORKER_REPLY: u8 = 0x02;
/// See `daemon-sandbox/src/frame.rs::MAX_FRAME_BYTES` for the rationale;
/// keep this value in lockstep with the daemon's cap.
const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Top-level envelope for every type=1 control frame. Carries
/// `request_id` so the daemon's reply can be correlated against the
/// originating call from a `Map<u64, Completer<WorkerReply>>` on the
/// Dart side. Mirror of `daemon-sandbox/src/frame.rs::ControlEnvelope`.
#[derive(Debug, Clone, serde::Serialize)]
struct ControlEnvelope {
    request_id: u64,
    #[serde(flatten)]
    control: Control,
}

// Note: we deliberately don't define a `WorkerReplyEnvelope` mirror
// here. The recv loop decodes via `serde_json::Value` first so it can
// peek at the discriminator for forwards-compat (unknown variants
// from a newer daemon get logged-and-dropped). That two-stage decode
// already extracts `request_id` and `kind` separately, so a
// `#[serde(flatten)]` envelope struct would be unused weight.

/// Reserved `request_id` for unsolicited daemon → client frames
/// (PTY bytes, future project-tree refresh broadcasts, etc.).
/// Clients must not use `0` as a real request id when issuing calls.
pub(crate) const PUSH_REQUEST_ID: u64 = 0;

/// Messages that can be sent via a type=1 control frame. Extend in lock-step
/// with `daemon-sandbox/src/frame.rs::Control`.
///
/// Serialize-only: the Dart side doesn't need to decode control
/// frames (they're strictly client → daemon today).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Control {
    /// Legacy resize for the standalone sandbox shell. On the embedded
    /// (desktop-hosted) daemon, use [`Control::TabResize`] after
    /// [`Control::AttachTab`] — that routes the resize to the specific
    /// tab's PTY. Kept for backward compat with the smoke-test binary.
    Resize { cols: u16, rows: u16 },
    /// Ask the daemon to send back its current project list as a
    /// [`WorkerReply::ProjectList`] frame. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ListProjects`.
    ListProjects,
    /// Subscribe to the live PTY byte stream for `(section_id, tab_id)`.
    /// The daemon forwards the stream as a series of [`TY_DATA`] frames
    /// until the session closes or another `AttachTab` / `DetachTab`
    /// arrives — at most one attachment per session. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::AttachTab`.
    AttachTab { section_id: String, tab_id: String },
    /// Stop forwarding PTY bytes for the currently-attached tab.
    /// Idempotent if nothing is attached. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::DetachTab`.
    DetachTab,
    /// Resize the currently-attached tab's PTY. Silently no-ops when
    /// nothing is attached. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::TabResize`.
    TabResize { cols: u16, rows: u16 },
    /// Ask the daemon to launch this tab's PTY if it's not already
    /// running. No-op if already live. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::LaunchTab`.
    LaunchTab { section_id: String, tab_id: String },
    /// Add an on-disk project to the daemon's store. Daemon replies
    /// with [`WorkerReply::ProjectAdded`] (post-mutation snapshot)
    /// on success or [`WorkerReply::Err`] on a duplicate / failed
    /// `prepare_project`. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::AddProject`.
    AddProject { path: String },
    /// Remove a project from the daemon's store by id. Daemon
    /// replies with [`WorkerReply::ProjectRemoved`]; idempotent on
    /// unknown ids. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::RemoveProject`.
    RemoveProject { project_id: String },
    /// Snapshot the host's "Open In" config — installed-and-enabled
    /// apps + preferred default. Reply: `WorkerReply::OpenInStateAck`.
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::OpenInState`.
    OpenInState,
    /// List the merged project + global custom actions for `project_id`.
    /// Reply: `WorkerReply::ProjectActionsAck`. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ListProjectActions`.
    ListProjectActions { project_id: String },
    /// Snapshot of agents the user has enabled on this host plus
    /// the preferred default. Reply:
    /// `WorkerReply::EnabledAgentsAck`. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadEnabledAgents`.
    ReadEnabledAgents,
    /// Submit the new-task modal over the shared wire. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::SubmitNewTask`.
    SubmitNewTask {
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_ids: Vec<String>,
        branch_mode_existing: bool,
        worktree_mode: bool,
    },
    /// Append one agent tab (or plain shell when `agent_id` is
    /// empty) to an existing section and queue its PTY launch.
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::AddAgentToSection`.
    AddAgentToSection {
        section_id: String,
        agent_id: String,
    },
    /// Persist the active tab for a section. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ActivateSectionTab`.
    ActivateSectionTab { section_id: String, tab_id: String },
    /// Remove one tab from a section and tear down its live PTY if
    /// present. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::CloseSectionTab`.
    CloseSectionTab { section_id: String, tab_id: String },
    /// Flip one section tab's `pinned` flag. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ToggleSectionTabPinned`.
    ToggleSectionTabPinned { section_id: String, tab_id: String },
    /// Full agent registry (every entry in `core::agents::AGENTS`,
    /// enabled or not) for the Settings → Agents page. Reply:
    /// `WorkerReply::AgentSettingsAck`. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadAgentSettings`.
    ReadAgentSettings,
    /// Snapshot of Settings → Open In on the daemon host. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadOpenInSettings`.
    ReadOpenInSettings,
    /// Toggle one Open-In app's enabled flag. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::SetOpenInAppEnabled`.
    SetOpenInAppEnabled { app_id: String, enabled: bool },
    /// Launch a project in a host-local app on the daemon host.
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::OpenProjectInApp`.
    OpenProjectInApp { project_id: String, app_id: String },
    /// Run one custom action inside `section_id`'s task — appends a
    /// fresh tab + queues its PTY launch. **Single-shot Ack**: the
    /// reply carries only the new tab id; PTY output flows over
    /// the existing `Control::AttachTab` pipeline. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::RunProjectAction`.
    RunProjectAction {
        project_id: String,
        section_id: String,
        action_id: String,
    },
    /// Upsert one custom action. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::SaveProjectAction`.
    SaveProjectAction {
        project_id: String,
        action: crate::api::local_session::ProjectActionDto,
        save_global_copy: bool,
    },
    /// Delete one custom action by id. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::DeleteProjectAction`.
    DeleteProjectAction {
        project_id: String,
        action_id: String,
    },
    /// `another-one-ojm.5` — discard a whole snapshot of changed
    /// files in one round-trip. The caller supplies the current
    /// `changed_files` list so the daemon can batch reverts and do
    /// one final git-state reread. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::DiscardAllChanges`.
    DiscardAllChanges {
        project_id: String,
        files: Vec<crate::api::local_session::ChangedFileDto>,
    },
    /// TOFU handshake — sent as the very first control frame after
    /// connect when this client has never paired with this daemon
    /// before. `pair_token` is the hex nonce parsed from the
    /// `pair=<hex>` query param on the pairing URL.
    /// `protocol_version` is the wire version we speak; the daemon
    /// closes with `anotherone/incompatible-version` on mismatch
    /// (see [`PROTOCOL_VERSION`]). Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::Hello`.
    Hello {
        pair_token: Option<String>,
        protocol_version: u32,
    },
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::CreateWorktreeTask`.
    /// Heavy: the daemon spawns a worker thread for the actual git
    /// worktree creation + project preparation, so the matching
    /// `WorkerReply::TaskCreated` may arrive tens of seconds later.
    CreateWorktreeTask {
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_provider: Option<AgentProvider>,
    },
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::RenameTask`.
    RenameTask { task_id: String, new_name: String },
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::SetTaskPinned`.
    SetTaskPinned { task_id: String, pinned: bool },
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::RemoveTask`.
    RemoveTask { project_id: String, task_id: String },
    /// Compute the canonical branch slug for free-text input.
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::SlugifyBranchName`.
    SlugifyBranchName { name: String },
    /// Branch names available on a project's repo. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadProjectBranches`.
    ReadProjectBranches { project_id: String },
    /// Default branch the new-task modal seeds for a project.
    /// Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::PrimaryBranchForProject`.
    PrimaryBranchForProject { project_id: String },
    /// User's preferred default commit action for a project's root
    /// repo. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::RepoDefaultCommitAction`.
    RepoDefaultCommitAction { project_id: String },
    /// Snapshot the active project's branch metadata — current
    /// branch + ahead/behind counts. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadActiveGitState`.
    ReadActiveGitState { project_id: String },
    /// Working-tree changes for a project. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadChangedFiles`.
    ReadChangedFiles { project_id: String },
    /// Resolve a project's GitHub remote URL. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadProjectGithubUrl`.
    ReadProjectGithubUrl { project_id: String },
    /// Recent commits on a project's current branch. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadRecentCommits`.
    ReadRecentCommits { project_id: String, limit: u32 },
    /// Per-commit file-change list. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadCommitFileChanges`.
    ReadCommitFileChanges {
        project_id: String,
        commit_id: String,
    },
    /// Diff a project's current branch against a target branch.
    /// Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadBranchCompareState`.
    ReadBranchCompareState {
        project_id: String,
        target_branch: String,
    },
    /// Snapshot the resolved branch settings for a project. Mirror
    /// of `daemon-sandbox/src/frame.rs::Control::ReadBranchSettings`.
    ReadBranchSettings { project_id: String },
    /// Update one branch-setting field. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::SetBranchSetting`.
    SetBranchSetting {
        project_id: String,
        field: String,
        branch_name: Option<String>,
    },
    /// `another-one-ojm.5` — stage one changed file. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::StageChangedFile`.
    /// Reply is `WorkerReply::StageChangedFileAck` carrying the
    /// post-mutation `changed_files` snapshot.
    StageChangedFile {
        project_id: String,
        path: String,
        original_path: Option<String>,
    },
    /// `another-one-ojm.5` — unstage one changed file. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::UnstageChangedFile`.
    UnstageChangedFile {
        project_id: String,
        path: String,
        original_path: Option<String>,
    },
    /// `another-one-ojm.5` — `git add -A` on the project root.
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::StageAllChanges`.
    StageAllChanges { project_id: String },
    /// `another-one-ojm.5` — unstage every staged change. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::UnstageAllChanges`.
    UnstageAllChanges { project_id: String },
    /// `another-one-ojm.5` — discard one file's working-tree changes.
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::DiscardChangedFile`.
    DiscardChangedFile {
        project_id: String,
        path: String,
        untracked: bool,
        original_path: Option<String>,
    },
    /// `another-one-ojm.5` — run one of the titlebar git actions.
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::RunToolbarGitAction`.
    RunToolbarGitAction {
        project_id: String,
        action_id: String,
    },
    /// `another-one-ojm.5` — create a branch from HEAD on `project_id`.
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::CreateBranch`.
    CreateBranch {
        project_id: String,
        branch_name: String,
        use_current_task: bool,
        migrate_changes: bool,
    },
    /// `another-one-ojm.5` — spawn a review task for a PR. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::CreateReviewTask`.
    CreateReviewTask {
        project_id: String,
        pull_request_number: u64,
        head_branch: String,
        agent_provider: Option<AgentProvider>,
    },
    /// Resolve the latest pull-request status for `project_id`'s
    /// current branch. Reply variant is
    /// [`WorkerReply::PullRequestStatusAck`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::FindPullRequestStatus`.
    FindPullRequestStatus { project_id: String },
    /// Read CI checks attached to `project_id`'s current PR. Reply
    /// variant is [`WorkerReply::PullRequestChecksAck`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::ReadPullRequestChecks`.
    ReadPullRequestChecks { project_id: String },
    /// Fetch open pull requests for `project_id` filtered by
    /// `filter_index` plus an optional free-text `query`. Reply
    /// variant is [`WorkerReply::ProjectPullRequestsAck`]. Mirror
    /// of `daemon-sandbox/src/frame.rs::Control::FindProjectPullRequests`.
    FindProjectPullRequests {
        project_id: String,
        filter_index: u32,
        query: String,
    },
    // ── Settings → Git Actions (`another-one-ojm.8`) ──────────────
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::ReadGitActionScripts`.
    ReadGitActionScripts,
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::SetGitCommitScript`.
    SetGitCommitScript { script: String },
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::ResetGitCommitScript`.
    ResetGitCommitScript,
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::SetGitPrScript`.
    SetGitPrScript { script: String },
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::ResetGitPrScript`.
    ResetGitPrScript,
    // ── Settings → Keybindings (`another-one-ojm.8`) ──────────────
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::ReadShortcutSettings`.
    ReadShortcutSettings,
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::SetShortcutBinding`.
    SetShortcutBinding { action_id: String, binding: String },
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::ResetShortcutBinding`.
    ResetShortcutBinding { action_id: String },
    // ── Settings → MCP (`another-one-ojm.8`) ──────────────────────
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::ReadMcpSettings`.
    ReadMcpSettings,
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::McpAddFromCatalog`.
    McpAddFromCatalog { catalog_id: String },
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::McpToggle`.
    McpToggle {
        entry_id: String,
        provider_id: String,
        enabled: bool,
    },
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::McpRemove`.
    McpRemove { entry_id: String },
}

/// Daemon → client worker replies (type=2 frame payload, JSON). Mirror
/// of `daemon-sandbox/src/frame.rs::WorkerReply`; keep variants in
/// lockstep with the daemon's schema.
///
/// Each variant is a curated projection of one core worker's reply,
/// not a mechanical derive on the `core::*_service` reply structs.
/// That lets the daemon evolve its internal types freely and makes
/// the public wire schema a deliberate artifact.
///
/// FRB-exposed: passed to Dart as a tagged union inside
/// [`WorkerReplyMessage`] via the `subscribe_worker_replies` stream
/// on [`IrohSession`].
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerReply {
    /// Response to [`Control::ListProjects`]. Order matches the
    /// desktop sidebar. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::ProjectList`.
    ProjectList { projects: Vec<ProjectSummary> },
    /// Inline-snapshot reply to [`Control::AddProject`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::ProjectAdded`.
    ProjectAdded { project: ProjectSummary },
    /// Inline echo of [`Control::RemoveProject`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::ProjectRemoved`.
    ProjectRemoved { project_id: String },
    /// Uniform per-request failure frame. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::Err`. Domain
    /// callers in `ojm.2..8` map this to a Dart-level exception
    /// type so the UI can branch on `kind` without parsing
    /// `message` strings.
    Err {
        message: String,
        #[serde(rename = "err_kind")]
        kind: ErrKind,
    },
    /// Mirror of `daemon-sandbox/src/frame.rs::WorkerReply::TaskCreated`.
    /// Reply to [`Control::CreateWorktreeTask`].
    TaskCreated {
        project_id: String,
        task: TaskSummary,
    },
    /// Mirror of `daemon-sandbox/src/frame.rs::WorkerReply::TaskRenamed`.
    /// Reply to [`Control::RenameTask`].
    TaskRenamed {
        changed: bool,
        task: Option<TaskSummary>,
    },
    /// Mirror of `daemon-sandbox/src/frame.rs::WorkerReply::TaskPinned`.
    /// Reply to [`Control::SetTaskPinned`].
    TaskPinned {
        changed: bool,
        task: Option<TaskSummary>,
    },
    /// Mirror of `daemon-sandbox/src/frame.rs::WorkerReply::TaskRemoved`.
    /// Reply to [`Control::RemoveTask`].
    TaskRemoved {
        project_id: String,
        task_id: String,
        removed: bool,
    },
    /// Reply to [`Control::SlugifyBranchName`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::SlugifyBranchNameAck`.
    SlugifyBranchNameAck { slug: String },
    /// Reply to [`Control::ReadProjectBranches`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::ProjectBranchesAck`.
    ProjectBranchesAck { branches: Vec<String> },
    /// Reply to [`Control::PrimaryBranchForProject`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::PrimaryBranchAck`.
    PrimaryBranchAck { branch: Option<String> },
    /// Reply to [`Control::RepoDefaultCommitAction`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::RepoDefaultCommitActionAck`.
    RepoDefaultCommitActionAck { action: Option<String> },
    /// Reply to [`Control::ReadActiveGitState`]. `state == None`
    /// when the project id is unknown. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::ActiveGitStateAck`.
    ActiveGitStateAck {
        state: Option<crate::api::local_session::ActiveGitStateDto>,
    },
    /// Reply to [`Control::ReadChangedFiles`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::ChangedFilesAck`.
    ChangedFilesAck {
        files: Option<Vec<crate::api::local_session::ChangedFileDto>>,
    },
    /// Reply to [`Control::ReadProjectGithubUrl`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::ProjectGithubUrlAck`.
    ProjectGithubUrlAck { url: Option<String> },
    /// Reply to [`Control::ReadRecentCommits`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::RecentCommitsAck`.
    RecentCommitsAck {
        view: Option<crate::api::local_session::RecentCommitsView>,
    },
    /// Reply to [`Control::ReadCommitFileChanges`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::CommitFileChangesAck`.
    CommitFileChangesAck {
        files: Option<Vec<crate::api::local_session::BranchCompareFileDto>>,
    },
    /// Reply to [`Control::ReadBranchCompareState`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::BranchCompareAck`.
    BranchCompareAck {
        view: Option<crate::api::local_session::BranchCompareView>,
    },
    /// Reply to [`Control::ReadBranchSettings`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::BranchSettingsAck`.
    BranchSettingsAck {
        settings: Option<crate::api::local_session::ResolvedProjectBranchSettingsDto>,
    },
    /// Reply to [`Control::SetBranchSetting`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::SetBranchSettingAck`.
    SetBranchSettingAck { changed: bool },
    /// `another-one-ojm.5` — ack for [`Control::StageChangedFile`].
    /// Mirror of `daemon-sandbox/src/frame.rs::WorkerReply::StageChangedFileAck`.
    /// Carries the post-mutation `changed_files` snapshot inline so
    /// the issuing client refreshes the right-sidebar Changes pane
    /// without a follow-up `ReadChangedFiles` round-trip.
    StageChangedFileAck {
        changed_files: Vec<crate::api::local_session::ChangedFileDto>,
    },
    /// `another-one-ojm.5` — ack for [`Control::UnstageChangedFile`].
    /// Same inline-snapshot semantics as
    /// [`Self::StageChangedFileAck`].
    UnstageChangedFileAck {
        changed_files: Vec<crate::api::local_session::ChangedFileDto>,
    },
    /// `another-one-ojm.5` — ack for [`Control::StageAllChanges`].
    StageAllChangesAck {
        changed_files: Vec<crate::api::local_session::ChangedFileDto>,
    },
    /// `another-one-ojm.5` — ack for [`Control::UnstageAllChanges`].
    UnstageAllChangesAck {
        changed_files: Vec<crate::api::local_session::ChangedFileDto>,
    },
    /// `another-one-ojm.5` — ack for [`Control::DiscardChangedFile`].
    DiscardChangedFileAck {
        changed_files: Vec<crate::api::local_session::ChangedFileDto>,
    },
    /// `another-one-ojm.5` — ack for [`Control::DiscardAllChanges`].
    /// Carries the final post-batch snapshot plus any per-path
    /// failures that occurred while discarding.
    DiscardAllChangesAck {
        changed_files: Vec<crate::api::local_session::ChangedFileDto>,
        failures: Vec<String>,
    },
    /// `another-one-ojm.5` — ack for [`Control::RunToolbarGitAction`].
    /// Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::ToolbarActionOutcomeAck`.
    ToolbarActionOutcomeAck {
        outcome: crate::api::local_session::ToolbarActionOutcomeDto,
    },
    /// `another-one-ojm.5` — ack for [`Control::CreateBranch`]. Mirror
    /// of `daemon-sandbox/src/frame.rs::WorkerReply::CreateBranchAck`.
    /// Carries the post-mutation `projects` snapshot inline so the
    /// issuing client repaints the projects drawer without a follow-
    /// up `ListProjects` round-trip.
    CreateBranchAck {
        section_id: String,
        projects: Vec<ProjectSummary>,
    },
    /// `another-one-ojm.5` — ack for [`Control::CreateReviewTask`].
    /// Same inline-snapshot semantics as
    /// [`Self::CreateBranchAck`].
    CreateReviewTaskAck {
        section_id: String,
        projects: Vec<ProjectSummary>,
    },
    /// Reply to [`Control::FindPullRequestStatus`]. `status: None`
    /// when the project has no PR for its current branch (or the
    /// project id is unknown). Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::PullRequestStatusAck`.
    /// The payload reuses `local_session::PullRequestStatusDto`
    /// directly so FRB produces a single Dart class regardless of
    /// transport.
    PullRequestStatusAck {
        status: Option<crate::api::local_session::PullRequestStatusDto>,
    },
    /// Reply to [`Control::ReadPullRequestChecks`]. Three-state
    /// payload: `Some(list)` = PR exists (list may be empty),
    /// `None` = no PR or unknown project. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::PullRequestChecksAck`.
    /// Reuses `local_session::CheckDto` directly so FRB produces a
    /// single Dart class regardless of transport.
    PullRequestChecksAck {
        checks: Option<Vec<crate::api::local_session::CheckDto>>,
    },
    /// Reply to [`Control::FindProjectPullRequests`]. `prs: None`
    /// covers the unknown-project case. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::ProjectPullRequestsAck`.
    /// Reuses `local_session::ProjectPagePullRequestDto` directly.
    ProjectPullRequestsAck {
        prs: Option<Vec<crate::api::local_session::ProjectPagePullRequestDto>>,
    },
    /// Reply to [`Control::OpenInState`]. Mirror of
    /// `daemon-sandbox/src/frame.rs::WorkerReply::OpenInStateAck`.
    /// Reuses `local_session::OpenInState` directly.
    OpenInStateAck {
        state: crate::api::local_session::OpenInState,
    },
    /// Reply to [`Control::ListProjectActions`]. Reuses
    /// `local_session::ProjectActionDto` directly.
    ProjectActionsAck {
        actions: Vec<crate::api::local_session::ProjectActionDto>,
    },
    /// Reply to [`Control::ReadEnabledAgents`]. Reuses
    /// `local_session::EnabledAgentsView` directly.
    EnabledAgentsAck {
        view: crate::api::local_session::EnabledAgentsView,
    },
    /// Reply to [`Control::SubmitNewTask`]. `section_id` is the task
    /// section the caller should focus; its initial tab is always
    /// `"0"`.
    SubmitNewTaskAck { section_id: String },
    /// Reply to [`Control::AddAgentToSection`]. `tab_id` is the
    /// freshly-minted tab that was appended and made active.
    AddAgentToSectionAck { tab_id: String },
    /// Reply to [`Control::ActivateSectionTab`].
    ActivateSectionTabAck,
    /// Reply to [`Control::CloseSectionTab`]. `active_tab_id` is the
    /// section's new active tab, or empty when the section is now
    /// tabless.
    CloseSectionTabAck { active_tab_id: String },
    /// Reply to [`Control::ToggleSectionTabPinned`].
    ToggleSectionTabPinnedAck { pinned: bool },
    /// Reply to [`Control::ReadAgentSettings`]. Reuses
    /// `local_session::AgentSettingsView` directly.
    AgentSettingsAck {
        view: crate::api::local_session::AgentSettingsView,
    },
    /// Reply to [`Control::ReadOpenInSettings`]. Reuses
    /// `local_session::OpenInSettingsView` directly.
    OpenInSettingsAck {
        view: crate::api::local_session::OpenInSettingsView,
    },
    /// Reply to [`Control::SetOpenInAppEnabled`].
    SetOpenInAppEnabledAck,
    /// Reply to [`Control::OpenProjectInApp`].
    OpenProjectInAppAck,
    /// Reply to [`Control::RunProjectAction`]. Single-shot Ack —
    /// `tab_id` is the freshly-minted uuid for the spawned tab.
    RunProjectActionAck { tab_id: String },
    /// Reply to [`Control::SaveProjectAction`].
    SaveProjectActionAck,
    /// Reply to [`Control::DeleteProjectAction`].
    DeleteProjectActionAck { deleted: bool },
    // ── Settings → Git Actions (`another-one-ojm.8`) ──────────────
    /// Reply to `Control::ReadGitActionScripts`.
    GitActionScriptsAck {
        view: crate::api::local_session::GitActionScriptsView,
    },
    /// Reply to `Control::SetGitCommitScript`.
    SetGitCommitScriptAck { changed: bool },
    /// Reply to `Control::ResetGitCommitScript`.
    ResetGitCommitScriptAck { changed: bool },
    /// Reply to `Control::SetGitPrScript`.
    SetGitPrScriptAck { changed: bool },
    /// Reply to `Control::ResetGitPrScript`.
    ResetGitPrScriptAck { changed: bool },
    // ── Settings → Keybindings (`another-one-ojm.8`) ──────────────
    /// Reply to `Control::ReadShortcutSettings`.
    ShortcutSettingsAck {
        view: crate::api::local_session::ShortcutSettingsView,
    },
    /// Reply to `Control::SetShortcutBinding`.
    SetShortcutBindingAck,
    /// Reply to `Control::ResetShortcutBinding`.
    ResetShortcutBindingAck,
    // ── Settings → MCP (`another-one-ojm.8`) ──────────────────────
    /// Reply to `Control::ReadMcpSettings`.
    McpSettingsAck {
        view: crate::api::local_session::McpSettingsView,
    },
    /// Reply to `Control::McpAddFromCatalog`.
    McpAddFromCatalogAck,
    /// Reply to `Control::McpToggle`.
    McpToggleAck,
    /// Reply to `Control::McpRemove`.
    McpRemoveAck,
}

// Wire-mirror structs that previously lived here (BranchCompareFileWire,
// BranchCompareWire, ResolvedBranchSettingsWire, CommitWire,
// RecentCommitsWire, ChangedFileWire, ToolbarActionOutcome,
// ActiveGitStateWire) were deleted in `another-one-ojm.11`. The
// FRB-side `WorkerReply::*Ack` variants now reference the
// `crate::api::local_session` DTOs directly — same serde shape as the
// daemon-sandbox wire types they decode from, so the wire stream
// deserializes cleanly into a single Dart class per concept (no
// parallel Wire/Dto pair). The daemon-sandbox-side wire types still
// exist in `daemon_sandbox::frame::*Wire` for the daemon's own
// serialization.

/// Mirror of `daemon-sandbox/src/frame.rs::ErrKind`. Wire form is
/// snake_case; the Dart side gets a freezed enum via FRB.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrKind {
    UnknownId,
    Unsupported,
    Unauthorised,
    Internal,
}

/// Pair of `(request_id, reply)` delivered to the Dart `IrohTransport`
/// over the `subscribe_worker_replies` stream. Splitting the
/// request_id out lets the Dart side maintain a
/// `Map<int, Completer<WorkerReply>>` keyed by request_id and complete
/// the matching future when the reply arrives, instead of relying on
/// stream-ordering for correlation.
///
/// `request_id == 0` (i.e. [`PUSH_REQUEST_ID`]) marks an unsolicited
/// daemon push that no caller is waiting on — the Dart layer routes
/// those to a separate broadcast subscription rather than the
/// completer table.
#[derive(Debug, Clone)]
pub struct WorkerReplyMessage {
    pub request_id: u64,
    pub reply: WorkerReply,
}

/// Mirror of `daemon-sandbox/src/frame.rs::ProjectSummary`. Contains
/// the nested task + tab tree so one `ListProjects` response is enough
/// for the mobile drawer + task page to render without follow-up
/// round-trips.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: ProjectKind,
    pub current_branch: Option<String>,
    pub tasks: Vec<TaskSummary>,
}

/// Mirror of `daemon-sandbox/src/frame.rs::TaskSummary`. Carries the
/// `section_id` half of the compound `TerminalRuntimeKey` used by
/// [`Control::AttachTab`].
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub name: String,
    pub section_id: String,
    pub branch_name: String,
    pub active_tab_id: String,
    pub tabs: Vec<TabSummary>,
    /// Mirrors desktop's `UiState::pinned_task_ids`. Pinned tasks
    /// sort to the top of the mobile projects drawer.
    pub pinned: bool,
    /// "5 minutes ago"-style string for the task's branch last
    /// commit. Empty when the project hasn't been git-refreshed
    /// yet — UI joins this with the branch name (when it differs
    /// from the task name) using `•` and drops empty segments,
    /// mirroring `desktop/src/left_sidebar.rs::branch_row`'s `meta`.
    #[serde(default)]
    pub last_commit_relative: String,
    /// Lines added on the task's working-tree branch since its
    /// merge base. UI renders `+N` in green next to the subtitle
    /// when non-zero. Mirrors GPUI's `branch.lines_added`.
    #[serde(default)]
    pub lines_added: i32,
    /// Lines removed on the task's working-tree branch since its
    /// merge base. UI renders `-N` in red next to `+N`.
    #[serde(default)]
    pub lines_removed: i32,
    /// Project id this task's working directory belongs to —
    /// root project id for plain tasks, the worktree's own project
    /// id for worktree tasks. The titlebar's Open-In / Git Actions
    /// / Custom Actions resolve their working dir through this id,
    /// so a worktree task opens its worktree path (not the root).
    /// Mirrors `core::project_store::Task::target_project_id`.
    #[serde(default)]
    pub target_project_id: String,
}

/// Mirror of `daemon-sandbox/src/frame.rs::TabSummary`. `running`
/// reflects whether the desktop has a live `LiveTerminalRuntime` for
/// this tab right now; `AttachTab` on a non-running tab yields no
/// data.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TabSummary {
    pub id: String,
    pub title: String,
    pub provider: Option<AgentProvider>,
    pub running: bool,
    /// Matches `PersistedTerminalTab::pinned`. Pinned tabs show a
    /// pin glyph on the mobile chip.
    pub pinned: bool,
    /// Matches `PersistedTerminalTab::fixed_title`. When `Some(_)`,
    /// render this instead of [`TabSummary::title`].
    pub fixed_title: Option<String>,
}

/// Mirror of `daemon-sandbox/src/frame.rs::ProjectKind`.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    Root,
    Worktree,
}

/// Mirror of `daemon-sandbox/src/frame.rs::AgentProvider`. Wire form
/// is snake_case: `"claude_code"`, `"cursor_agent"`, `"codex"`, etc.
/// `Shell` is the catch-all for plain-PTY tabs with no agent
/// provider set.
///
/// Both `Serialize` and `Deserialize` are required because the
/// type appears on both sides of the wire — daemon → client in
/// `WorkerReply::ProjectList` (Deserialize) and client → daemon in
/// `Control::CreateWorktreeTask` (Serialize).
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
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

// `PullRequestStatusDto` and `PullRequestStateDto` are reused from
// `crate::api::local_session` directly — they already have
// `Serialize`/`Deserialize` derived (added alongside this verb)
// and produce the same Dart class on the FRB boundary regardless
// of which transport asks for them. Avoiding a parallel mirror
// here keeps a single source of truth for the PR-status shape.

// ── Settings → Git Actions / Keybindings / MCP wire types ────────
//
// Wire payloads introduced by `another-one-ojm.8`. The structs are
// declared on the LocalSession side already (with FRB bindings the
// Dart UI consumes); we re-use them here so the WorkerReply variants
// landing in this module deserialize straight into the FRB-bound
// shape Dart speaks. Two same-named structs across modules would
// silently strip the SseEncode impl off one of them, so the cross-
// module re-use is intentional, not duplication.
//
// Once `another-one-bridge::api::local_session` is deleted per ADR
// `another-one-67l`, these become first-class wire types declared
// here. Until then the dependency direction is iroh_client →
// local_session.

pub use crate::api::local_session::{
    GitActionScriptsView, McpCatalogEntryDto, McpServerDto, McpSettingsView, McpSourceDto,
    McpTransportKindDto, ShortcutSettingsRow, ShortcutSettingsView,
};

/// Writes one frame to the Iroh send stream.
async fn write_frame(send: &mut SendStream, ty: u8, payload: &[u8]) -> anyhow::Result<()> {
    let mut header = [0u8; 5];
    header[0] = ty;
    header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    send.write_all(&header).await?;
    send.write_all(payload).await?;
    Ok(())
}

/// Reads one frame from the Iroh recv stream; returns `None` on clean EOF.
async fn read_frame(recv: &mut RecvStream) -> anyhow::Result<Option<(u8, Vec<u8>)>> {
    let mut header = [0u8; 5];
    let mut read = 0;
    while read < 5 {
        match recv.read(&mut header[read..]).await? {
            Some(0) | None => {
                return if read == 0 {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("stream ended mid-header"))
                };
            }
            Some(n) => read += n,
        }
    }
    let ty = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if len > MAX_FRAME_BYTES {
        anyhow::bail!("frame too large: {len} bytes");
    }
    let mut payload = vec![0u8; len];
    read = 0;
    while read < len {
        match recv.read(&mut payload[read..]).await? {
            Some(0) | None => anyhow::bail!("stream ended mid-payload"),
            Some(n) => read += n,
        }
    }
    Ok(Some((ty, payload)))
}

/// Load the device's persistent iroh secret key from
/// `{DATA_DIR}/iroh_secret_key`, or generate + write one on first
/// run. Fails if Dart hasn't called [`set_data_dir`] yet (meaning
/// we have nowhere safe to persist); in that case the caller is
/// expected to surface a clear error rather than silently falling
/// back to an ephemeral key that would break TOFU on restart.
///
/// `pub(crate)` so the embedded-daemon bootstrap can resolve the
/// device's NodeId at boot time and pre-allowlist it in the daemon's
/// paired-peers file (`another-one-ojm.9` loopback bootstrap), which
/// lets the desktop dial its own daemon over iroh without consuming
/// the pair nonce reserved for actual mobile pairing flows.
pub(crate) fn load_or_create_device_secret_key() -> anyhow::Result<SecretKey> {
    let path = {
        let slot = data_dir_slot().lock().expect("data_dir mutex poisoned");
        slot.clone()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "set_data_dir must be called before iroh_connect — \
                     the app needs a persistent path for the device secret key"
                )
            })?
            .join("iroh_secret_key")
    };
    load_or_create_secret_key_at(&path)
}

fn load_or_create_secret_key_at(path: &Path) -> anyhow::Result<SecretKey> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create data dir {}", parent.display()))?;
    }
    if let Ok(content) = std::fs::read_to_string(path) {
        let trimmed = content.trim();
        let bytes = hex_decode_32(trimmed)
            .with_context(|| format!("parse secret key at {}", path.display()))?;
        return Ok(SecretKey::from_bytes(&bytes));
    }
    let sk = SecretKey::generate();
    let hex = hex_encode_32(&sk.to_bytes());
    std::fs::write(path, format!("{hex}\n"))
        .with_context(|| format!("write secret key to {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    tracing::info!(path = %path.display(), "generated new device iroh secret key");
    Ok(sk)
}

fn hex_encode_32(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn hex_decode_32(s: &str) -> anyhow::Result<[u8; 32]> {
    if s.len() != 64 {
        anyhow::bail!("expected 64 hex chars, got {}", s.len());
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = u8::from_str_radix(&s[i * 2..i * 2 + 1], 16).context("bad hex")?;
        let lo = u8::from_str_radix(&s[i * 2 + 1..i * 2 + 2], 16).context("bad hex")?;
        *byte = (hi << 4) | lo;
    }
    Ok(out)
}

/// Dedicated tokio runtime for all iroh + local-session work. FRB's
/// default async executor is not a tokio runtime, so iroh's network
/// actors never get polled if we run them on the calling task — and
/// `LocalSession`'s subscription forwarders share the same need to
/// keep producing on a real runtime. `pub(crate)` so sibling
/// modules under `api/` can reuse it without each spinning up its
/// own runtime.
pub(crate) fn tokio_rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("another_one_bridge-tokio")
            .build()
            .expect("build tokio runtime")
    })
}

/// Record the application data directory Dart has chosen for us.
/// Must be called before `iroh_connect` so the secret key can be
/// loaded/created there. Safe to call multiple times — last write
/// wins. On Android/iOS, pass
/// `(await path_provider.getApplicationSupportDirectory()).path`.
pub fn set_data_dir(path: String) {
    let mut slot = data_dir_slot().lock().expect("data_dir mutex poisoned");
    *slot = Some(PathBuf::from(path));
}

#[frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
    setup_tracing();
    // Force the runtime to initialize eagerly so first-call latency doesn't
    // include runtime construction.
    let _ = tokio_rt();
}

/// Install a tracing subscriber that routes events to Android's logcat on
/// Android, and to stderr elsewhere. Default filter is modest; override with
/// `RUST_LOG` when debugging (e.g. `RUST_LOG=iroh=debug`).
fn setup_tracing() {
    use tracing_subscriber::{prelude::*, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,another_one_bridge=info,iroh=warn"));

    #[cfg(target_os = "android")]
    let layer = tracing_android::layer("another_one_bridge").expect("tracing-android layer");

    #[cfg(not(target_os = "android"))]
    let layer = tracing_subscriber::fmt::layer();

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init();
}

/// Opaque handle to a live Iroh QUIC session. Dart holds this object and
/// calls methods on it; the actual Iroh state lives in Rust.
#[frb(opaque)]
pub struct IrohSession {
    /// The local endpoint we bound for this session. Closed on Drop.
    _endpoint: Endpoint,
    /// Sends framed messages (ty, payload) from Rust to the send task,
    /// which writes them into the QUIC send stream. `None` means closed.
    send_tx: Mutex<Option<mpsc::Sender<(u8, Vec<u8>)>>>,
    /// Holds the bytes-from-daemon stream until `subscribe()` wires it to a
    /// Dart `StreamSink`. Taken once and moved into the forwarding task.
    incoming_rx: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    /// Holds decoded worker replies (from `TY_WORKER_REPLY` frames) until
    /// `subscribe_worker_replies()` wires it to a Dart sink. Each item is
    /// a `(request_id, reply)` pair so the Dart layer can dispatch via
    /// its `Map<int, Completer<WorkerReply>>` rather than stream order.
    worker_replies_rx: Mutex<Option<mpsc::Receiver<WorkerReplyMessage>>>,
    /// Per-verb pending-reply table for calls that the bridge fully
    /// resolves (decoded JSON → typed DTO → return-to-Dart) without
    /// going through the FRB-bound `WorkerReply` enum.
    ///
    /// The recv loop checks `pending` before attempting to decode the
    /// payload as a `WorkerReply` variant: if a oneshot is registered
    /// for the frame's `request_id`, the raw JSON `Value` is delivered
    /// there and the payload skips the broadcast stream. Per-verb
    /// methods (e.g. [`Self::open_in_state`]) register a oneshot
    /// before sending the control and await the resolved JSON.
    ///
    /// Why split this from `worker_replies_rx`:
    ///   - Keeps the FRB-bound `WorkerReply` enum small (no
    ///     OpenInStateAck / EnabledAgentsAck etc. variants leaking
    ///     into Dart's freezed surface), since most ojm.7 verbs need
    ///     no Dart-side broadcast subscriber — the calling provider
    ///     awaits a single typed reply and discards.
    ///   - Lets per-verb methods return typed DTOs (e.g.
    ///     `Result<OpenInState>`) directly, instead of forcing the
    ///     Dart side to pattern-match on a sealed `WorkerReply`
    ///     subclass per verb.
    ///
    /// `ListProjects` keeps the broadcast path because the Dart UI
    /// fans the result out to multiple listeners (project drawer,
    /// titlebar, status header).
    #[frb(ignore)]
    pending: Arc<StdMutex<PendingTable>>,
    /// Monotonic per-session request id allocator. Starts at 1 because
    /// `0` is reserved for daemon-pushed (unsolicited) frames — see
    /// [`PUSH_REQUEST_ID`]. Wraps at u64::MAX which is effectively
    /// never; even at 1 GHz issuance the wrap takes 500 years.
    next_request_id: AtomicU64,
    /// Closes the underlying connection when invoked.
    closer: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// Map of `request_id` → oneshot waiter for the recv loop to deliver
/// the raw JSON payload of a worker-reply frame. Populated by
/// per-verb FRB methods before sending; drained by the recv loop on
/// matching reply or by `IrohSession::close` when the session ends.
///
/// `#[frb(ignore)]` keeps this off the FRB-bound surface — it's an
/// internal book-keeping type with no Dart equivalent. Without the
/// ignore, FRB picks it up via the `pub`-walked iroh_client module
/// and tries to emit Dart bindings for `HashMap<u64, oneshot::Sender>`,
/// which it can't represent.
#[frb(ignore)]
#[derive(Default)]
struct PendingTable {
    waiters: HashMap<u64, oneshot::Sender<serde_json::Value>>,
}

/// Dial a daemon's Iroh endpoint by its public `EndpointId`.
///
/// At least one of `direct_addrs` or `relay_urls` must be non-empty — the
/// sandbox has no address-lookup service, so we can't discover how to reach
/// the daemon on our own. The daemon's ticket file prints both; pass them
/// through. When both are given iroh prefers the direct path and falls
/// back to the relay if hole-punching fails (the typical mobile-cellular
/// path).
pub async fn iroh_connect(
    endpoint_id: String,
    direct_addrs: Vec<String>,
    relay_urls: Vec<String>,
    pair_token: Option<String>,
) -> anyhow::Result<IrohSession> {
    tokio_rt()
        .spawn(async move {
            iroh_connect_inner(endpoint_id, direct_addrs, relay_urls, pair_token).await
        })
        .await
        .map_err(|e| anyhow::anyhow!("connect task panicked: {e}"))?
}

async fn iroh_connect_inner(
    endpoint_id: String,
    direct_addrs: Vec<String>,
    relay_urls: Vec<String>,
    pair_token: Option<String>,
) -> anyhow::Result<IrohSession> {
    tracing::info!(
        "iroh_connect: id={} direct={:?} relays={:?}",
        endpoint_id,
        direct_addrs,
        relay_urls,
    );

    let id: EndpointId = endpoint_id.trim().parse().context("invalid EndpointId")?;

    // Parse direct addresses eagerly so bad input surfaces before bind.
    let parsed_addrs: Vec<std::net::SocketAddr> = direct_addrs
        .iter()
        .map(|s| {
            s.parse::<std::net::SocketAddr>()
                .map_err(|e| anyhow::anyhow!("bad direct addr {s:?}: {e}"))
        })
        .collect::<anyhow::Result<_>>()?;
    let parsed_relays: Vec<RelayUrl> = relay_urls
        .iter()
        .map(|s| {
            s.parse::<RelayUrl>()
                .map_err(|e| anyhow::anyhow!("bad relay url {s:?}: {e}"))
        })
        .collect::<anyhow::Result<_>>()?;
    if parsed_addrs.is_empty() && parsed_relays.is_empty() {
        return Err(anyhow::anyhow!(
            "at least one direct address or relay URL is required \
             (sandbox has no address lookup)"
        ));
    }

    // Relay mode: if the caller gave us a relay URL, honour it (N0's dev
    // mesh lives behind `RelayMode::Default`). Otherwise stay disabled for
    // the LAN-only direct path.
    let relay_mode = if parsed_relays.is_empty() {
        RelayMode::Disabled
    } else {
        RelayMode::Default
    };
    tracing::info!(
        "iroh_connect: binding (Minimal preset, relay_mode={:?}, explicit DNS)",
        relay_mode,
    );
    // Android gotcha: `DnsResolver::default()` calls `with_system_defaults()`
    // which tries to read `/etc/resolv.conf`. iroh's own doc notes this "does
    // not work at least on some Androids" and says it falls back to Google
    // DNS — but in practice on the emulator the read hangs long enough to
    // stall bind(). We explicitly hand iroh a resolver so it skips system
    // detection entirely.
    //
    // Default is Cloudflare (`1.1.1.1:53`) rather than Google (`8.8.8.8:53`)
    // so every user's daemon lookups don't default to a Google-operated
    // resolver. Override with the `ANOTHERONE_DNS` env var if the user
    // wants a different provider — any `<ip>:<port>` string parseable as a
    // `SocketAddr` works. Fall back to the default silently on parse error
    // so a fat-fingered env var doesn't brick the mobile app.
    let dns_addr: std::net::SocketAddr = std::env::var("ANOTHERONE_DNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "1.1.1.1:53".parse().expect("static ipv4 socket addr"));
    tracing::info!(%dns_addr, "iroh_connect: using configured DNS resolver");
    let dns = DnsResolver::with_nameserver(dns_addr);
    // Persist the client's iroh identity so the EndpointId stays stable
    // across app restarts. Without this, TOFU pairing breaks every
    // time the user reopens the app.
    let secret_key =
        load_or_create_device_secret_key().context("load/create device iroh secret key")?;
    let endpoint = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        Endpoint::builder(presets::Minimal)
            .secret_key(secret_key)
            .relay_mode(relay_mode)
            .alpns(vec![])
            .dns_resolver(dns)
            .bind(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("bind timed out after 15s (Minimal+DNS)"))?
    .context("bind client endpoint")?;
    tracing::info!("iroh_connect: endpoint bound, dialing {}", id);

    let mut addr = EndpointAddr::new(id);
    for sa in &parsed_addrs {
        addr = addr.with_ip_addr(*sa);
    }
    for url in parsed_relays {
        addr = addr.with_relay_url(url);
    }

    let conn = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        endpoint.connect(addr, ALPN),
    )
    .await
    .map_err(|_| anyhow::anyhow!("connect timed out after 10s"))?
    .context("connect to daemon")?;
    tracing::info!("iroh_connect: connected");

    let (mut send, mut recv) = conn.open_bi().await.context("open_bi")?;
    tracing::info!("iroh_connect: opened bidi stream");

    // Outbound pipe: Dart → channel → framed writes to Iroh send stream.
    // Channel items are already-framed (ty, payload) pairs so the writer
    // task doesn't need to know the protocol.
    let (send_tx, mut send_rx) = mpsc::channel::<(u8, Vec<u8>)>(64);
    // First frame MUST be `Control::Hello` so the daemon can complete
    // TOFU pairing before any other control / data frames arrive. The
    // daemon ignores Hello from already-paired peers, so sending it
    // unconditionally is safe. We send via the mpsc so ordering is
    // preserved with whatever the Dart layer sends next.
    //
    // Hello uses `request_id == PUSH_REQUEST_ID` (= 0) because no
    // caller is waiting on a reply — the daemon either accepts and
    // stays silent or closes the connection with
    // `anotherone/incompatible-version` / `anotherone/unpaired`.
    // Reserving 0 for "no reply expected" lets the Dart layer treat
    // any worker_reply with id 0 as a daemon push rather than a
    // dispatch-to-completer event.
    let hello_payload = serde_json::to_vec(&ControlEnvelope {
        request_id: PUSH_REQUEST_ID,
        control: Control::Hello {
            pair_token,
            protocol_version: PROTOCOL_VERSION,
        },
    })
    .context("encode hello")?;
    send_tx
        .send((TY_CONTROL, hello_payload))
        .await
        .map_err(|_| anyhow::anyhow!("send channel closed before hello"))?;
    tokio_rt().spawn(async move {
        while let Some((ty, payload)) = send_rx.recv().await {
            if let Err(e) = write_frame(&mut send, ty, &payload).await {
                tracing::debug!(error = %e, "iroh frame write failed");
                break;
            }
        }
        let _ = send.finish();
    });

    // Inbound pipe: framed reads from Iroh → per-frame-type channel → Dart
    // (once subscribed). Type=0 frames carry PTY output; type=2 frames carry
    // JSON-encoded `WorkerReply`s. Type=1 (server→client control) is
    // reserved for future use. Unknown types are logged and dropped so older
    // clients stay forwards-compatible as the daemon adds variants.
    let (incoming_tx, incoming_rx) = mpsc::channel::<Vec<u8>>(128);
    let (worker_replies_tx, worker_replies_rx) = mpsc::channel::<WorkerReplyMessage>(64);
    let (close_tx, mut close_rx) = tokio::sync::oneshot::channel::<()>();
    let pending = Arc::new(StdMutex::new(PendingTable::default()));
    let pending_for_recv = pending.clone();
    let conn_for_close = conn.clone();
    tokio_rt().spawn(async move {
        loop {
            tokio::select! {
                _ = &mut close_rx => break,
                frame = read_frame(&mut recv) => match frame {
                    Ok(Some((TY_DATA, payload))) => {
                        if incoming_tx.send(payload).await.is_err() {
                            break;
                        }
                    }
                    Ok(Some((TY_WORKER_REPLY, payload))) => {
                        // Two-stage decode for forwards-compat: parse
                        // as a generic JSON value first so we can
                        // peek at the `kind` discriminator. If the
                        // kind is one the current build knows, do
                        // the strict decode; otherwise log + drop so
                        // a future daemon variant (TaskStateChanged
                        // etc.) doesn't blow up an older client.
                        //
                        // The envelope is `#[serde(flatten)]`-ed onto
                        // `WorkerReply`, so the on-wire shape is one
                        // flat object: `{"request_id": N, "kind": "...", ...}`.
                        // We pluck `request_id` from the generic
                        // `Value` first (default 0 = push frame) then
                        // try the strict variant decode.
                        match serde_json::from_slice::<serde_json::Value>(&payload) {
                            Ok(value) => {
                                let request_id = value
                                    .get("request_id")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(PUSH_REQUEST_ID);
                                // First check the per-verb pending
                                // table. Per-verb FRB methods (e.g.
                                // `open_in_state`) register a oneshot
                                // keyed on the request_id they sent.
                                // If matched, the raw JSON goes
                                // straight to the awaiter — skip the
                                // broadcast path entirely so the
                                // Dart-bound `WorkerReply` enum
                                // doesn't have to grow per-verb
                                // variants nobody else listens to.
                                if request_id != PUSH_REQUEST_ID {
                                    let waiter = pending_for_recv
                                        .lock()
                                        .unwrap_or_else(|p| p.into_inner())
                                        .waiters
                                        .remove(&request_id);
                                    if let Some(tx) = waiter {
                                        // oneshot send fails only if the
                                        // receiver was dropped (caller
                                        // cancelled / session closed). Drop
                                        // silently — the caller already
                                        // unblocked with an error of its own.
                                        let _ = tx.send(value);
                                        continue;
                                    }
                                }
                                // Clone the discriminator before the
                                // strict decode moves `value` —
                                // otherwise we'd have no way to log
                                // the unknown variant name.
                                let kind = value
                                    .get("kind")
                                    .and_then(|k| k.as_str())
                                    .unwrap_or("<missing>")
                                    .to_string();
                                match serde_json::from_value::<WorkerReply>(value) {
                                    Ok(reply) => {
                                        let message = WorkerReplyMessage { request_id, reply };
                                        // try_send, not send().await — this
                                        // recv task also drives the PTY
                                        // stream which *does* want
                                        // backpressure; we can't let a
                                        // stuck worker_replies consumer
                                        // stall PTY bytes.
                                        use tokio::sync::mpsc::error::TrySendError;
                                        match worker_replies_tx.try_send(message) {
                                            Ok(()) => {}
                                            Err(TrySendError::Full(_)) => {
                                                tracing::debug!("worker_replies channel full; dropping frame");
                                            }
                                            Err(TrySendError::Closed(_)) => {
                                                tracing::debug!("worker_replies channel closed; dropping frame");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::debug!(
                                            kind,
                                            request_id,
                                            error = %e,
                                            "unknown/unsupported worker_reply variant; dropping (daemon is newer than client?)"
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    payload_bytes = payload.len(),
                                    "failed to parse worker_reply frame as JSON"
                                );
                            }
                        }
                    }
                    Ok(Some((ty, _))) => {
                        tracing::debug!(frame_type = ty, "unhandled iroh frame type");
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(error = %e, "iroh frame read failed");
                        break;
                    }
                },
            }
        }
        conn_for_close.close(0u8.into(), b"close");
    });

    Ok(IrohSession {
        _endpoint: endpoint,
        send_tx: Mutex::new(Some(send_tx)),
        incoming_rx: Mutex::new(Some(incoming_rx)),
        worker_replies_rx: Mutex::new(Some(worker_replies_rx)),
        pending,
        // Start at 1: id 0 is reserved for daemon-pushed frames so
        // the Dart layer can distinguish "reply to my call" from
        // "unsolicited update" without inspecting variant kinds.
        next_request_id: AtomicU64::new(1),
        closer: Mutex::new(Some(close_tx)),
    })
}

impl IrohSession {
    /// Send raw bytes to the daemon (will be written into the PTY's stdin).
    pub async fn send(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.send_frame(TY_DATA, bytes).await
    }

    /// Allocate the next per-session request id. Dart calls this
    /// before issuing a control verb so it can register a `Completer`
    /// in its dispatch map keyed by the same id. Strictly-monotonic
    /// across the session; never returns 0 (reserved for push
    /// frames — see [`PUSH_REQUEST_ID`]).
    pub fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Request a PTY resize on the daemon's end. Goes through the same
    /// stream as data, multiplexed by frame type. The legacy `Resize`
    /// variant carries no data the client needs to wait on, so it
    /// uses a fresh request_id but no caller correlates against it.
    pub async fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.send_control(self.next_request_id(), Control::Resize { cols, rows })
            .await
    }

    /// Ask the daemon to send back its current project list as a
    /// [`WorkerReply::ProjectList`] frame. The reply arrives on
    /// `subscribe_worker_replies` with a matching `request_id`;
    /// today the Dart wrapper still consumes by stream order, so the
    /// id round-trips but isn't dispatched-on yet — domain tasks
    /// (`another-one-ojm.2..8`) are responsible for migrating each
    /// verb to the completer-table model.
    pub async fn list_projects(&self) -> anyhow::Result<()> {
        self.send_control(self.next_request_id(), Control::ListProjects)
            .await
    }

    /// Subscribe this session to the live PTY byte stream for
    /// `(section_id, tab_id)`. The daemon will forward the attached
    /// tab's output as [`TY_DATA`] frames on the existing `subscribe`
    /// sink. At most one attachment per session — re-issuing replaces
    /// the previous one. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::AttachTab`.
    pub async fn attach_tab(&self, section_id: String, tab_id: String) -> anyhow::Result<()> {
        self.send_control(
            self.next_request_id(),
            Control::AttachTab { section_id, tab_id },
        )
        .await
    }

    /// Stop forwarding PTY bytes for the currently-attached tab.
    /// Idempotent if nothing is attached. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::DetachTab`.
    pub async fn detach_tab(&self) -> anyhow::Result<()> {
        self.send_control(self.next_request_id(), Control::DetachTab)
            .await
    }

    /// Resize the currently-attached tab's PTY. Silently no-ops on
    /// the daemon when nothing is attached. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::TabResize`.
    pub async fn tab_resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.send_control(self.next_request_id(), Control::TabResize { cols, rows })
            .await
    }

    /// Ask the daemon to launch the tab's PTY if it isn't already
    /// live. No-op on the daemon side if the tab is already running.
    /// After this, a subsequent `attach_tab` will receive bytes.
    pub async fn launch_tab(&self, section_id: String, tab_id: String) -> anyhow::Result<()> {
        self.send_control(
            self.next_request_id(),
            Control::LaunchTab { section_id, tab_id },
        )
        .await
    }

    // ── Project mutation (another-one-ojm.2) ──────────────────────

    /// Issue a [`Control::AddProject`] under a Dart-allocated
    /// `request_id` so the Dart layer can register a `Completer`
    /// keyed by the same id before the frame goes out. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::AddProject`.
    ///
    /// Unlike the legacy fire-and-forget verbs above, mutator verbs
    /// reply with an inline snapshot the issuer needs (see
    /// `WorkerReply::ProjectAdded`), so the request id has to be
    /// known *before* `send` runs — the Dart caller calls
    /// [`next_request_id`] first, registers its completer, then
    /// invokes this method with the id it allocated. That ordering
    /// guarantees the reply can never beat the completer-table
    /// insertion.
    pub async fn add_project(&self, request_id: u64, path: String) -> anyhow::Result<()> {
        self.send_control(request_id, Control::AddProject { path })
            .await
    }

    /// Issue a [`Control::RemoveProject`] under a Dart-allocated
    /// `request_id`. Same allocation contract as [`add_project`].
    /// Mirror of `daemon-sandbox/src/frame.rs::Control::RemoveProject`.
    pub async fn remove_project(&self, request_id: u64, project_id: String) -> anyhow::Result<()> {
        self.send_control(request_id, Control::RemoveProject { project_id })
            .await
    }

    // ── Task mutation (another-one-ojm.3) ─────────────────────────

    /// Issue a [`Control::CreateWorktreeTask`] under `request_id`.
    /// The Dart side allocates the id (via [`next_request_id`]) and
    /// registers a completer keyed by the same id before calling
    /// here, so the matching `WorkerReply::TaskCreated` (or `Err`)
    /// is dispatched into the awaiting future. Mirror of
    /// `LocalSession::create_worktree_task`.
    pub async fn create_worktree_task(
        &self,
        request_id: u64,
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_provider: Option<AgentProvider>,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::CreateWorktreeTask {
                project_id,
                task_name,
                source_branch,
                agent_provider,
            },
        )
        .await
    }

    /// Issue a [`Control::RenameTask`] under `request_id`. Mirror of
    /// `LocalSession::rename_task`.
    pub async fn rename_task(
        &self,
        request_id: u64,
        task_id: String,
        new_name: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::RenameTask { task_id, new_name })
            .await
    }

    /// Issue a [`Control::SetTaskPinned`] under `request_id`. Mirror
    /// of `LocalSession::set_task_pinned`.
    pub async fn set_task_pinned(
        &self,
        request_id: u64,
        task_id: String,
        pinned: bool,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::SetTaskPinned { task_id, pinned })
            .await
    }

    /// Issue a [`Control::RemoveTask`] under `request_id`. Mirror of
    /// `LocalSession::remove_task`.
    pub async fn remove_task(
        &self,
        request_id: u64,
        project_id: String,
        task_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::RemoveTask {
                project_id,
                task_id,
            },
        )
        .await
    }

    // ── Git state read verbs (`another-one-ojm.4`) ─────────────────
    //
    // Each method takes a Dart-allocated `request_id` and sends the
    // matching `Control::*` frame. The Dart caller registers a
    // completer keyed on `request_id` before the call returns; the
    // daemon's reply is dispatched into that completer via the
    // `subscribe_worker_replies` stream.

    /// Issue [`Control::SlugifyBranchName`] for `name`. Pure verb —
    /// no project state involved on the daemon side.
    pub async fn slugify_branch_name(&self, request_id: u64, name: String) -> anyhow::Result<()> {
        self.send_control(request_id, Control::SlugifyBranchName { name })
            .await
    }

    /// Issue [`Control::ReadProjectBranches`] for `project_id`.
    pub async fn read_project_branches(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadProjectBranches { project_id })
            .await
    }

    /// Issue [`Control::PrimaryBranchForProject`] for `project_id`.
    pub async fn primary_branch_for_project(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::PrimaryBranchForProject { project_id })
            .await
    }

    /// Issue [`Control::RepoDefaultCommitAction`] for `project_id`.
    pub async fn repo_default_commit_action(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::RepoDefaultCommitAction { project_id })
            .await
    }

    /// Issue [`Control::ReadActiveGitState`] for `project_id`.
    pub async fn read_active_git_state(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadActiveGitState { project_id })
            .await
    }

    /// Issue [`Control::ReadChangedFiles`] for `project_id`.
    pub async fn read_changed_files(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadChangedFiles { project_id })
            .await
    }

    /// Issue [`Control::ReadProjectGithubUrl`] for `project_id`.
    pub async fn read_project_github_url(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadProjectGithubUrl { project_id })
            .await
    }

    /// Issue [`Control::ReadRecentCommits`] for `project_id` capped
    /// at `limit` entries.
    pub async fn read_recent_commits(
        &self,
        request_id: u64,
        project_id: String,
        limit: u32,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadRecentCommits { project_id, limit })
            .await
    }

    // ── Git mutation verbs (`another-one-ojm.5`) ───────────────────

    /// `another-one-ojm.5` — issue a `Control::StageChangedFile`
    /// frame against the daemon. Fire-and-forget at the FRB level:
    /// the matching `WorkerReply::StageChangedFileAck` arrives on
    /// `subscribe_worker_replies` keyed to a fresh `request_id` the
    /// Dart layer allocates via [`Self::next_request_id`]. The Dart
    /// `IrohTransport` registers a `Completer` against that id
    /// before calling, so the await-side awaits the ack from there.
    pub async fn stage_changed_file(
        &self,
        request_id: u64,
        project_id: String,
        path: String,
        original_path: Option<String>,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::StageChangedFile {
                project_id,
                path,
                original_path,
            },
        )
        .await
    }

    // ── Settings → Git Actions (`another-one-ojm.8`) ──────────────

    /// Send `Control::ReadGitActionScripts`.
    pub async fn read_git_action_scripts(&self, request_id: u64) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadGitActionScripts)
            .await
    }

    /// Send `Control::SetGitCommitScript`.
    pub async fn set_git_commit_script(
        &self,
        request_id: u64,
        script: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::SetGitCommitScript { script })
            .await
    }

    /// Send `Control::ResetGitCommitScript`.
    pub async fn reset_git_commit_script(&self, request_id: u64) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ResetGitCommitScript)
            .await
    }

    /// Send `Control::SetGitPrScript`.
    pub async fn set_git_pr_script(&self, request_id: u64, script: String) -> anyhow::Result<()> {
        self.send_control(request_id, Control::SetGitPrScript { script })
            .await
    }

    /// Send `Control::ResetGitPrScript`.
    pub async fn reset_git_pr_script(&self, request_id: u64) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ResetGitPrScript)
            .await
    }

    // ── Settings → Keybindings (`another-one-ojm.8`) ──────────────

    /// Send `Control::ReadShortcutSettings`.
    pub async fn read_shortcut_settings(&self, request_id: u64) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadShortcutSettings)
            .await
    }

    /// Send `Control::SetShortcutBinding`.
    pub async fn set_shortcut_binding(
        &self,
        request_id: u64,
        action_id: String,
        binding: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::SetShortcutBinding { action_id, binding },
        )
        .await
    }

    /// Send `Control::ResetShortcutBinding`.
    pub async fn reset_shortcut_binding(
        &self,
        request_id: u64,
        action_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ResetShortcutBinding { action_id })
            .await
    }

    // ── Settings → MCP (`another-one-ojm.8`) ──────────────────────

    /// Send `Control::ReadMcpSettings`.
    pub async fn read_mcp_settings(&self, request_id: u64) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadMcpSettings)
            .await
    }

    /// Send `Control::McpAddFromCatalog`.
    pub async fn mcp_add_from_catalog(
        &self,
        request_id: u64,
        catalog_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::McpAddFromCatalog { catalog_id })
            .await
    }

    /// Send `Control::McpToggle`.
    pub async fn mcp_toggle(
        &self,
        request_id: u64,
        entry_id: String,
        provider_id: String,
        enabled: bool,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::McpToggle {
                entry_id,
                provider_id,
                enabled,
            },
        )
        .await
    }

    /// Send `Control::McpRemove`.
    pub async fn mcp_remove(&self, request_id: u64, entry_id: String) -> anyhow::Result<()> {
        self.send_control(request_id, Control::McpRemove { entry_id })
            .await
    }

    /// `another-one-ojm.5` — issue a `Control::UnstageChangedFile`
    /// frame. Same correlation contract as [`Self::stage_changed_file`].
    pub async fn unstage_changed_file(
        &self,
        request_id: u64,
        project_id: String,
        path: String,
        original_path: Option<String>,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::UnstageChangedFile {
                project_id,
                path,
                original_path,
            },
        )
        .await
    }

    /// `another-one-ojm.5` — issue a `Control::StageAllChanges` frame.
    pub async fn stage_all_changes(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::StageAllChanges { project_id })
            .await
    }

    /// `another-one-ojm.5` — issue a `Control::UnstageAllChanges`
    /// frame.
    pub async fn unstage_all_changes(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::UnstageAllChanges { project_id })
            .await
    }

    /// `another-one-ojm.5` — issue a `Control::DiscardChangedFile`
    /// frame.
    pub async fn discard_changed_file(
        &self,
        request_id: u64,
        project_id: String,
        path: String,
        untracked: bool,
        original_path: Option<String>,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::DiscardChangedFile {
                project_id,
                path,
                untracked,
                original_path,
            },
        )
        .await
    }

    /// `another-one-ojm.5` — issue a `Control::DiscardAllChanges`
    /// frame.
    pub async fn discard_all_changes(
        &self,
        request_id: u64,
        project_id: String,
        files: Vec<crate::api::local_session::ChangedFileDto>,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::DiscardAllChanges { project_id, files })
            .await
    }

    /// `another-one-ojm.5` — issue a `Control::RunToolbarGitAction`
    /// frame.
    pub async fn run_toolbar_git_action(
        &self,
        request_id: u64,
        project_id: String,
        action_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::RunToolbarGitAction {
                project_id,
                action_id,
            },
        )
        .await
    }

    /// `another-one-ojm.5` — issue a `Control::CreateBranch` frame.
    pub async fn create_branch(
        &self,
        request_id: u64,
        project_id: String,
        branch_name: String,
        use_current_task: bool,
        migrate_changes: bool,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::CreateBranch {
                project_id,
                branch_name,
                use_current_task,
                migrate_changes,
            },
        )
        .await
    }

    /// `another-one-ojm.5` — issue a `Control::CreateReviewTask` frame.
    pub async fn create_review_task(
        &self,
        request_id: u64,
        project_id: String,
        pull_request_number: u64,
        head_branch: String,
        agent_provider: Option<AgentProvider>,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::CreateReviewTask {
                project_id,
                pull_request_number,
                head_branch,
                agent_provider,
            },
        )
        .await
    }

    // ── PR + checks read verbs (`another-one-ojm.6`) ───────────────

    /// Issue [`Control::FindPullRequestStatus`] under `request_id`.
    /// The matching [`WorkerReply::PullRequestStatusAck`] (or
    /// [`WorkerReply::Err`]) arrives on `subscribe_worker_replies`
    /// keyed by the same id.
    pub async fn find_pull_request_status(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::FindPullRequestStatus { project_id })
            .await
    }

    /// Issue [`Control::ReadPullRequestChecks`] under `request_id`.
    pub async fn read_pull_request_checks(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadPullRequestChecks { project_id })
            .await
    }

    /// Issue [`Control::FindProjectPullRequests`] under `request_id`.
    pub async fn find_project_pull_requests(
        &self,
        request_id: u64,
        project_id: String,
        filter_index: u32,
        query: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::FindProjectPullRequests {
                project_id,
                filter_index,
                query,
            },
        )
        .await
    }

    // ── Custom actions + Open In + agents read verbs (`another-one-ojm.7`) ───

    /// Issue [`Control::OpenInState`] under `request_id`.
    pub async fn open_in_state(&self, request_id: u64) -> anyhow::Result<()> {
        self.send_control(request_id, Control::OpenInState).await
    }

    /// Issue [`Control::ListProjectActions`] under `request_id`.
    pub async fn list_project_actions(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ListProjectActions { project_id })
            .await
    }

    /// Issue [`Control::ReadEnabledAgents`] under `request_id`.
    pub async fn read_enabled_agents(&self, request_id: u64) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadEnabledAgents)
            .await
    }

    /// Issue [`Control::SubmitNewTask`] under `request_id`.
    pub async fn submit_new_task(
        &self,
        request_id: u64,
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_ids: Vec<String>,
        branch_mode_existing: bool,
        worktree_mode: bool,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::SubmitNewTask {
                project_id,
                task_name,
                source_branch,
                agent_ids,
                branch_mode_existing,
                worktree_mode,
            },
        )
        .await
    }

    /// Issue [`Control::AddAgentToSection`] under `request_id`.
    pub async fn add_agent_to_section(
        &self,
        request_id: u64,
        section_id: String,
        agent_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::AddAgentToSection {
                section_id,
                agent_id,
            },
        )
        .await
    }

    /// Issue [`Control::ActivateSectionTab`] under `request_id`.
    pub async fn activate_section_tab(
        &self,
        request_id: u64,
        section_id: String,
        tab_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::ActivateSectionTab { section_id, tab_id },
        )
        .await
    }

    /// Issue [`Control::CloseSectionTab`] under `request_id`.
    pub async fn close_section_tab(
        &self,
        request_id: u64,
        section_id: String,
        tab_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::CloseSectionTab { section_id, tab_id })
            .await
    }

    /// Issue [`Control::ToggleSectionTabPinned`] under `request_id`.
    pub async fn toggle_section_tab_pinned(
        &self,
        request_id: u64,
        section_id: String,
        tab_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::ToggleSectionTabPinned { section_id, tab_id },
        )
        .await
    }

    /// Issue [`Control::ReadAgentSettings`] under `request_id`.
    pub async fn read_agent_settings(&self, request_id: u64) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadAgentSettings)
            .await
    }

    /// Issue [`Control::ReadOpenInSettings`] under `request_id`.
    pub async fn read_open_in_settings(&self, request_id: u64) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadOpenInSettings)
            .await
    }

    /// Issue [`Control::SetOpenInAppEnabled`] under `request_id`.
    pub async fn set_open_in_app_enabled(
        &self,
        request_id: u64,
        app_id: String,
        enabled: bool,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::SetOpenInAppEnabled { app_id, enabled })
            .await
    }

    /// Issue [`Control::OpenProjectInApp`] under `request_id`.
    pub async fn open_project_in_app(
        &self,
        request_id: u64,
        project_id: String,
        app_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::OpenProjectInApp { project_id, app_id })
            .await
    }

    /// Issue [`Control::RunProjectAction`] under `request_id`.
    pub async fn run_project_action(
        &self,
        request_id: u64,
        project_id: String,
        section_id: String,
        action_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::RunProjectAction {
                project_id,
                section_id,
                action_id,
            },
        )
        .await
    }

    /// Issue [`Control::SaveProjectAction`] under `request_id`.
    pub async fn save_project_action(
        &self,
        request_id: u64,
        project_id: String,
        action: crate::api::local_session::ProjectActionDto,
        save_global_copy: bool,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::SaveProjectAction {
                project_id,
                action,
                save_global_copy,
            },
        )
        .await
    }

    /// Issue [`Control::DeleteProjectAction`] under `request_id`.
    pub async fn delete_project_action(
        &self,
        request_id: u64,
        project_id: String,
        action_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::DeleteProjectAction {
                project_id,
                action_id,
            },
        )
        .await
    }

    /// Issue [`Control::ReadCommitFileChanges`] for `project_id` /
    /// `commit_id`.
    pub async fn read_commit_file_changes(
        &self,
        request_id: u64,
        project_id: String,
        commit_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::ReadCommitFileChanges {
                project_id,
                commit_id,
            },
        )
        .await
    }

    /// Issue [`Control::ReadBranchCompareState`] for `project_id`
    /// against `target_branch`.
    pub async fn read_branch_compare_state(
        &self,
        request_id: u64,
        project_id: String,
        target_branch: String,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::ReadBranchCompareState {
                project_id,
                target_branch,
            },
        )
        .await
    }

    /// Issue [`Control::ReadBranchSettings`] for `project_id`.
    pub async fn read_branch_settings(
        &self,
        request_id: u64,
        project_id: String,
    ) -> anyhow::Result<()> {
        self.send_control(request_id, Control::ReadBranchSettings { project_id })
            .await
    }

    /// Issue [`Control::SetBranchSetting`] for `project_id`. `field`
    /// is one of `"default-branch"` / `"default-target-branch"`;
    /// `branch_name == None` clears the override.
    pub async fn set_branch_setting(
        &self,
        request_id: u64,
        project_id: String,
        field: String,
        branch_name: Option<String>,
    ) -> anyhow::Result<()> {
        self.send_control(
            request_id,
            Control::SetBranchSetting {
                project_id,
                field,
                branch_name,
            },
        )
        .await
    }

    /// Wrap a `Control` in the `request_id`-tagged envelope and push
    /// it to the writer task. Internal to the existing per-verb
    /// helpers above; future per-verb additions in `ojm.2..8` can
    /// also reuse this rather than re-implementing the wrap.
    async fn send_control(&self, request_id: u64, control: Control) -> anyhow::Result<()> {
        let payload = serde_json::to_vec(&ControlEnvelope {
            request_id,
            control,
        })
        .context("encode control envelope")?;
        self.send_frame(TY_CONTROL, payload).await
    }

    async fn send_frame(&self, ty: u8, payload: Vec<u8>) -> anyhow::Result<()> {
        let tx = self.send_tx.lock().await;
        match tx.as_ref() {
            Some(tx) => tx
                .send((ty, payload))
                .await
                .map_err(|_| anyhow::anyhow!("send channel closed")),
            None => Err(anyhow::anyhow!("session closed")),
        }
    }

    /// Start pushing inbound bytes into the given Dart StreamSink. Call once
    /// per session; subsequent calls return an error.
    pub async fn subscribe(&self, sink: StreamSink<Vec<u8>>) -> anyhow::Result<()> {
        let mut guard = self.incoming_rx.lock().await;
        let mut rx = guard
            .take()
            .ok_or_else(|| anyhow::anyhow!("already subscribed"))?;
        drop(guard);

        tokio_rt().spawn(async move {
            while let Some(bytes) = rx.recv().await {
                if sink.add(bytes).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    /// Start pushing decoded worker replies into the given Dart StreamSink.
    /// Same one-shot subscription shape as [`subscribe`]; the second call
    /// returns an error. Each item is a [`WorkerReplyMessage`] carrying
    /// the originating `request_id` (or [`PUSH_REQUEST_ID`] = `0` for
    /// daemon-pushed frames) plus the decoded variant. Replies arrive
    /// in the order the daemon sent them; the Dart layer dispatches
    /// against its `Map<int, Completer<WorkerReply>>` rather than
    /// relying on ordering.
    pub async fn subscribe_worker_replies(
        &self,
        sink: StreamSink<WorkerReplyMessage>,
    ) -> anyhow::Result<()> {
        let mut guard = self.worker_replies_rx.lock().await;
        let mut rx = guard
            .take()
            .ok_or_else(|| anyhow::anyhow!("already subscribed to worker replies"))?;
        drop(guard);

        tokio_rt().spawn(async move {
            while let Some(message) = rx.recv().await {
                if sink.add(message).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    /// Closes the session. Safe to call multiple times.
    pub async fn close(&self) {
        self.send_tx.lock().await.take();
        if let Some(close_tx) = self.closer.lock().await.take() {
            let _ = close_tx.send(());
        }
        // Wake any per-verb callers blocked on a pending oneshot —
        // dropping the senders surfaces as `RecvError` on the awaiter
        // side, which `request_and_await` translates into a
        // session-closed error.
        let mut guard = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        guard.waiters.clear();
    }
}
