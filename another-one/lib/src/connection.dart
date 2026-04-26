// Multi-daemon connection abstraction (Phase 2 of the GPUI→Flutter
// migration).
//
// Today's mobile app has a single `IrohTransport` instance bound to
// whatever endpoint the user paired with. The future Flutter desktop
// — and eventually a multi-host mobile experience — needs to hold N
// connections of mixed kinds:
//
//   * `IrohConnection` — remote daemon over QUIC + iroh (today's
//     mobile path).
//   * `LocalConnection` — in-process FFI to the desktop's embedded
//     daemon (Phase 2 of the migration). Bypasses the wire entirely
//     for local-host operations; same interface so the UI doesn't
//     care which kind it's talking to.
//
// `DaemonConnection` is the unified interface those implementors
// satisfy. `ConnectionManager` holds them in a list and broadcasts
// changes so the UI can render a host switcher.
//
// Today's `IrohTransport` will be migrated to implement this in a
// follow-up; consumers (`AppRoot`, `TaskPage`, etc.) that hold a
// concrete `IrohTransport` keep working unchanged.
//
// `TerminalTransport` (the older, narrower interface in
// `transport.dart`) is retained for now since `task_page.dart`
// references it explicitly. It's a strict subset of
// `DaemonConnection`; future cleanups can migrate consumers and
// retire it.

import 'dart:async';
import 'dart:typed_data';

import 'rust/api/iroh_client.dart';
import 'rust/api/local_session.dart'
    show
        ActiveGitStateDto,
        AgentSettingsView,
        BranchCompareFileDto,
        BranchCompareView,
        ChangedFileDto,
        CheckDto,
        EnabledAgentsView,
        GitActionScriptsView,
        McpSettingsView,
        OpenInSettingsView,
        OpenInState,
        ProjectActionDto,
        ProjectPagePullRequestDto,
        PullRequestStatusDto,
        RecentCommitsView,
        ResolvedProjectBranchSettingsDto,
        ShortcutSettingsView,
        ToolbarActionOutcomeDto;
import 'transport.dart';

/// Unified interface for any daemon — local FFI or remote iroh —
/// the UI can hold and drive. One connection corresponds to one
/// daemon endpoint.
abstract class DaemonConnection {
  /// Opaque identifier stable within a single `ConnectionManager`.
  /// Implementations decide the format (uuid v4, endpoint id, etc.).
  String get id;

  /// Human-readable label rendered in the host switcher / header.
  /// Examples: `"Local"`, `"laptop@home"`, `"192.168.1.5"`.
  String get displayName;

  // ── Status / lifecycle ──────────────────────────────────────────

  Stream<TransportStatus> get status;
  TransportStatus get currentStatus;

  /// Starts the connection. Non-blocking; reports progress via
  /// [status]. May throw synchronously if the connection is in
  /// a state where `connect` is invalid (already connected, already
  /// closed, etc.).
  void connect();

  /// Closes the connection and releases underlying resources. Safe
  /// to call multiple times. After close, the connection is inert
  /// — build a new one to reconnect.
  Future<void> close();

  // ── Project tree ────────────────────────────────────────────────

  /// Asks the daemon to send its full project / task / tab tree.
  /// Reply arrives on [workerReplies] as a `WorkerReply.projectList`.
  Future<void> listProjects();

  /// Stream of structured replies from the daemon (project list,
  /// future: git refresh results, MCP tool results, etc.). Each
  /// implementation produces the same `WorkerReply` variants
  /// regardless of transport.
  Stream<WorkerReply> get workerReplies;

  // ── Per-tab attach / detach / launch ────────────────────────────

  /// Subscribes to the live PTY byte stream for `(section_id, tab_id)`.
  /// Bytes arrive on [incoming] until `detachTab` is called or the
  /// connection drops. At most one attachment per connection.
  Future<void> attachTab({required String sectionId, required String tabId});

  /// Stops the current tab attachment if any.
  Future<void> detachTab();

  /// Asks the daemon to spawn the tab's PTY if it isn't already
  /// running. Idempotent on already-running tabs.
  Future<void> launchTab({required String sectionId, required String tabId});

  /// Resizes the currently-attached tab's PTY. No-op if nothing is
  /// attached.
  Future<void> tabResize({required int cols, required int rows});

