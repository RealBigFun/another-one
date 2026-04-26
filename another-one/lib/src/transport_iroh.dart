// Iroh implementation of TerminalTransport.
//
// Wraps the flutter_rust_bridge-generated IrohSession (see rust/api/iroh_client.dart),
// which in turn wraps an iroh::Endpoint QUIC connection to a daemon speaking
// the `anotherone/pty/1` ALPN. Data and control (resize) frames share the
// same bidirectional stream via the length-prefixed framing defined in
// daemon-sandbox/src/frame.rs — resizes are delivered, not dropped.

import 'dart:async';
import 'dart:typed_data';

import 'connection.dart';
import 'rust/api/iroh_client.dart';
import 'rust/api/local_session.dart' as ls;
import 'transport.dart';

/// Dart surface for a `WorkerReply::Err` frame returned by the
/// daemon over the iroh wire. Carries the message string the daemon
/// emitted plus the `ErrKind` classification so call sites can
/// branch on `kind` without parsing `message`. Today the git-state
/// verbs in `another-one-ojm.4` throw this on the Err arm; the
/// `IrohTransport` overrides do not catch it, so it propagates to
/// the Riverpod async value as the error state.
class IrohWireException implements Exception {
  final String message;
  final ErrKind kind;

  const IrohWireException({required this.message, required this.kind});

  @override
  String toString() => 'IrohWireException(${kind.name}): $message';
}

// Extends `DaemonConnection` (not `implements`) so the abstract
// class's default mutation impls — which throw with a wire-variant-
// specific message — are inherited unchanged. `implements
// TerminalTransport` is still required because that's a separate
// abstract class. When the iroh wire grows `Control::AddProject`
// etc., override these methods to route through the new variants
// instead of throwing.
class IrohTransport extends DaemonConnection implements TerminalTransport {
  /// Hex-encoded EndpointId of the daemon to dial.
  final String endpointId;

  /// Direct `host:port` socket addresses of the daemon. Sandbox-only: no
  /// address-lookup service. At least one of [directAddrs]/[relayUrls]
  /// must be non-empty.
  final List<String> directAddrs;

  /// Relay URLs the daemon is reachable through. Needed when the client is
  /// off-LAN (e.g. mobile on cellular behind CGNAT) and direct hole-punching
  /// won't succeed.
  final List<String> relayUrls;

  /// TOFU pair token from the QR's `pair=<hex>` query param. Sent to
  /// the daemon in the first `Hello` control frame so an unpaired
  /// daemon can verify this peer scanned the current QR. Null for
  /// endpoints persisted from an older app version (or entered by
  /// hand) — the daemon will accept them iff they were already paired
  /// from a prior session; a brand-new device with null here will be
  /// rejected, which matches the security contract.
  final String? pairToken;

  final StreamController<Uint8List> _incoming =
      StreamController<Uint8List>.broadcast();
  final StreamController<TransportStatus> _status =
      StreamController<TransportStatus>.broadcast();
  // Worker replies are domain-data pushes from the daemon (git state,
  // future: PR status, etc.) — distinct from PTY bytes in semantics and
  // rate, so they get their own broadcast stream rather than being
  // union'd into [incoming]. Null WorkerReply sentinel never flows —
  // the Rust side guarantees every `add` carries a real variant.
  //
  // Two paths feed this controller:
  //   * Daemon-pushed frames (request_id == 0) — broadcast as-is to
  //     anyone listening on [workerReplies].
  //   * Replies to outbound calls (request_id > 0) — also broadcast,
  //     AND dispatched into [_pending] so a caller awaiting a
  //     specific request_id can receive its result via the completer
  //     map without filtering the broadcast stream itself.
  //
  // The dual path is deliberate: existing code that listens to
  // `workerReplies` keeps working unchanged, and per-call code can
  // adopt the completer model incrementally as `another-one-ojm.2..8`
  // migrate each verb.
  final StreamController<WorkerReply> _workerReplies =
      StreamController<WorkerReply>.broadcast();

