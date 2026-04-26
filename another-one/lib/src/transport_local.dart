// In-process FFI implementation of DaemonConnection.
//
// Wraps the FRB-generated LocalSession (see rust/api/local_session.dart),
// which talks directly to the host binary's `RegistryState` instead of
// going through QUIC. The Flutter desktop UI uses this transport for
// its embedded daemon — same interface as `IrohTransport`, zero
// network round-trip.
//
// One LocalTransport per session. The host binary calls
// `another_one_bridge::local_registry::set_local_registry` from Rust at
// boot before any Dart `connect()` reaches the FFI; the LocalSession
// methods read/write that shared `RegistryState`.

import 'dart:async';
import 'dart:typed_data';

import 'connection.dart';
import 'rust/api/iroh_client.dart' show AgentProvider, WorkerReply;
import 'rust/api/local_session.dart';
import 'transport.dart';

// Extends `DaemonConnection` so future trait-only methods with
// default impls are inherited automatically; sibling
// `IrohTransport` does the same. `TerminalTransport` is still an
// `implements` since it's a separate abstract.
class LocalTransport extends DaemonConnection implements TerminalTransport {
  /// Stable identifier for this connection within a `ConnectionManager`.
  /// Today there's only ever one local connection (the desktop's own
  /// embedded daemon), so a constant is fine; switch to a uuid if/when
  /// the architecture grows to support multiple in-process daemons.
  static const String _id = 'local';

  final StreamController<Uint8List> _incoming =
      StreamController<Uint8List>.broadcast();
  final StreamController<TransportStatus> _status =
      StreamController<TransportStatus>.broadcast();
  final StreamController<WorkerReply> _workerReplies =
      StreamController<WorkerReply>.broadcast();

  LocalSession? _session;
  StreamSubscription<Uint8List>? _incomingSub;
  StreamSubscription<WorkerReply>? _workerRepliesSub;
  TransportStatus _current = const TransportStatus.disconnected();
  bool _closed = false;

  // ── DaemonConnection identity ───────────────────────────────────

  @override
  String get id => _id;

  @override
  String get displayName => 'Local';

  // ── Streams + status ────────────────────────────────────────────

  @override
  Stream<Uint8List> get incoming => _incoming.stream;

  @override
  Stream<TransportStatus> get status => _status.stream;

  @override
  TransportStatus get currentStatus => _current;

  @override
  Stream<WorkerReply> get workerReplies => _workerReplies.stream;

  // ── Lifecycle ───────────────────────────────────────────────────

  @override
  void connect() {
    if (_closed) return;
    if (_session != null) return; // already connected
    _publish(const TransportStatus.connecting());
    _connectAsync();
  }

  Future<void> _connectAsync() async {
    try {
      final session = await localConnect();
      if (_closed) {
        await session.close();
        return;
      }
      _session = session;
      _incomingSub = session.subscribe().listen(
        _incoming.add,
        onError: (err) => _publish(TransportStatus.error(err.toString())),
        onDone: () {
          // The byte channel closes when the session does. Flip to
          // disconnected so listeners notice; we don't tear down the
          // controllers since a future `connect()` would rebuild
          // them — but this transport is one-shot like the iroh
          // sibling, so today the session is over.
          _publish(const TransportStatus.disconnected());
        },
        cancelOnError: true,
      );
      _workerRepliesSub = session.subscribeWorkerReplies().listen(
        _workerReplies.add,
        onError: (_) {},
      );
      _publish(const TransportStatus.connected());
    } catch (e) {
      _publish(TransportStatus.error(e.toString()));
    }
  }

  // ── DaemonConnection methods ────────────────────────────────────

  @override
  Future<void> listProjects() async {
    final session = _session;
    if (session == null) return;
    await session.listProjects();
  }

  /// Add an on-disk project directory to the embedded daemon's
  /// store. Returns whether a new project was inserted (`false`
  /// means the same path was already registered — idempotent).
  /// On success the daemon pushes a fresh ProjectList over
  /// [workerReplies], so callers don't need a follow-up
  /// [listProjects].
  @override
  Future<bool> addProject(String path) async {
    final session = _session;
    if (session == null) {
      throw StateError('addProject: LocalTransport not connected');
    }
    return session.addProject(path: path);
  }

