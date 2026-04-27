//! FRB-bound view DTOs the desktop + mobile UIs consume.
//!
//! Originally housed the `LocalSession` FFI fast-path that the
//! desktop used to talk to its in-process daemon; that path was
//! deleted in `another-one-ojm.11` once the loopback iroh wire
//! reached parity. The DTOs stay because they are the canonical
//! FRB-exposed shape of every cross-cutting view payload — both
//! transports' `WorkerReply::*Ack` variants reuse them directly
//! (see `crate::api::iroh_client::WorkerReply`), so FRB generates
//! a single Dart class per DTO regardless of the underlying
//! transport.
//!
//! Module name kept (`api::local_session`) on purpose: rotating it
//! to `api::dtos` would touch ~20 Dart import sites for zero
//! semantic gain. The rename is bookkeeping that can land later
//! independently.

use super::iroh_client::AgentProvider;

/// Placeholder reference so FRB's parser keeps [`OpenInSettingsView`]
/// + [`OpenInAppSettingsRow`] in the generated Dart bindings even
/// though no `pub fn` / `WorkerReply` variant currently consumes
/// them. The Settings → Open In page imports the types; the
/// underlying `Control::ReadOpenInSettings` wire variant is
/// follow-up work (not yet wired). Returns a default empty view so
/// the Dart side doesn't see `null` from a partially-implemented
/// daemon.
#[doc(hidden)]
pub fn _force_export_open_in_settings_view() -> OpenInSettingsView {
    OpenInSettingsView {
        available_apps: Vec::new(),
    }
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::ToolbarActionOutcome`]. The
/// titlebar surfaces `toast_message` as a snackbar (warning palette
/// when `warning` is true) and uses `refresh_git_state` to decide
/// whether to invalidate the active changed-files / git-state
/// providers after the call returns.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ToolbarActionOutcomeDto {
    pub toast_message: String,
    pub warning: bool,
    pub refresh_git_state: bool,
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::PullRequestStatus`]. Drives
/// the titlebar dropdown's Create PR / Draft PR enabledness — when
/// a PR already exists for the branch, those rows are disabled.
///
/// `Serialize`/`Deserialize` are derived so the iroh wire's
/// `WorkerReply::PullRequestStatusAck` payload can reuse this
/// struct directly (instead of carrying a parallel mirror in
/// `crate::api::iroh_client`). FRB generates a single Dart class
/// for both transports — `LocalTransport.findPullRequestStatus`
/// and `IrohTransport.findPullRequestStatus` produce identical
/// `PullRequestStatusDto` values on the Dart side.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PullRequestStatusDto {
    pub number: u64,
    pub url: String,
    pub state: PullRequestStateDto,
}

/// Branch metadata snapshot for the active project — current branch
/// name plus ahead/behind counts. Drives the titlebar's
/// idle-primary-action selection.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ActiveGitStateDto {
    pub current_branch: Option<String>,
    pub ahead_count: u32,
    pub behind_count: u32,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ProjectBranchCompareState`].
/// Drives the right sidebar's Compare pane: the current branch +
/// configured target + the file list of the diff.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BranchCompareView {
    pub current_branch: Option<String>,
    pub target_branch: String,
    pub files: Vec<BranchCompareFileDto>,
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::PullRequestState`]. Drives the
/// chip + chrome on each PR row: open vs merged vs closed shapes
/// the badge palette and the row-level affordances.
///
/// Wire form mirrors `daemon-sandbox/src/frame.rs::PullRequestState`
/// — lowercase strings — so the iroh wire and the LocalSession
/// path produce the same Dart enum value.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PullRequestStateDto {
    Open,
    Closed,
    Merged,
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::ProjectPagePullRequest`]. One
/// entry per row in the project page's Open PRs section.
///
/// `Serialize`/`Deserialize` are derived so the iroh wire's
/// `WorkerReply::ProjectPullRequestsAck` payload can reuse this
/// struct directly. Same parity-via-single-source-of-truth move as
/// `PullRequestStatusDto` / `CheckDto`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectPagePullRequestDto {
    pub number: u64,
    pub url: String,
    pub title: String,
    /// Head ref (the PR's source branch). Rendered in mono on the
    /// row's bottom line.
    pub branch: String,
    pub author: String,
    pub lines_added: i32,
    pub lines_removed: i32,
    pub draft: bool,
    /// `true` when GitHub's review_decision is REVIEW_REQUIRED —
    /// drives the red CI badge and 'Review required' chip.
    pub review_required: bool,
    pub review_requested_to_me: bool,
    pub created_by_me: bool,
    pub state: PullRequestStateDto,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ResolvedProjectBranchSettings`].