  // ── Per-tab PTY data ────────────────────────────────────────────

  /// Bytes received from the currently-attached tab's PTY. Chunk
  /// boundaries are not guaranteed to align with lines.
  Stream<Uint8List> get incoming;

  /// Sends raw bytes to the currently-attached tab's PTY stdin.
  void sendBytes(List<int> bytes);

  // ── Project mutation ────────────────────────────────────────────
  //
  // These verbs each have a corresponding `Control` wire variant on
  // the iroh transport that hasn't landed yet (see migration plan
  // §"Wire-protocol additions"). LocalTransport implements them
  // directly against the in-process RegistryState; IrohTransport
  // inherits the throwing defaults below until the wire grows. The
  // throw message names the missing wire variant so the failure mode
  // is "loud and self-documenting" rather than silent no-op when a
  // remote-host UI tries to mutate.

  /// Add an on-disk project directory to the daemon's project store.
  /// Returns whether a new project was inserted (`false` means the
  /// path was already known — idempotent).
  Future<bool> addProject(String path) {
    throw UnimplementedError(
      'addProject: requires Control::AddProject wire variant on the '
      'iroh transport (not yet implemented).',
    );
  }

  /// Remove a project from the daemon's store. Cascades to the
  /// project's tasks + terminal sections.
  Future<void> removeProject(String projectId) {
    throw UnimplementedError(
      'removeProject: requires Control::RemoveProject wire variant '
      'on the iroh transport (not yet implemented).',
    );
  }

  /// Create a worktree task on `projectId`. Returns the new task's
  /// `sectionId` so callers can navigate to it.
  Future<String> createWorktreeTask({
    required String projectId,
    required String taskName,
    required String sourceBranch,
    AgentProvider? agentProvider,
  }) {
    throw UnimplementedError(
      'createWorktreeTask: requires Control::CreateTask wire variant '
      'on the iroh transport (not yet implemented).',
    );
  }

  /// Rename a task. Returns whether the on-disk store actually
  /// changed (false on unknown id or no-op rename).
  Future<bool> renameTask(String taskId, String newName) {
    throw UnimplementedError(
      'renameTask: requires Control::RenameTask wire variant on the '
      'iroh transport (not yet implemented).',
    );
  }

  /// Pin or unpin a task. Returns whether state changed.
  Future<bool> setTaskPinned(String taskId, bool pinned) {
    throw UnimplementedError(
      'setTaskPinned: requires Control::SetTaskPinned wire variant '
      'on the iroh transport (not yet implemented).',
    );
  }

  /// Remove a task from the daemon's store. The on-disk worktree
  /// branch is left untouched.
  Future<bool> removeTask(String projectId, String taskId) {
    throw UnimplementedError(
      'removeTask: requires Control::DeleteTask wire variant on the '
      'iroh transport (not yet implemented).',
    );
  }

  /// Resolve a project's GitHub remote URL (`git remote get-url
  /// origin`, normalised). Returns `null` when not a github.com
  /// remote.
  Future<String?> readProjectGithubUrl(String projectId) {
    throw UnimplementedError(
      'readProjectGithubUrl: requires the iroh transport to expose '
      'the project github link cache (not yet implemented).',
    );
  }

  /// Snapshot of the host's "Open In" config — installed-and-enabled
  /// apps + the preferred default. Used by the titlebar split-button
  /// to render its primary icon and the chevron dropdown.
  ///
  /// Open-In is currently a desktop-host-local concern and not
  /// remoted: the iroh transport throws by design until a remote
  /// client gets a "launch app on the host you're connected to"
  /// surface (out of scope for the migration).
  Future<OpenInState> openInState() {
    throw UnimplementedError(
      'openInState: Open-In is host-local; remote daemons do not '
      'expose installed-app detection (out of migration scope).',
    );
  }

  /// Open `projectId`'s directory in the named app and persist that
  /// app as the user's preferred default. `appId` matches
  /// `OpenInAppKind::id()` — `"cursor"`, `"zed"`, `"vscode"`, or
  /// `"file-manager"`. Throws if `appId` is unknown or the spawn
  /// fails.
  Future<void> openProjectInApp({
    required String projectId,
    required String appId,
  }) {
    throw UnimplementedError(
      'openProjectInApp: Open-In is host-local; remote daemons do '
      'not launch apps on a remote host (out of migration scope).',
    );
  }