  /// Remove a project from the embedded daemon's store. Cascades
  /// to the project's tasks + terminal sections. Idempotent — an
  /// unknown id is a silent no-op. The daemon pushes a fresh
  /// ProjectList on completion.
  @override
  Future<void> removeProject(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError('removeProject: LocalTransport not connected');
    }
    await session.removeProject(projectId: projectId);
  }

  /// Create a worktree task on `projectId`. Spawns a fresh git
  /// worktree off `sourceBranch`, prepares the worktree project,
  /// and inserts both the new project + task into the daemon's
  /// store. Returns the new task's `sectionId` so callers can
  /// navigate to it.
  ///
  /// `agentProvider` of `null` (or `AgentProvider.shell`) launches
  /// a plain shell tab. Any concrete provider value is propagated
  /// to `TerminalLaunchConfig::for_provider` so future `launch_tab`
  /// calls spawn the agent CLI.
  @override
  Future<String> createWorktreeTask({
    required String projectId,
    required String taskName,
    required String sourceBranch,
    AgentProvider? agentProvider,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('createWorktreeTask: LocalTransport not connected');
    }
    return session.createWorktreeTask(
      projectId: projectId,
      taskName: taskName,
      sourceBranch: sourceBranch,
      agentProvider: agentProvider,
    );
  }

  /// Rename a task. Empty/whitespace-only names are rejected. Returns
  /// whether the on-disk store actually changed (false for unknown
  /// ids or no-op renames).
  @override
  Future<bool> renameTask(String taskId, String newName) async {
    final session = _session;
    if (session == null) {
      throw StateError('renameTask: LocalTransport not connected');
    }
    return session.renameTask(taskId: taskId, newName: newName);
  }

  /// Pin or unpin a task. Pinned tasks float to the top of their
  /// project's list. Returns whether state changed (false on
  /// idempotent re-set).
  @override
  Future<bool> setTaskPinned(String taskId, bool pinned) async {
    final session = _session;
    if (session == null) {
      throw StateError('setTaskPinned: LocalTransport not connected');
    }
    return session.setTaskPinned(taskId: taskId, pinned: pinned);
  }

  /// Remove a task from the embedded daemon's store. The on-disk
  /// worktree branch is left untouched. Returns whether a task was
  /// actually removed (false on unknown id).
  @override
  Future<bool> removeTask(String projectId, String taskId) async {
    final session = _session;
    if (session == null) {
      throw StateError('removeTask: LocalTransport not connected');
    }
    return session.removeTask(projectId: projectId, taskId: taskId);
  }

  /// Resolve the project's GitHub remote URL by shelling out (in a
  /// blocking task) to `git remote get-url origin` on the project
  /// path. Returns `null` when the project has no `origin` remote
  /// or the remote isn't a github.com URL. Stable across the
  /// project's lifetime; cache results aggressively.
  @override
  Future<String?> readProjectGithubUrl(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError('readProjectGithubUrl: LocalTransport not connected');
    }
    return session.readProjectGithubUrl(projectId: projectId);
  }

  /// Read the host's enabled-and-installed Open-In apps + the
  /// preferred default. Powers the titlebar split-button: primary
  /// icon comes from `preferredAppId`, dropdown rows from
  /// `enabledApps`. Cheap to call repeatedly so callers can
  /// `ref.refresh` after mutating actions instead of subscribing.
  @override
  Future<OpenInState> openInState() async {
    final session = _session;
    if (session == null) {
      throw StateError('openInState: LocalTransport not connected');
    }
    return session.openInState();
  }

  /// Open the project's directory in the named app and record that
  /// app as the new preferred default. Spawns the platform-specific
  /// command via the embedded daemon's host process — only valid on
  /// the local connection.
  @override
  Future<void> openProjectInApp({
    required String projectId,
    required String appId,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('openProjectInApp: LocalTransport not connected');
    }
    await session.openProjectInApp(projectId: projectId, appId: appId);
  }

  /// Snapshot the working-tree changes for `projectId`. Reads
  /// through `read_project_git_state` synchronously (inside a
  /// `spawn_blocking`) on the daemon side. `null` is returned for
  /// an unknown project id — same gate the right sidebar applies.
  @override
  Future<List<ChangedFileDto>?> readChangedFiles(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError('readChangedFiles: LocalTransport not connected');
    }
    return session.readChangedFiles(projectId: projectId);
  }

  /// Read the most recent `limit` commits on the project's current
  /// branch via `read_project_branch_commit_state`. `null` for an
  /// unknown project id.
  @override
  Future<RecentCommitsView?> readRecentCommits({
    required String projectId,
    required int limit,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('readRecentCommits: LocalTransport not connected');
    }
    return session.readRecentCommits(projectId: projectId, limit: limit);
  }

  @override
  Future<List<BranchCompareFileDto>?> readCommitFileChanges({
    required String projectId,
    required String commitId,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('readCommitFileChanges: LocalTransport not connected');
    }
    return session.readCommitFileChanges(
      projectId: projectId,
      commitId: commitId,
    );
  }

  @override
  Future<List<ProjectPagePullRequestDto>?> findProjectPullRequests({
    required String projectId,
    required int filterIndex,
    required String query,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError(
        'findProjectPullRequests: LocalTransport not connected',
      );
    }
    return session.findProjectPullRequests(
      projectId: projectId,
      filterIndex: filterIndex,
      query: query,
    );
  }

  @override
  Future<String> createReviewTask({
    required String projectId,
    required int pullRequestNumber,
    required String headBranch,
    AgentProvider? agentProvider,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('createReviewTask: LocalTransport not connected');
    }
    return session.createReviewTask(
      projectId: projectId,
      pullRequestNumber: BigInt.from(pullRequestNumber),
      headBranch: headBranch,
      agentProvider: agentProvider,
    );
  }

  @override
  Future<String> createBranch({
    required String projectId,
    required String branchName,
    required bool useCurrentTask,
    required bool migrateChanges,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('createBranch: LocalTransport not connected');
    }
    return session.createBranch(
      projectId: projectId,
      branchName: branchName,
      useCurrentTask: useCurrentTask,
      migrateChanges: migrateChanges,
    );
  }

  @override
  Future<String> slugifyBranchName(String name) async {
    final session = _session;
    if (session == null) {
      throw StateError('slugifyBranchName: LocalTransport not connected');
    }
    return session.slugifyBranchName(name: name);
  }

  @override
  Future<String?> repoDefaultCommitAction(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError(
        'repoDefaultCommitAction: LocalTransport not connected',
      );
    }
    return session.repoDefaultCommitAction(projectId: projectId);
  }

  @override
  Future<ActiveGitStateDto?> readActiveGitState(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError('readActiveGitState: LocalTransport not connected');
    }
    return session.readActiveGitState(projectId: projectId);
  }

  @override
  Future<PullRequestStatusDto?> findPullRequestStatus(
    String projectId,
  ) async {
    final session = _session;
    if (session == null) {
      throw StateError('findPullRequestStatus: LocalTransport not connected');
    }
    return session.findPullRequestStatus(projectId: projectId);
  }

  @override
  Future<ToolbarActionOutcomeDto> runToolbarGitAction({
    required String projectId,
    required String actionId,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('runToolbarGitAction: LocalTransport not connected');
    }
    return session.runToolbarGitAction(
      projectId: projectId,
      actionId: actionId,
    );
  }

  @override
  Future<BranchCompareView?> readBranchCompareState({
    required String projectId,
    required String targetBranch,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('readBranchCompareState: LocalTransport not connected');
    }
    return session.readBranchCompareState(
      projectId: projectId,
      targetBranch: targetBranch,
    );
  }

  @override
  Future<ResolvedProjectBranchSettingsDto?> readBranchSettings(
    String projectId,
  ) async {
    final session = _session;
    if (session == null) {
      throw StateError('readBranchSettings: LocalTransport not connected');
    }
    return session.resolvedBranchSettings(projectId: projectId);
  }

  @override
  Future<bool> setBranchSetting({
    required String projectId,
    required String field,
    String? branchName,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('setBranchSetting: LocalTransport not connected');
    }
    return session.setProjectBranchSetting(
      projectId: projectId,
      field: field,
      branchName: branchName,
    );
  }

  /// Shell out (via the embedded daemon) to `gh pr checks` for the
  /// current branch's PR. See [`DaemonConnection.readPullRequestChecks`]
  /// for the three-state return contract.
  @override
  Future<List<CheckDto>?> readPullRequestChecks(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError('readPullRequestChecks: LocalTransport not connected');
    }
    return session.readPullRequestChecks(projectId: projectId);
  }

  @override
  Future<void> stageChangedFile({
    required String projectId,
    required String path,
    String? originalPath,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('stageChangedFile: LocalTransport not connected');
    }
    await session.stageChangedFile(
      projectId: projectId,
      path: path,
      originalPath: originalPath,
    );
  }

  @override
  Future<void> unstageChangedFile({
    required String projectId,
    required String path,
    String? originalPath,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('unstageChangedFile: LocalTransport not connected');
    }
    await session.unstageChangedFile(
      projectId: projectId,
      path: path,
      originalPath: originalPath,
    );
  }

  @override
  Future<void> stageAllChanges(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError('stageAllChanges: LocalTransport not connected');
    }
    await session.stageAllChanges(projectId: projectId);
  }

  @override
  Future<void> unstageAllChanges(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError('unstageAllChanges: LocalTransport not connected');
    }
    await session.unstageAllChanges(projectId: projectId);
  }

  @override
  Future<void> discardChangedFile({
    required String projectId,
    required String path,
    required bool untracked,
    String? originalPath,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('discardChangedFile: LocalTransport not connected');
    }
    await session.discardChangedFile(
      projectId: projectId,
      path: path,
      originalPath: originalPath,
      untracked: untracked,
    );
  }

  @override
  Future<List<ProjectActionDto>> listProjectActions(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError('listProjectActions: LocalTransport not connected');
    }
    return session.listProjectActions(projectId: projectId);
  }

  @override
  Future<void> saveProjectAction({
    required String projectId,
    required ProjectActionDto action,
    required bool saveGlobalCopy,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('saveProjectAction: LocalTransport not connected');
    }
    await session.saveProjectAction(
      projectId: projectId,
      action: action,
      saveGlobalCopy: saveGlobalCopy,
    );
  }

  @override
  Future<bool> deleteProjectAction({
    required String projectId,
    required String actionId,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('deleteProjectAction: LocalTransport not connected');
    }
    return session.deleteProjectAction(
      projectId: projectId,
      actionId: actionId,
    );
  }

  @override
  Future<String> runProjectAction({
    required String projectId,
    required String sectionId,
    required String actionId,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('runProjectAction: LocalTransport not connected');
    }
    return session.runProjectAction(
      projectId: projectId,
      sectionId: sectionId,
      actionId: actionId,
    );
  }

  @override
  Future<List<String>> readProjectBranches(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError('readProjectBranches: LocalTransport not connected');
    }
    return session.readProjectBranches(projectId: projectId);
  }

  @override
  Future<String?> primaryBranchForProject(String projectId) async {
    final session = _session;
    if (session == null) {
      throw StateError('primaryBranchForProject: LocalTransport not connected');
    }
    return session.primaryBranchForProject(projectId: projectId);
  }

  @override
  Future<EnabledAgentsView> readEnabledAgents() async {
    final session = _session;
    if (session == null) {
      throw StateError('readEnabledAgents: LocalTransport not connected');
    }
    return session.readEnabledAgents();
  }

  @override
  Future<String> submitNewTask({
    required String projectId,
    required String taskName,
    required String sourceBranch,
    required List<String> agentIds,
    required bool branchModeExisting,
    required bool worktreeMode,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('submitNewTask: LocalTransport not connected');
    }
    return session.submitNewTask(
      projectId: projectId,
      taskName: taskName,
      sourceBranch: sourceBranch,
      agentIds: agentIds,
      branchModeExisting: branchModeExisting,
      worktreeMode: worktreeMode,
    );
  }

  @override
  Future<String> addAgentToSection({
    required String sectionId,
    required String agentId,
  }) async {
    final session = _session;
    if (session == null) {
      throw StateError('addAgentToSection: LocalTransport not connected');
    }
    return session.addAgentToSection(
      sectionId: sectionId,
      agentId: agentId,
    );
  }

  @override
  Future<void> attachTab({
    required String sectionId,
    required String tabId,
  }) async {
    final session = _session;
    if (session == null) return;
    await session.attachTab(sectionId: sectionId, tabId: tabId);
  }

  @override
  Future<void> detachTab() async {
    final session = _session;
    if (session == null) return;
    await session.detachTab();
  }

  @override
  Future<void> launchTab({
    required String sectionId,
    required String tabId,
  }) async {
    final session = _session;
    if (session == null) return;
    await session.launchTab(sectionId: sectionId, tabId: tabId);
  }

  @override
  Future<void> tabResize({required int cols, required int rows}) async {
    final session = _session;
    if (session == null) return;
    await session.tabResize(cols: cols, rows: rows);
  }

  @override
  void sendBytes(List<int> bytes) {
    final session = _session;
    if (session == null) return;
    // Fire-and-forget: PTY stdin sends shouldn't block the UI thread,
    // and the FRB call is already async-on-Rust-side. Errors surface
    // via the byte stream's onError if the writer's gone.
    unawaited(session.send(bytes: bytes));
  }

  @override
  void sendResize({required int cols, required int rows}) {
    // Convenience alias for `tabResize` — `TerminalTransport`'s
    // older single-PTY interface uses this name; new callers should
    // prefer `tabResize` directly.
    unawaited(tabResize(cols: cols, rows: rows));
  }

  // ── Close ────────────────────────────────────────────────────────

  @override
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    final session = _session;
    _session = null;
    await _incomingSub?.cancel();
    await _workerRepliesSub?.cancel();
    if (session != null) {
      await session.close();
    }
    _publish(const TransportStatus.disconnected());
    await _incoming.close();
    await _status.close();
    await _workerReplies.close();
  }

  void _publish(TransportStatus s) {
    _current = s;
    if (_status.isClosed) return;
    _status.add(s);
  }
}