/// Drives the project page Configuration panel: the current
/// configured + effective values for both fields, plus the
/// available branch list the dropdowns enumerate.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ResolvedProjectBranchSettingsDto {
    pub root_project_id: String,
    pub available_branches: Vec<String>,
    /// `Some(name)` when the user explicitly picked a default branch;
    /// `None` means automatic (UI shows the trigger label as
    /// "Automatic").
    pub configured_default_branch: Option<String>,
    /// What the project actually uses today — falls back to
    /// `automatic_primary_branch_name` when configured is None or
    /// unavailable.
    pub effective_default_branch: Option<String>,
    pub configured_default_target_branch: Option<String>,
    pub effective_default_target_branch: Option<String>,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::BranchCompareFile`]. Each
/// entry is one file changed inside a commit (or branch compare).
/// `status` is the single git status char ('A', 'M', 'D', 'R', 'C',
/// 'T') passed through verbatim — UI maps it via the same
/// `changed_file_status_color` table the Changes pane uses.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BranchCompareFileDto {
    pub path: String,
    /// Set on rename/copy entries — the from-path. UI renders
    /// "Renamed from {original_path}" beneath the row when present.
    pub original_path: Option<String>,
    /// Single status char as a 1-char string (FRB doesn't expose
    /// `char` directly).
    pub status: String,
    pub additions: i32,
    pub deletions: i32,
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::PullRequestCheckBucket`].
/// Drives the glyph + colour for each check row on the right
/// sidebar's Checks pane.
///
/// Wire form mirrors `daemon-sandbox/src/frame.rs::CheckBucket`
/// — snake_case strings — so the iroh wire and the LocalSession
/// path produce the same Dart enum value.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckBucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::PullRequestCheck`]. Mostly raw
/// — UI maps `bucket` to glyph/colour and `state` is the verbatim
/// string `gh pr checks` returned ("pass", "in_progress", etc.).
///
/// `Serialize`/`Deserialize` are derived so the iroh wire's
/// `WorkerReply::PullRequestChecksAck` payload can reuse this
/// struct directly (instead of carrying a parallel mirror in
/// `crate::api::iroh_client`). Same parity-via-single-source-of-
/// truth move as `PullRequestStatusDto` above.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckDto {
    /// Check name (e.g. "build / linux", "lint").
    pub name: String,
    /// Raw state string from gh CLI; shown as the row subtitle.
    pub state: String,
    pub bucket: CheckBucket,
    /// Optional human description gh CLI sometimes provides.
    pub description: Option<String>,
    /// Link to the check run page on GitHub. UI renders the row
    /// clickable when set.
    pub link: Option<String>,
    /// Pre-formatted "1m 23s"-style duration. None for checks that
    /// haven't started or completed.
    pub duration_text: Option<String>,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::BranchCommit`]. Carries the
/// pre-computed relative authored timestamp ("3 hours ago") so the
/// UI doesn't have to round-trip through chrono on every redraw.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CommitDto {
    /// Full SHA — used as the row id and for the diff lookup.
    pub id: String,
    /// 7-char abbreviated SHA shown next to the message.
    pub short_id: String,
    /// First line of the commit message.
    pub subject: String,
    pub author_name: String,
    /// Pre-formatted "X minutes ago"-style label. Computed Rust-side
    /// because chrono is already a dep there; doing it Dart-side
    /// would mean shipping a humanize-duration package for one
    /// caller. This is borderline display logic but the read is
    /// one-shot per pane open so the FFI cost is a wash.
    pub authored_relative: String,
}

/// FRB-friendly snapshot of the right sidebar's Commits pane data.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RecentCommitsView {
    /// Current branch — shown as the pane subtitle in GPUI.
    pub current_branch: Option<String>,
    /// True when more commits exist past the requested `limit`. UI
    /// uses this to render a "Load more" affordance.
    pub has_more: bool,
    pub commits: Vec<CommitDto>,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ChangedFile`]. Carries the