  /// Outstanding `Future<WorkerReply>`s keyed by the `request_id` we
  /// allocated when issuing the call. Populated by [sendControlAndAwait]
  /// before the wire frame goes out; drained as `WorkerReplyMessage`s
  /// flow back from the daemon. A request_id is reserved for push
  /// frames (`0`) and never appears in this map.
  ///
  /// Per the foundation task `another-one-ojm.1` this map is wired
  /// up but not yet consumed — the existing `listProjects` / etc.
  /// methods still rely on the broadcast stream. Domain children
  /// (`another-one-ojm.2..8`) will route their new verbs through
  /// [sendControlAndAwait] and complete their futures here.
  final Map<int, Completer<WorkerReply>> _pending = {};

  IrohSession? _session;
  StreamSubscription<Uint8List>? _incomingSub;
  StreamSubscription<WorkerReplyMessage>? _workerRepliesSub;
  TransportStatus _current = const TransportStatus.disconnected();
  bool _closed = false;

  IrohTransport(
    this.endpointId, {
    this.directAddrs = const [],
    this.relayUrls = const [],
    this.pairToken,
  });

  @override
  Stream<Uint8List> get incoming => _incoming.stream;

  @override
  Stream<TransportStatus> get status => _status.stream;

  @override
  TransportStatus get currentStatus => _current;

  /// Daemon-pushed worker replies. Today the only variant is
  /// `ProjectList`, pushed in response to [listProjects].
  @override
  Stream<WorkerReply> get workerReplies => _workerReplies.stream;

  // ── DaemonConnection identity ───────────────────────────────────

  /// Remote endpoint id is a stable opaque value within the
  /// `ConnectionManager`'s lifetime — same daemon → same id.
  @override
  String get id => endpointId;

  /// Renders as the truncated endpoint id for now. Future: pull a
  /// human-set name from `paired_peers` so the UI shows
  /// `"laptop@home"` instead of hex.
  @override
  String get displayName =>
      endpointId.length > 12 ? '${endpointId.substring(0, 12)}…' : endpointId;

  @override
  void connect() {
    if (_closed) return;
    _publish(const TransportStatus.connecting());
    _connectAsync();
  }

  Future<void> _connectAsync() async {
    try {
      final session = await irohConnect(
        endpointId: endpointId,
        directAddrs: directAddrs,
        relayUrls: relayUrls,
        pairToken: pairToken,
      );
      if (_closed) {
        await session.close();
        return;
      }
      _session = session;
      _incomingSub = session.subscribe().listen(
        (bytes) {
          _incoming.add(bytes);
          if (_current.state != TransportState.connected) {
            _publish(const TransportStatus.connected());
          }
        },
        onError: (err) => _publish(_statusForError(err)),
        onDone: () => _publish(const TransportStatus.disconnected()),
        cancelOnError: true,
      );
      // Errors here don't tear down the transport — worker-reply
      // delivery is best-effort relative to the core PTY path.
      // Each WorkerReplyMessage is fanned out two ways: completer
      // dispatch (when request_id matches an outstanding call) and
      // the broadcast stream (always). See [_pending].
      _workerRepliesSub = session.subscribeWorkerReplies().listen(
        _dispatchWorkerReplyMessage,
        onError: (_) {},
      );
      _publish(const TransportStatus.connected());
    } catch (e) {
      _publish(_statusForError(e));
    }
  }

  /// Map an error thrown by the iroh layer to the best-fitting
  /// TransportStatus. The daemon closes the connection with the
  /// ASCII reason `anotherone/unpaired` when the peer isn't in its
  /// allowlist or fails TOFU validation, and `anotherone/incompatible-version`
  /// when its `Control::Hello.protocol_version` disagrees with the
  /// daemon's. iroh surfaces those reasons inside the close error
  /// string, so a substring match is good enough. Kept short to
  /// avoid leaking UI copy onto the wire.
  TransportStatus _statusForError(Object err) {
    final msg = err.toString();
    if (msg.contains('anotherone/unpaired')) {
      return const TransportStatus.unpaired('pairing expired or cleared');
    }
    if (msg.contains('anotherone/incompatible-version')) {
      return TransportStatus.error(
        'daemon speaks a different protocol version — please update '
        'the desktop or mobile app to match',
      );
    }
    return TransportStatus.error(msg);
  }

