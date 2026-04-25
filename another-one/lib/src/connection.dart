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