/// raw status chars + diff counts; UI maps them to glyphs/colours
/// per `desktop/src/right_sidebar.rs::changed_file_status_char`
/// and `changed_file_status_color`. We don't pre-format on the
/// Rust side so the bridge stays display-agnostic and we don't
/// pay the cross-FFI cost of re-encoding every redraw.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChangedFileDto {
    /// Path relative to the project root, the way `git status` reports it.
    pub path: String,
    /// Set on rename (`R`) / copy (`C`) entries — the from-path. UI
    /// renders this as `original → path` when present.
    pub original_path: Option<String>,
    pub staged_additions: i32,
    pub staged_deletions: i32,
    pub unstaged_additions: i32,
    pub unstaged_deletions: i32,
    /// Index status char from `git status --porcelain` — `M`/`A`/`D`/
    /// `R`/`C`/`?`/' '. UI maps via the GPUI char-to-glyph table.
    pub index_status: String,
    /// Worktree status char — same alphabet as `index_status`.
    pub worktree_status: String,
    /// True when the file is `??` (untracked) in `git status`.
    pub untracked: bool,
}

/// FRB-friendly mirror of [`OpenInAppKind`] with the
/// pre-computed display strings. Lives here (not in core) because
/// FRB's binding generator only walks bridge crate types — we'd
/// need a re-export shim either way and the mapping is one-to-one.
///
/// `Deserialize` exists so the iroh transport can decode the wire
/// payload (`OpenInAppWire` from `daemon-sandbox`) straight into this
/// type — the field names match by design. FRB ignores extra derives.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OpenInAppDto {
    /// Stable id matching `OpenInAppKind::id()` — `"cursor"`,
    /// `"zed"`, `"vscode"`, `"file-manager"`. Round-trips through
    /// [`LocalSession::open_project_in_app`].
    pub id: String,
    /// Human-readable label rendered in the dropdown. Localised at
    /// the platform level (Finder vs File Manager vs File Explorer).
    pub label: String,
    /// Tooltip text — same copy GPUI's titlebar dropdown uses.
    pub description: String,
    /// Asset path for the app's glyph, relative to the app bundle's
    /// asset root. Both the GPUI and Flutter UIs ship the same
    /// `assets/icons/open_in__*.svg` files, so the path is valid
    /// on either side without translation.
    pub icon_path: String,
}

/// Snapshot returned by [`LocalSession::open_in_state`].
///
/// `Deserialize` exists for the iroh transport; field names match
/// `daemon-sandbox::frame::OpenInStateWire` so the wire JSON decodes
/// directly into this DTO without a per-field map step.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OpenInState {
    /// Apps offered in the dropdown, ordered as `OpenInAppKind::all()`
    /// declares them — Cursor, Zed, VS Code, File Manager.
    pub enabled_apps: Vec<OpenInAppDto>,
    /// Id of the app the titlebar's primary action launches. `None`
    /// when no app is enabled at all (a fresh install on a host with
    /// none of the editors detected).
    pub preferred_app_id: Option<String>,
}

/// FRB-friendly mirror of one entry in
/// [`another_one_core::agents::AGENTS`]. Carries everything the
/// new-task modal's agent multi-select needs to render a chip
/// (label + icon path) without the UI side hard-coding a copy.
///
/// `Deserialize` lets the iroh transport decode the daemon's
/// `AgentSummaryWire` directly into this DTO.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentSummaryDto {
    /// Stable id used by the bridge's `submit_new_task` verb.
    pub id: String,
    pub label: String,
    pub icon_path: String,
    pub provider: Option<AgentProvider>,
}