  /// Ask the daemon to send its project list. The response arrives on
  /// [workerReplies] as a `WorkerReply_ProjectList`. Returns a
  /// future for callers that want to surface send errors; most call
  /// sites can ignore it (the list will simply not arrive).
  @override
  Future<void> listProjects() async {
    final session = _session;
    if (session == null) return;
    await session.listProjects();
  }

  /// Attach this session's PTY-byte stream to a specific live tab on
  /// the daemon. Replaces any previous attachment; daemon begins
  /// forwarding TY_DATA frames for `tabId` under section `sectionId`.
  ///
  /// After calling, subscribe to [incoming] to receive bytes for the
  /// attached tab. Calling [attachTab] again with a different tab
  /// implicitly detaches the previous one.
  @override
  Future<void> attachTab({
    required String sectionId,
    required String tabId,
  }) async {
    final session = _session;
    if (session == null) return;
    await session.attachTab(sectionId: sectionId, tabId: tabId);
  }

  /// Stop receiving PTY bytes for the currently-attached tab. Safe to
  /// call without an active attachment (no-op).
  @override
  Future<void> detachTab() async {
    final session = _session;
    if (session == null) return;
    await session.detachTab();
  }

  /// Resize the currently-attached tab's PTY. Unlike [sendResize],
  /// this targets the tab on the daemon's side, not a single-session
  /// PTY — required when the daemon is bridging into a
  /// desktop-hosted live tab.
  @override
  Future<void> tabResize({required int cols, required int rows}) async {
    final session = _session;
    if (session == null) return;
    await session.tabResize(cols: cols, rows: rows);
  }

  /// Ask the daemon to launch this tab's PTY if it isn't already live.
  /// Safe to call unconditionally before [attachTab] — it's a no-op on
  /// the daemon side when the tab is already running.
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
  void sendBytes(List<int> bytes) {
    final session = _session;
    if (session == null) return;
    // Fire-and-forget: send is async but we don't block the caller. Errors
    // surface through the session's next operation.
    unawaited(session.send(bytes: bytes));
  }

  @override
  void sendResize({required int cols, required int rows}) {
    final session = _session;
    if (session == null) return;
    // Fire-and-forget; if the session is torn down before this completes,
    // the resize is simply lost, which matches how we handle `sendBytes`.
    unawaited(session.resize(cols: cols, rows: rows));
  }