  /// One-shot read of working-tree changes for `projectId`. Powers
  /// the right sidebar's Changes pane. Returns `null` when the
  /// project id is unknown — UI renders the "working tree clean"
  /// empty state in that case rather than surfacing an error.
  ///
  /// Per-keystroke refresh is not the model here; callers
  /// `ref.invalidate` after a known-mutation (commit, switch
  /// branch, stage) or on a coarse interval. A streaming variant
  /// can land later if the polling loop becomes a bottleneck.
  Future<List<ChangedFileDto>?> readChangedFiles(String projectId) {
    throw UnimplementedError(
      'readChangedFiles: requires Control::ReadChangedFiles wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Recent commits on `projectId`'s current branch, capped at
  /// `limit`. Powers the right sidebar's Commits pane. Returns
  /// `null` when the project id is unknown.
  Future<RecentCommitsView?> readRecentCommits({
    required String projectId,
    required int limit,
  }) {
    throw UnimplementedError(
      'readRecentCommits: requires Control::ReadRecentCommits wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Per-commit file change list — powers the expandable rows of
  /// the Commits pane. Returns `null` for an unknown project id;
  /// throws when the git invocation fails (e.g. commit pruned).
  Future<List<BranchCompareFileDto>?> readCommitFileChanges({
    required String projectId,
    required String commitId,
  }) {
    throw UnimplementedError(
      'readCommitFileChanges: requires Control::ReadCommitFileChanges '
      'wire variant on the iroh transport (not yet implemented).',
    );
  }

  /// Create a new branch from HEAD on `projectId`. When
  /// `useCurrentTask` is true the existing checkout switches in
  /// place; otherwise a new worktree is created and (optionally)
  /// uncommitted changes are migrated into it. Returns the new
  /// task's `sectionId` for the worktree case, empty string for
  /// the current-task case.
  Future<String> createBranch({
    required String projectId,
    required String branchName,
    required bool useCurrentTask,
    required bool migrateChanges,
  }) {
    throw UnimplementedError(
      'createBranch: requires Control::CreateBranch wire variant on '
      'the iroh transport (not yet implemented).',
    );
  }

  /// Compute the canonical branch slug for a free-text input.
  /// Powers the Create Branch modal's live `Branch: …` preview.
  Future<String> slugifyBranchName(String name) {
    throw UnimplementedError(
      'slugifyBranchName: requires Control::SlugifyBranchName wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// User's preferred default commit action for the active
  /// project's root repo. Returns `"commit"`, `"commit-and-push"`,
  /// or `null` when no preference has been recorded.
  Future<String?> repoDefaultCommitAction(String projectId) {
    throw UnimplementedError(
      'repoDefaultCommitAction: requires Control::RepoDefaultCommitAction '
      'wire variant on the iroh transport (not yet implemented).',
    );
  }

  /// Snapshot the active project's branch metadata: current branch
  /// name, ahead/behind counts. Powers the titlebar git-actions
  /// split-button's primary-action selection.
  Future<ActiveGitStateDto?> readActiveGitState(String projectId) {
    throw UnimplementedError(
      'readActiveGitState: requires Control::ReadActiveGitState wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Latest pull-request status for `projectId`'s current branch.
  /// Returns `null` when the project has no open PR. Drives the
  /// Create PR / Draft PR enabledness in the titlebar dropdown.
  /// Both transports implement this against their respective code
  /// paths: `LocalTransport` calls
  /// `LocalSession::find_pull_request_status` directly; `IrohTransport`
  /// issues `Control::FindPullRequestStatus` and dispatches the
  /// `WorkerReply::PullRequestStatusAck` reply through its
  /// completer table (see `another-one-ojm.6`).
  Future<PullRequestStatusDto?> findPullRequestStatus(String projectId);

  /// Run a toolbar git action against `projectId`. `actionId` is one
  /// of `"commit"`, `"commit-and-push"`, `"undo-last-commit"`,
  /// `"fetch"`, `"pull"`, `"push"`, `"force-push"`, `"create-pr"`,
  /// `"create-draft-pr"`. Returns the toast message + warning /
  /// refresh flags so callers can surface a snackbar and decide
  /// whether to invalidate dependent providers.
  Future<ToolbarActionOutcomeDto> runToolbarGitAction({
    required String projectId,
    required String actionId,
  }) {
    throw UnimplementedError(
      'runToolbarGitAction: requires Control::RunToolbarGitAction wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Diff the project's current branch against `targetBranch`
  /// (= `target..HEAD`). Powers the right sidebar's Compare pane.
  /// Returns `null` for unknown projects; throws when the diff
  /// invocation fails (target branch doesn't exist, etc.).
  Future<BranchCompareView?> readBranchCompareState({
    required String projectId,
    required String targetBranch,
  }) {
    throw UnimplementedError(
      'readBranchCompareState: requires Control::ReadBranchCompareState '
      'wire variant on the iroh transport (not yet implemented).',
    );
  }

  /// Resolve the project's branch settings — configured + effective
  /// values for default and target branch, plus the available
  /// branch list. Powers the project page's Configuration panel
  /// and the right sidebar's Compare tab gate. Returns `null` for
  /// unknown project ids or projects without repo metadata.
  Future<ResolvedProjectBranchSettingsDto?> readBranchSettings(
    String projectId,
  ) {
    throw UnimplementedError(
      'readBranchSettings: requires Control::ReadBranchSettings wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Fetch open pull requests for `projectId` filtered by
  /// `filterIndex` (0=all, 1=needs my review, 2=author:@me, 3=draft)
  /// plus an optional free-text `query` (GitHub search syntax).
  /// Returns `null` for unknown project ids; throws when gh CLI
  /// fails (CLI missing, auth, network).
  ///
  /// Both transports implement this against their respective code
  /// paths: `LocalTransport` calls
  /// `LocalSession::find_project_pull_requests` directly;
  /// `IrohTransport` issues `Control::FindProjectPullRequests` and
  /// dispatches the `WorkerReply::ProjectPullRequestsAck` reply
  /// through its completer table (`another-one-ojm.6`).
  Future<List<ProjectPagePullRequestDto>?> findProjectPullRequests({
    required String projectId,
    required int filterIndex,
    required String query,
  });

  /// Spawn a review task targeting a specific PR. Clones the PR's
  /// head branch into a worktree, prepares the project, inserts the
  /// task. Returns the new task's `sectionId`.
  ///
  /// `agentProvider` of `null` (or `AgentProvider.shell`) launches
  /// a plain shell tab. Any concrete provider value selects the
  /// corresponding agent CLI for the new task.
  Future<String> createReviewTask({
    required String projectId,
    required int pullRequestNumber,
    required String headBranch,
    AgentProvider? agentProvider,
  }) {
    throw UnimplementedError(
      'createReviewTask: requires Control::CreateReviewTask wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Update one branch-setting field. `field` is `"default-branch"`
  /// or `"default-target-branch"`; `branchName` of `null` clears
  /// the override (effective falls back to automatic resolution).
  /// Returns true when the persisted store changed.
  Future<bool> setBranchSetting({
    required String projectId,
    required String field,
    String? branchName,
  }) {
    throw UnimplementedError(
      'setBranchSetting: requires Control::SetBranchSetting wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// CI check runs for `projectId`'s current PR. Powers the right
  /// sidebar's Checks pane.
  ///
  /// Three-state contract:
  ///   * `Some(list)` — the PR exists; these are its checks
  ///     (possibly empty when no checks are configured).
  ///   * `null` — no PR for the current branch, or unknown project.
  ///   * Throw — gh CLI missing, network error, etc.
  ///
  /// Both transports implement this against their respective code
  /// paths: `LocalTransport` calls
  /// `LocalSession::read_pull_request_checks` directly;
  /// `IrohTransport` issues `Control::ReadPullRequestChecks` and
  /// dispatches the `WorkerReply::PullRequestChecksAck` reply
  /// through its completer table (`another-one-ojm.6`).
  Future<List<CheckDto>?> readPullRequestChecks(String projectId);

  /// Stage one changed file via `git add`. `originalPath` is set
  /// only on rename/copy entries — git needs both source and
  /// destination to resolve the pair correctly. Throws on git
  /// failure with the stderr appended.
  Future<void> stageChangedFile({
    required String projectId,
    required String path,
    String? originalPath,
  }) {
    throw UnimplementedError(
      'stageChangedFile: requires Control::StageChangedFile wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Unstage one changed file via `git restore --staged` (or
  /// `git reset HEAD` on older git). See [`stageChangedFile`] for
  /// the rename-pair contract.
  Future<void> unstageChangedFile({
    required String projectId,
    required String path,
    String? originalPath,
  }) {
    throw UnimplementedError(
      'unstageChangedFile: requires Control::UnstageChangedFile wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// `git add -A` on the project root — stage every change.
  Future<void> stageAllChanges(String projectId) {
    throw UnimplementedError(
      'stageAllChanges: requires Control::StageAllChanges wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Unstage every currently-staged change.
  Future<void> unstageAllChanges(String projectId) {
    throw UnimplementedError(
      'unstageAllChanges: requires Control::UnstageAllChanges wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Discard one file's changes. Untracked files are deleted from
  /// disk; tracked files are restored from HEAD. `untracked` is
  /// passed through verbatim so the daemon picks the right code
  /// path. Mirrors `core::revert_changed_file`. This action is
  /// destructive and should always be gated behind a confirmation.
  Future<void> discardChangedFile({
    required String projectId,
    required String path,
    required bool untracked,
    String? originalPath,
  }) {
    throw UnimplementedError(
      'discardChangedFile: requires Control::DiscardChangedFile wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  // ── Custom actions (titlebar split-button + modal editor) ───────

  /// List the merged project + global custom actions for `project_id`,
  /// in dropdown order. Empty list when no actions exist or the
  /// project is unknown — matches `ProjectStore::project_actions`.
  Future<List<ProjectActionDto>> listProjectActions(String projectId) {
    throw UnimplementedError(
      'listProjectActions: requires Control::ListProjectActions wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Insert or update a custom action. `saveGlobalCopy=true` saves
  /// to global UI state (visible across every project on the host)
  /// and removes any project-scoped copy with the same id; `false`
  /// saves to the project's own list and removes the global copy.
  Future<void> saveProjectAction({
    required String projectId,
    required ProjectActionDto action,
    required bool saveGlobalCopy,
  }) {
    throw UnimplementedError(
      'saveProjectAction: requires Control::SaveProjectAction wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Delete a custom action by id from both project and global
  /// lists. Returns whether anything was removed.
  Future<bool> deleteProjectAction({
    required String projectId,
    required String actionId,
  }) {
    throw UnimplementedError(
      'deleteProjectAction: requires Control::DeleteProjectAction wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Run a custom action inside `sectionId`'s task: appends a fresh
  /// terminal tab, queues its PTY launch, and (for shell actions)
  /// records the command bytes to write once the PTY is up. Returns
  /// the new tab id.
  Future<String> runProjectAction({
    required String projectId,
    required String sectionId,
    required String actionId,
  }) {
    throw UnimplementedError(
      'runProjectAction: requires Control::RunProjectAction wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  // ── New-task modal data ──────────────────────────────────────────

  /// Branch names available for `projectId`. Powers the new-task
  /// modal's source-branch dropdown.
  Future<List<String>> readProjectBranches(String projectId) {
    throw UnimplementedError(
      'readProjectBranches: requires Control::ReadProjectBranches wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Default branch the new-task modal seeds for `projectId`.
  /// `null` when the project has no current branch (fresh repo).
  Future<String?> primaryBranchForProject(String projectId) {
    throw UnimplementedError(
      'primaryBranchForProject: requires Control::PrimaryBranchForProject '
      'wire variant on the iroh transport (not yet implemented).',
    );
  }

  /// Snapshot of the user-enabled agents on this host plus the
  /// preferred default. Drives the new-task modal's multi-select.
  Future<EnabledAgentsView> readEnabledAgents() {
    throw UnimplementedError(
      'readEnabledAgents: requires Control::ReadEnabledAgents wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Submit the new-task modal. Routes to either the worktree or
  /// direct path based on `worktreeMode`. Returns the new task's
  /// `sectionId`.
  Future<String> submitNewTask({
    required String projectId,
    required String taskName,
    required String sourceBranch,
    required List<String> agentIds,
    required bool branchModeExisting,
    required bool worktreeMode,
  }) {
    throw UnimplementedError(
      'submitNewTask: requires Control::SubmitNewTask wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Append an agent tab (or plain shell when `agentId` is empty)
  /// to an existing task's section. Returns the new tab id so the
  /// UI can switch focus to it.
  Future<String> addAgentToSection({
    required String sectionId,
    required String agentId,
  }) {
    throw UnimplementedError(
      'addAgentToSection: requires Control::AddAgentToSection wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Set the active tab for a section. Persists the choice; does
  /// not relaunch (Dart-side `selectedTabProvider` triggers attach).
  Future<void> activateSectionTab({
    required String sectionId,
    required String tabId,
  }) {
    throw UnimplementedError(
      'activateSectionTab: requires Control::ActivateSectionTab wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Remove a tab from a section. Returns the new active tab id
  /// (empty when the section is now empty).
  Future<String> closeSectionTab({
    required String sectionId,
    required String tabId,
  }) {
    throw UnimplementedError(
      'closeSectionTab: requires Control::CloseSectionTab wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Flip the `pinned` flag on a tab. Returns the new pinned state.
  Future<bool> toggleSectionTabPinned({
    required String sectionId,
    required String tabId,
  }) {
    throw UnimplementedError(
      'toggleSectionTabPinned: requires Control::ToggleSectionTabPinned '
      'wire variant on the iroh transport (not yet implemented).',
    );
  }

  // ── Settings → Agents ────────────────────────────────────────────

  /// Full agent registry — every entry in `core::agents::AGENTS`
  /// paired with per-host enabled/default flags + launch args.
  /// Drives the Settings → Agents page.
  Future<AgentSettingsView> readAgentSettings() {
    throw UnimplementedError(
      'readAgentSettings: requires Control::ReadAgentSettings wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Toggle an agent's enabled flag. Returns whether anything
  /// changed.
  Future<bool> setAgentEnabled({
    required String agentId,
    required bool enabled,
  }) {
    throw UnimplementedError(
      'setAgentEnabled: requires Control::SetAgentEnabled wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Mark an agent as the default. Returns whether anything changed.
  Future<bool> setDefaultAgent(String agentId) {
    throw UnimplementedError(
      'setDefaultAgent: requires Control::SetDefaultAgent wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Replace the launch-args list for an agent. Returns whether
  /// the value actually changed.
  Future<bool> setAgentLaunchArgs({
    required String agentId,
    required List<String> args,
  }) {
    throw UnimplementedError(
      'setAgentLaunchArgs: requires Control::SetAgentLaunchArgs wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  // ── Settings → Open In ───────────────────────────────────────────

  /// Snapshot of every detected Open-In app on this host paired
  /// with its enabled flag. Empty list when no supported app is
  /// installed.
  Future<OpenInSettingsView> readOpenInSettings() {
    throw UnimplementedError(
      'readOpenInSettings: requires Control::ReadOpenInSettings wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Toggle an Open-In app's enabled flag.
  Future<void> setOpenInAppEnabled({
    required String appId,
    required bool enabled,
  }) {
    throw UnimplementedError(
      'setOpenInAppEnabled: requires Control::SetOpenInAppEnabled wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  // ── Settings → Git Actions ───────────────────────────────────────

  /// Snapshot of both git-action LLM scripts (commit + PR), with
  /// the resolved-current text and a per-script "using default"
  /// flag.
  Future<GitActionScriptsView> readGitActionScripts() {
    throw UnimplementedError(
      'readGitActionScripts: requires Control::ReadGitActionScripts wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Update the commit-message generation script. Empty / matching
  /// the default reverts to the built-in template.
  Future<bool> setGitCommitScript(String script) {
    throw UnimplementedError(
      'setGitCommitScript: requires Control::SetGitCommitScript wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Reset the commit-message script back to the built-in default.
  Future<bool> resetGitCommitScript() {
    throw UnimplementedError(
      'resetGitCommitScript: requires Control::ResetGitCommitScript wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Update the PR title/body generation script.
  Future<bool> setGitPrScript(String script) {
    throw UnimplementedError(
      'setGitPrScript: requires Control::SetGitPrScript wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Reset the PR script back to the built-in default.
  Future<bool> resetGitPrScript() {
    throw UnimplementedError(
      'resetGitPrScript: requires Control::ResetGitPrScript wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  // ── Settings → Keybindings ───────────────────────────────────────

  /// Snapshot of every shortcut action paired with its current
  /// + default binding strings (kebab-case modifiers).
  Future<ShortcutSettingsView> readShortcutSettings() {
    throw UnimplementedError(
      'readShortcutSettings: requires Control::ReadShortcutSettings '
      'wire variant on the iroh transport (not yet implemented).',
    );
  }

  /// Set / clear a shortcut binding. Empty `binding` clears the
  /// action (it becomes inert).
  Future<void> setShortcutBinding({
    required String actionId,
    required String binding,
  }) {
    throw UnimplementedError(
      'setShortcutBinding: requires Control::SetShortcutBinding wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Reset a shortcut to its built-in default.
  Future<void> resetShortcutBinding(String actionId) {
    throw UnimplementedError(
      'resetShortcutBinding: requires Control::ResetShortcutBinding '
      'wire variant on the iroh transport (not yet implemented).',
    );
  }

  // ── Settings → MCP ───────────────────────────────────────────────

  /// Snapshot of the MCP catalog + on-disk registry.
  Future<McpSettingsView> readMcpSettings() {
    throw UnimplementedError(
      'readMcpSettings: requires Control::ReadMcpSettings wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Add a catalog entry to the registry.
  Future<void> mcpAddFromCatalog(String catalogId) {
    throw UnimplementedError(
      'mcpAddFromCatalog: requires Control::McpAddFromCatalog wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Toggle a registry entry's enabled flag for one provider.
  /// Runs `sync_all` on success.
  Future<void> mcpToggle({
    required String entryId,
    required String providerId,
    required bool enabled,
  }) {
    throw UnimplementedError(
      'mcpToggle: requires Control::McpToggle wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }

  /// Remove a registry entry. Runs `sync_all` on success.
  Future<void> mcpRemove(String entryId) {
    throw UnimplementedError(
      'mcpRemove: requires Control::McpRemove wire '
      'variant on the iroh transport (not yet implemented).',
    );
  }
}

/// In-memory list of active [DaemonConnection]s. Holds N regardless
/// of kind; broadcasts changes so the UI can re-render a host
/// switcher whenever a connection is added or removed.
///
/// Today's mobile app uses exactly one connection; the manager is
/// shape-compatible with that singleton case but ready to expand
/// without a UX overhaul.
class ConnectionManager {
  final List<DaemonConnection> _connections = [];
  final StreamController<List<DaemonConnection>> _changes =
      StreamController<List<DaemonConnection>>.broadcast();

  /// Read-only snapshot of currently-registered connections, in
  /// insertion order.
  List<DaemonConnection> get connections => List.unmodifiable(_connections);

  /// Emits the new connection list on every add / remove. Late
  /// subscribers do NOT get a replay — they should bootstrap from
  /// [connections] and then listen for further changes.
  Stream<List<DaemonConnection>> get changes => _changes.stream;

  /// Registers `connection` and emits the updated list. Does not
  /// call `connect()` — that's the caller's job, since lifecycle
  /// preferences (auto-connect vs. user-triggered) belong with the
  /// owner of the connection.
  void add(DaemonConnection connection) {
    _connections.add(connection);
    _changes.add(connections);
  }

  /// Closes the connection identified by `id` and removes it.
  /// No-op if no connection matches.
  Future<void> remove(String id) async {
    final index = _connections.indexWhere((c) => c.id == id);
    if (index < 0) return;
    final removed = _connections.removeAt(index);
    await removed.close();
    _changes.add(connections);
  }

  /// Returns the connection with the given `id`, or `null` if none
  /// match.
  DaemonConnection? lookup(String id) =>
      _connections.cast<DaemonConnection?>().firstWhere(
            (c) => c?.id == id,
            orElse: () => null,
          );

  /// Closes every connection and clears the list.
  Future<void> closeAll() async {
    final closing = _connections.toList();
    _connections.clear();
    _changes.add(connections);
    await Future.wait(closing.map((c) => c.close()));
  }
}