/// Snapshot returned by [`LocalSession::read_enabled_agents`].
/// Pairs the enabled-agents list with the user's preferred default
/// (the chip the modal pre-checks on open).
///
/// `Deserialize` shape matches `daemon-sandbox::frame::EnabledAgentsViewWire`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EnabledAgentsView {
    pub agents: Vec<AgentSummaryDto>,
    pub default_agent_id: Option<String>,
}

/// One row of the Settings → Agents page. Carries everything the
/// page renders (label + icon + enabled / default flags +
/// per-agent launch args list) so the UI can update its state
/// without re-issuing reads after every toggle.
///
/// `Deserialize` mirrors `daemon-sandbox::frame::AgentSettingsRowWire`
/// so the iroh transport decodes the wire JSON straight into this
/// DTO.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentSettingsRow {
    pub id: String,
    pub label: String,
    pub icon_path: String,
    pub provider: Option<AgentProvider>,
    pub enabled: bool,
    pub is_default: bool,
    pub launch_args: Vec<String>,
}

/// Snapshot returned by [`LocalSession::read_agent_settings`].
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentSettingsView {
    /// Every agent in `AGENTS` (canonical order), enabled-or-not.
    pub agents: Vec<AgentSettingsRow>,
    pub default_agent_id: Option<String>,
}

/// One row of the Settings → Open In page. Carries everything the
/// page renders (label + icon + description + per-host enabled
/// flag). UI maps these to clickable rows that toggle through
/// `Control::SetOpenInAppEnabled` (placeholder — wire variant not
/// yet implemented; the Settings → Open In page throws
/// `UnimplementedError` from the [`crate::api::iroh_client::IrohSession`]
/// override until the daemon side lands).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OpenInAppSettingsRow {
    /// Stable id matching `OpenInAppKind::id()` — `"cursor"`,
    /// `"zed"`, `"vscode"`, `"file-manager"`.
    pub id: String,
    pub label: String,
    pub description: String,
    pub icon_path: String,
    pub enabled: bool,
}

/// Snapshot of the Settings → Open In page. Same placeholder note
/// as [`OpenInAppSettingsRow`] — wire variant not yet implemented
/// post-`ojm.11`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OpenInSettingsView {
    /// Every Open-In app the host detected as installed, in
    /// canonical order. Empty when no supported app is on the
    /// host's PATH / installed.
    pub available_apps: Vec<OpenInAppSettingsRow>,
}

/// Snapshot returned by [`LocalSession::read_git_action_scripts`].
/// Carries the resolved-current text for both the commit and PR
/// scripts (built-in default when there's no override) plus a
/// `using_default` flag per script so the UI can flip the
/// subtitle copy without re-checking.
///
/// `serde::Deserialize` is wired so the iroh wire's
/// `WorkerReply::GitActionScriptsAck` (introduced in
/// `another-one-ojm.8`) decodes straight into this struct.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GitActionScriptsView {
    pub commit_script: String,
    pub commit_using_default: bool,
    pub pr_script: String,
    pub pr_using_default: bool,
}

/// One row of the Settings → Keybindings page. Carries the
/// human-readable label + the current binding string + the
/// built-in default binding for "reset" affordances.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ShortcutSettingsRow {
    /// Stable kebab-case id for the action (`cycle-projects`,
    /// `new-task`, etc.). Round-trips through
    /// [`LocalSession::set_shortcut_binding`].
    pub id: String,
    pub label: String,
    /// Current binding string, e.g. `"cmd-shift-]"`. Empty when
    /// the action has been intentionally cleared.
    pub current_binding: String,
    pub default_binding: String,
}

/// Snapshot returned by [`LocalSession::read_shortcut_settings`].
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ShortcutSettingsView {
    pub actions: Vec<ShortcutSettingsRow>,
}

/// FRB-friendly mirror of [`another_one_core::mcp::McpSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpSourceDto {
    Catalog,
    Custom,
    BuiltInDaemon,
}

/// FRB-friendly mirror of [`another_one_core::mcp::McpTransport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransportKindDto {
    Stdio,
    Http,
}