  @override
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    await _incomingSub?.cancel();
    _incomingSub = null;
    await _workerRepliesSub?.cancel();
    _workerRepliesSub = null;
    // Wake every outstanding caller with an error so they don't
    // hang forever after the session closes mid-flight. Use a
    // close-specific exception so call sites can distinguish "session
    // dropped" from "daemon returned an Err frame" once that lands.
    for (final completer in _pending.values) {
      if (!completer.isCompleted) {
        completer.completeError(
          StateError('IrohTransport closed before reply arrived'),
        );
      }
    }
    _pending.clear();
    final s = _session;
    _session = null;
    if (s != null) {
      await s.close();
    }
    _publish(const TransportStatus.disconnected());
    await _incoming.close();
    await _workerReplies.close();
    await _status.close();
  }

  void _publish(TransportStatus s) {
    _current = s;
    if (!_status.isClosed) _status.add(s);
  }

  /// Route a `WorkerReplyMessage` to its waiting completer (if any)
  /// AND broadcast the unwrapped `WorkerReply` so existing listeners
  /// on [workerReplies] keep receiving frames. Push frames
  /// (`requestId == 0`) skip the completer table.
  ///
  /// Implementation note: BigInt → int conversion is safe because
  /// request_ids start at 1 and increment monotonically; saturating
  /// the JS-safe-int range (2^53) takes 285 years at 1 GHz issuance.
  /// We could keep the map keyed on BigInt but `int` is the natural
  /// Dart key type and matches every other id in the codebase.
  void _dispatchWorkerReplyMessage(WorkerReplyMessage message) {
    if (!_workerReplies.isClosed) {
      _workerReplies.add(message.reply);
    }
    final id = message.requestId.toInt();
    if (id == 0) return; // daemon push — no caller is waiting
    final completer = _pending.remove(id);
    if (completer != null && !completer.isCompleted) {
      completer.complete(message.reply);
    }
    // No completer found is fine: callers that don't await a reply
    // (fire-and-forget like `attachTab`) leave the id unregistered
    // even though the daemon may still emit a reply. Drop silently
    // rather than logging — this is the steady-state for
    // launch-style verbs.
  }

  /// Issue a control frame keyed by a freshly-allocated request_id
  /// and return a future that completes with the matching daemon
  /// reply. Domain verbs landing in `another-one-ojm.2..8` use
  /// this in place of the existing fire-and-forget `session.foo()`
  /// pattern.
  ///
  /// `send` is the caller-supplied closure that performs the actual
  /// FRB call once the request_id has been registered in the
  /// dispatch map — taking it as a closure means each verb decides
  /// which `Control::*` variant + arguments to encode without this
  /// helper having to know about every variant. Callers receive the
  /// `request_id` so they can pass it to a Rust-side `send_control`
  /// equivalent (added per-verb in domain tasks).
  ///
  /// Each domain verb landing in `another-one-ojm.2..8` routes its
  /// reply through this helper. First adopters: the git-state read
  /// verbs landed in `another-one-ojm.4`. Each Rust-side helper on
  /// `IrohSession` (e.g. `slugifyBranchName`) returns the freshly-
  /// allocated `request_id`; the Dart layer registers a completer
  /// keyed on it after the FRB call resolves and the recv loop
  /// dispatches the matching reply via [_dispatchWorkerReplyMessage].
  Future<WorkerReply> _sendControlAndAwait(
    Future<void> Function(int requestId) send,
  ) async {
    final session = _session;
    if (session == null) {
      throw StateError('IrohTransport not connected');
    }
    final id = (await session.nextRequestId()).toInt();
    final completer = Completer<WorkerReply>();
    _pending[id] = completer;
    try {
      await send(id);
    } catch (e) {
      _pending.remove(id);
      rethrow;
    }
    return completer.future;
  }

  // ── Project mutation (another-one-ojm.2) ────────────────────────

  /// Add an on-disk project at `path`. Routes through
  /// `Control::AddProject` + awaits `WorkerReply::ProjectAdded` (or
  /// `WorkerReply::Err`) via the per-session request-id dispatch
  /// table. Returns `true` on a fresh insert, `false` on the
  /// duplicate-path case (matches `LocalSession::addProject`).
  /// Daemon-side "already exists" surfaces as `WorkerReply::Err`
  /// — translated back to `false` here for boolean parity; every
  /// other `Err` throws a `StateError` for the UI toast.
  @override
  Future<bool> addProject(String path) async {
    final reply = await _sendControlAndAwait(
      (id) => _session!.addProject(requestId: BigInt.from(id), path: path),
    );
    return switch (reply) {
      WorkerReply_ProjectAdded() => true,
      WorkerReply_Err(:final message) =>
        message.contains('project at this path already exists')
            ? false
            : throw StateError('addProject failed: $message'),
      _ => throw StateError(
          'addProject: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  /// Remove a project from the daemon's store by id. Idempotent —
  /// unknown ids reply with `ProjectRemoved`, no error.
  @override
  Future<void> removeProject(String projectId) async {
    final reply = await _sendControlAndAwait(
      (id) => _session!.removeProject(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
    );
    switch (reply) {
      case WorkerReply_ProjectRemoved():
        return;
      case WorkerReply_Err(:final message):
        throw StateError('removeProject failed: $message');
      default:
        throw StateError(
          'removeProject: unexpected daemon reply ${reply.runtimeType}',
        );
    }
  }

  // ── Task mutation (another-one-ojm.3) ───────────────────────────
  //
  // Each verb allocates a request_id via [_sendControlAndAwait],
  // issues the matching `Control::*` frame on the live session,
  // unwraps the `WorkerReply::*Ack`, and surfaces `WorkerReply::Err`
  // as a `StateError(message)` for the UI toast.

  @override
  Future<String> createWorktreeTask({
    required String projectId,
    required String taskName,
    required String sourceBranch,
    AgentProvider? agentProvider,
  }) async {
    final reply = await _sendControlAndAwait((id) async {
      final session = _session;
      if (session == null) {
        throw StateError('IrohTransport not connected');
      }
      await session.createWorktreeTask(
        requestId: BigInt.from(id),
        projectId: projectId,
        taskName: taskName,
        sourceBranch: sourceBranch,
        agentProvider: agentProvider,
      );
    });
    return switch (reply) {
      WorkerReply_TaskCreated(:final task) => task.sectionId,
      WorkerReply_Err(:final message) => throw StateError(message),
      _ => throw StateError(
          'createWorktreeTask: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  @override
  Future<bool> renameTask(String taskId, String newName) async {
    final reply = await _sendControlAndAwait((id) async {
      final session = _session;
      if (session == null) {
        throw StateError('IrohTransport not connected');
      }
      await session.renameTask(
        requestId: BigInt.from(id),
        taskId: taskId,
        newName: newName,
      );
    });
    return switch (reply) {
      WorkerReply_TaskRenamed(:final changed) => changed,
      WorkerReply_Err(:final message) => throw StateError(message),
      _ => throw StateError(
          'renameTask: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  @override
  Future<bool> setTaskPinned(String taskId, bool pinned) async {
    final reply = await _sendControlAndAwait((id) async {
      final session = _session;
      if (session == null) {
        throw StateError('IrohTransport not connected');
      }
      await session.setTaskPinned(
        requestId: BigInt.from(id),
        taskId: taskId,
        pinned: pinned,
      );
    });
    return switch (reply) {
      WorkerReply_TaskPinned(:final changed) => changed,
      WorkerReply_Err(:final message) => throw StateError(message),
      _ => throw StateError(
          'setTaskPinned: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  @override
  Future<bool> removeTask(String projectId, String taskId) async {
    final reply = await _sendControlAndAwait((id) async {
      final session = _session;
      if (session == null) {
        throw StateError('IrohTransport not connected');
      }
      await session.removeTask(
        requestId: BigInt.from(id),
        projectId: projectId,
        taskId: taskId,
      );
    });
    return switch (reply) {
      WorkerReply_TaskRemoved(:final removed) => removed,
      WorkerReply_Err(:final message) => throw StateError(message),
      _ => throw StateError(
          'removeTask: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  // ── Git state read verbs (`another-one-ojm.4`) ─────────────────
  //
  // Each method allocates a request_id Rust-side, registers a Dart
  // completer keyed on it, and awaits the matching reply via the
  // shared `_dispatchWorkerReplyMessage` path. On the Err frame
  // shape (`WorkerReply.err`), we throw an [IrohWireException] —
  // same surface the FRB-bound `LocalSession` exposes for its
  // anyhow errors — so caller code can `try/catch` and route to a
  // toast without inspecting variant kinds.

  @override
  Future<String> slugifyBranchName(String name) async {
    final reply = await _sendControlAndAwait(
      (id) => _session!.slugifyBranchName(
        requestId: BigInt.from(id),
        name: name,
      ),
    );
    return switch (reply) {
      WorkerReply_SlugifyBranchNameAck(:final slug) => slug,
      WorkerReply_Err(:final message, :final kind) =>
        throw IrohWireException(message: message, kind: kind),
      _ => throw StateError(
          'slugifyBranchName: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  @override
  Future<List<String>> readProjectBranches(String projectId) async {
    final reply = await _sendControlAndAwait(
      (id) => _session!.readProjectBranches(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
    );
    return switch (reply) {
      WorkerReply_ProjectBranchesAck(:final branches) => branches,
      WorkerReply_Err(:final message, :final kind) =>
        throw IrohWireException(message: message, kind: kind),
      _ => throw StateError(
          'readProjectBranches: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  @override
  Future<String?> primaryBranchForProject(String projectId) async {
    final reply = await _sendControlAndAwait(
      (id) => _session!.primaryBranchForProject(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
    );
    return switch (reply) {
      WorkerReply_PrimaryBranchAck(:final branch) => branch,
      WorkerReply_Err(:final message, :final kind) =>
        throw IrohWireException(message: message, kind: kind),
      _ => throw StateError(
          'primaryBranchForProject: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  @override
  Future<String?> repoDefaultCommitAction(String projectId) async {
    final reply = await _sendControlAndAwait(
      (id) => _session!.repoDefaultCommitAction(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
    );
    return switch (reply) {
      WorkerReply_RepoDefaultCommitActionAck(:final action) => action,
      WorkerReply_Err(:final message, :final kind) =>
        throw IrohWireException(message: message, kind: kind),
      _ => throw StateError(
          'repoDefaultCommitAction: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  @override
  Future<ls.ActiveGitStateDto?> readActiveGitState(String projectId) async {
    final reply = await _sendControlAndAwait(
      (id) => _session!.readActiveGitState(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
    );
    return switch (reply) {
      WorkerReply_ActiveGitStateAck(:final state) => state == null
          ? null
          : ls.ActiveGitStateDto(
              currentBranch: state.currentBranch,
              aheadCount: state.aheadCount,
              behindCount: state.behindCount,
            ),
      WorkerReply_Err(:final message, :final kind) =>
        throw IrohWireException(message: message, kind: kind),
      _ => throw StateError(
          'readActiveGitState: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  @override
  Future<List<ls.ChangedFileDto>?> readChangedFiles(String projectId) async {
    final reply = await _sendControlAndAwait(
      (id) => _session!.readChangedFiles(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
    );
    return switch (reply) {
      WorkerReply_ChangedFilesAck(:final files) =>
        files?.map(_changedFileWireToDto).toList(growable: false),
      WorkerReply_Err(:final message, :final kind) =>
        throw IrohWireException(message: message, kind: kind),
      _ => throw StateError(
          'readChangedFiles: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  @override
  Future<String?> readProjectGithubUrl(String projectId) async {
    final reply = await _sendControlAndAwait(
      (id) => _session!.readProjectGithubUrl(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
    );
    return switch (reply) {
      WorkerReply_ProjectGithubUrlAck(:final url) => url,
      WorkerReply_Err(:final message, :final kind) =>
        throw IrohWireException(message: message, kind: kind),
      _ => throw StateError(
          'readProjectGithubUrl: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  @override
  Future<ls.RecentCommitsView?> readRecentCommits({
    required String projectId,
    required int limit,
  }) async {
    final reply = await _sendControlAndAwait(
      (id) => _session!.readRecentCommits(
        requestId: BigInt.from(id),
        projectId: projectId,
        limit: limit,
      ),
    );
    return switch (reply) {
      WorkerReply_RecentCommitsAck(:final view) => view == null
          ? null
          : ls.RecentCommitsView(
              currentBranch: view.currentBranch,
              hasMore: view.hasMore,
              commits: view.commits.map(_commitWireToDto).toList(growable: false),
            ),
      WorkerReply_Err(:final message, :final kind) =>
        throw IrohWireException(message: message, kind: kind),
      _ => throw StateError(
          'readRecentCommits: unexpected daemon reply ${reply.runtimeType}',
        ),
    };
  }

  ls.CommitDto _commitWireToDto(CommitWire c) => ls.CommitDto(
        id: c.id,
        shortId: c.shortId,
        subject: c.subject,
        authorName: c.authorName,
        authoredRelative: c.authoredRelative,
      );

  ls.ChangedFileDto _changedFileWireToDto(ChangedFileWire f) =>
      ls.ChangedFileDto(
        path: f.path,
        originalPath: f.originalPath,
        stagedAdditions: f.stagedAdditions,
        stagedDeletions: f.stagedDeletions,
        unstagedAdditions: f.unstagedAdditions,
        unstagedDeletions: f.unstagedDeletions,
        indexStatus: f.indexStatus,
        worktreeStatus: f.worktreeStatus,
        untracked: f.untracked,
      );
}