/// One row of the Settings → MCP page's registry section.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct McpServerDto {
    pub id: String,
    pub label: String,
    pub source: McpSourceDto,
    pub transport_kind: McpTransportKindDto,
    /// Provider ids (kebab-case: `claude-code`, `cursor-agent`,
    /// `codex`, `gemini`, `opencode`, `amp`) the entry is enabled
    /// for. UI maps these to short labels.
    pub enabled_for: Vec<String>,
}

/// One row of the Settings → MCP page's catalog section. Carries
/// the static metadata; the UI flips this into an `McpServerDto`
/// row after `mcp_add_from_catalog`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct McpCatalogEntryDto {
    pub id: String,
    pub label: String,
    pub description: String,
    pub docs_url: String,
}

/// Snapshot returned by [`LocalSession::read_mcp_settings`].
#[derive(Debug, Clone, serde::Deserialize)]
pub struct McpSettingsView {
    pub catalog_entries: Vec<McpCatalogEntryDto>,
    pub registry_entries: Vec<McpServerDto>,
    /// Providers whose last sync failed — UI tints their toggle
    /// red. Empty in this first cut (sync errors live only in
    /// GPUI's `mcp_last_sync_errors` today).
    pub sync_error_provider_ids: Vec<String>,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ProjectActionIcon`]. Stable
/// kebab-case ids round-trip the GPUI on-disk format
/// (`projects.json`) so a user can switch desktop binaries without
/// the icon picker resetting.
///
/// `Deserialize` exists so the iroh transport can decode the wire
/// payload (`ProjectActionIconWire` from `daemon-sandbox`) directly
/// into this DTO. Wire form is kebab-case to match
/// `core::project_store::ProjectActionIcon`'s on-disk shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionIconDto {
    Play,
    Test,
    Lint,
    Configure,
    Build,
    Debug,
    Agent,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ProjectActionScope`]. Ordered
/// "project first, global last" because that's how the dropdown row
/// order treats them — global rows render with a globe glyph beside
/// the action label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionScopeDto {
    Project,
    Global,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ProjectActionAccess`]. Drives
/// the agent-mode CLI's permission flag — `default` passes nothing
/// extra, the other three map to `--read-only`, `--workspace-write`,
/// `--full-access` (Claude Code today; other providers ignore).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionAccessDto {
    Default,
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

/// Tagged-union mirror of
/// [`another_one_core::project_store::ProjectActionKind`]. The Dart
/// side discriminates on the variant; FRB emits a sealed-class
/// hierarchy with `Shell` and `Agent` subclasses.
///
/// `Deserialize` lets the iroh transport decode the daemon's
/// `ProjectActionKindWire` (externally-tagged via serde default)
/// directly into this enum.
#[derive(Debug, Clone, serde::Deserialize)]
pub enum ProjectActionKindDto {
    /// A shell command typed verbatim into a freshly-spawned PTY.
    /// `command` is run as `<command>\n` so multi-line input works
    /// the same way it would in an interactive shell.
    Shell { command: String },
    /// An agent CLI launch. `prompt` is the seed message;
    /// `model`/`traits`/`mode`/`access` are agent-specific knobs
    /// fed to `project_action_agent_launch_args`.
    Agent {
        prompt: String,
        provider: AgentProvider,
        model: Option<String>,
        traits: Option<String>,
        mode: Option<String>,
        access: ProjectActionAccessDto,
    },
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ProjectAction`]. Carries the
/// lot — id (empty for a never-saved action), display name, icon,
/// run-on-worktree-create flag, scope, and the kind-specific
/// payload. UI maps `icon` to its asset path via
/// `ProjectActionIconDto.icon_path` (Dart-side helper).
///
/// `Deserialize` lets the iroh transport decode the daemon's
/// `ProjectActionWire` shape directly into this DTO; field names
/// align by design.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProjectActionDto {
    pub id: String,
    pub name: String,
    pub icon: ProjectActionIconDto,
    pub run_on_worktree_create: bool,
    pub scope: ProjectActionScopeDto,
    pub kind: ProjectActionKindDto,
}
