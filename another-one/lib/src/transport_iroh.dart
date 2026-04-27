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

  /// Optional human label that overrides the truncated-hex default.
  /// The desktop's loopback bootstrap sets this to `"Local"` so the
  /// host switcher renders the same label the legacy LocalTransport
  /// did. Null on a mobile pair flow where the truncated EndpointId
  /// is the right thing to show until a friendly name is recorded.
  final String? displayNameOverride;

  IrohTransport(
    this.endpointId, {
    this.directAddrs = const [],
    this.relayUrls = const [],
    this.pairToken,
    this.displayNameOverride,
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

  /// Renders [displayNameOverride] when set (loopback bootstrap shows
  /// `"Local"`), otherwise the truncated endpoint id. Future: pull a
  /// human-set name from `paired_peers` so the UI shows
  /// `"laptop@home"` instead of hex.
  @override
  String get displayName =>
      displayNameOverride ??
      (endpointId.length > 12 ? '${endpointId.substring(0, 12)}…' : endpointId);

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

  /// Project/task mutation acks only carry the changed entity, while
  /// desktop state derives selection, sidebars, and active project
  /// context from the full `ProjectList` snapshot. Queue a best-effort
  /// refresh after successful mutations so those surfaces rehydrate
  /// from authoritative daemon state without every widget having to
  /// remember a follow-up `listProjects()` call.
  void _queueProjectListRefresh() {
    unawaited(listProjects().catchError((_) {}));
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

  /// Resolve the latest pull-request status for `projectId`'s
  /// current branch over the iroh wire.
  @override
  Future<ls.PullRequestStatusDto?> findPullRequestStatus(
    String projectId,
  ) async {
    return _callStateAck<
      ls.PullRequestStatusDto?,
      WorkerReply_PullRequestStatusAck
    >(
      verb: 'findPullRequestStatus',
      send: (requestId) => _session!.findPullRequestStatus(
        requestId: BigInt.from(requestId),
        projectId: projectId,
      ),
      mapAck: (ack) => ack.status,
    );
  }

  /// CI checks attached to `projectId`'s current PR.
  @override
  Future<List<ls.CheckDto>?> readPullRequestChecks(String projectId) async {
    return _callStateAck<List<ls.CheckDto>?, WorkerReply_PullRequestChecksAck>(
      verb: 'readPullRequestChecks',
      send: (requestId) => _session!.readPullRequestChecks(
        requestId: BigInt.from(requestId),
        projectId: projectId,
      ),
      mapAck: (ack) => ack.checks,
    );
  }

  /// Open pull requests for `projectId` filtered by `filterIndex`.
  @override
  Future<List<ls.ProjectPagePullRequestDto>?> findProjectPullRequests({
    required String projectId,
    required int filterIndex,
    required String query,
  }) async {
    return _callStateAck<
      List<ls.ProjectPagePullRequestDto>?,
      WorkerReply_ProjectPullRequestsAck
    >(
      verb: 'findProjectPullRequests',
      send: (requestId) => _session!.findProjectPullRequests(
        requestId: BigInt.from(requestId),
        projectId: projectId,
        filterIndex: filterIndex,
        query: query,
      ),
      mapAck: (ack) => ack.prs,
    );
  }

  // ── Custom actions + Open In + agents read verbs (`another-one-ojm.7`) ───

  /// Snapshot the host's "Open In" config.
  @override
  Future<ls.OpenInState> openInState() async {
    return _callStateAck<ls.OpenInState, WorkerReply_OpenInStateAck>(
      verb: 'openInState',
      send: (requestId) =>
          _session!.openInState(requestId: BigInt.from(requestId)),
      mapAck: (ack) => ack.state,
    );
  }

  /// Project + global custom actions for `projectId`.
  @override
  Future<List<ls.ProjectActionDto>> listProjectActions(String projectId) async {
    return _callStateAck<
      List<ls.ProjectActionDto>,
      WorkerReply_ProjectActionsAck
    >(
      verb: 'listProjectActions',
      send: (requestId) => _session!.listProjectActions(
        requestId: BigInt.from(requestId),
        projectId: projectId,
      ),
      mapAck: (ack) => ack.actions,
    );
  }

  /// Snapshot of agents the user has enabled on this host.
  @override
  Future<ls.EnabledAgentsView> readEnabledAgents() async {
    return _callStateAck<ls.EnabledAgentsView, WorkerReply_EnabledAgentsAck>(
      verb: 'readEnabledAgents',
      send: (requestId) =>
          _session!.readEnabledAgents(requestId: BigInt.from(requestId)),
      mapAck: (ack) => ack.view,
    );
  }

  /// Submit the new-task modal over the shared iroh wire.
  @override
  Future<String> submitNewTask({
    required String projectId,
    required String taskName,
    required String sourceBranch,
    required List<String> agentIds,
    required bool branchModeExisting,
    required bool worktreeMode,
  }) async {
    return _callWireAck<String, WorkerReply_SubmitNewTaskAck>(
      verb: 'submitNewTask',
      send: (requestId) => _session!.submitNewTask(
        requestId: BigInt.from(requestId),
        projectId: projectId,
        taskName: taskName,
        sourceBranch: sourceBranch,
        agentIds: agentIds,
        branchModeExisting: branchModeExisting,
        worktreeMode: worktreeMode,
      ),
      mapAck: (ack) => ack.sectionId,
    );
  }

  @override
  Future<String> addAgentToSection({
    required String sectionId,
    required String agentId,
  }) async {
    return _callWireAck<String, WorkerReply_AddAgentToSectionAck>(
      verb: 'addAgentToSection',
      send: (requestId) => _session!.addAgentToSection(
        requestId: BigInt.from(requestId),
        sectionId: sectionId,
        agentId: agentId,
      ),
      mapAck: (ack) => ack.tabId,
    );
  }

  @override
  Future<void> activateSectionTab({
    required String sectionId,
    required String tabId,
  }) async {
    return _callWireVoid<WorkerReply_ActivateSectionTabAck>(
      verb: 'activateSectionTab',
      send: (requestId) => _session!.activateSectionTab(
        requestId: BigInt.from(requestId),
        sectionId: sectionId,
        tabId: tabId,
      ),
    );
  }

  @override
  Future<String> closeSectionTab({
    required String sectionId,
    required String tabId,
  }) async {
    return _callWireAck<String, WorkerReply_CloseSectionTabAck>(
      verb: 'closeSectionTab',
      send: (requestId) => _session!.closeSectionTab(
        requestId: BigInt.from(requestId),
        sectionId: sectionId,
        tabId: tabId,
      ),
      mapAck: (ack) => ack.activeTabId,
    );
  }

  @override
  Future<bool> toggleSectionTabPinned({
    required String sectionId,
    required String tabId,
  }) async {
    return _callWireAck<bool, WorkerReply_ToggleSectionTabPinnedAck>(
      verb: 'toggleSectionTabPinned',
      send: (requestId) => _session!.toggleSectionTabPinned(
        requestId: BigInt.from(requestId),
        sectionId: sectionId,
        tabId: tabId,
      ),
      mapAck: (ack) => ack.pinned,
    );
  }

  /// Full agent registry — drives the Settings → Agents page.
  @override
  Future<ls.AgentSettingsView> readAgentSettings() async {
    return _callStateAck<ls.AgentSettingsView, WorkerReply_AgentSettingsAck>(
      verb: 'readAgentSettings',
      send: (requestId) =>
          _session!.readAgentSettings(requestId: BigInt.from(requestId)),
      mapAck: (ack) => ack.view,
    );
  }

  @override
  Future<bool> setAgentEnabled({
    required String agentId,
    required bool enabled,
  }) async {
    return _callWireAck<bool, WorkerReply_SetAgentEnabledAck>(
      verb: 'setAgentEnabled',
      send: (requestId) => _session!.setAgentEnabled(
        requestId: BigInt.from(requestId),
        agentId: agentId,
        enabled: enabled,
      ),
      mapAck: (ack) => ack.changed,
    );
  }

  @override
  Future<bool> setDefaultAgent(String agentId) async {
    return _callWireAck<bool, WorkerReply_SetDefaultAgentAck>(
      verb: 'setDefaultAgent',
      send: (requestId) => _session!.setDefaultAgent(
        requestId: BigInt.from(requestId),
        agentId: agentId,
      ),
      mapAck: (ack) => ack.changed,
    );
  }

  @override
  Future<bool> setAgentLaunchArgs({
    required String agentId,
    required List<String> args,
  }) async {
    return _callWireAck<bool, WorkerReply_SetAgentLaunchArgsAck>(
      verb: 'setAgentLaunchArgs',
      send: (requestId) => _session!.setAgentLaunchArgs(
        requestId: BigInt.from(requestId),
        agentId: agentId,
        args: args,
      ),
      mapAck: (ack) => ack.changed,
    );
  }

  /// Settings → Open In snapshot from the daemon host.
  @override
  Future<ls.OpenInSettingsView> readOpenInSettings() async {
    return _callWireAck<ls.OpenInSettingsView, WorkerReply_OpenInSettingsAck>(
      verb: 'readOpenInSettings',
      send: (requestId) =>
          _session!.readOpenInSettings(requestId: BigInt.from(requestId)),
      mapAck: (ack) => ack.view,
    );
  }

  /// Toggle one daemon-host Open-In app.
  @override
  Future<void> setOpenInAppEnabled({
    required String appId,
    required bool enabled,
  }) async {
    return _callWireVoid<WorkerReply_SetOpenInAppEnabledAck>(
      verb: 'setOpenInAppEnabled',
      send: (requestId) => _session!.setOpenInAppEnabled(
        requestId: BigInt.from(requestId),
        appId: appId,
        enabled: enabled,
      ),
    );
  }

  /// Launch `projectId` in `appId` on the daemon host.
  @override
  Future<void> openProjectInApp({
    required String projectId,
    required String appId,
  }) async {
    return _callWireVoid<WorkerReply_OpenProjectInAppAck>(
      verb: 'openProjectInApp',
      send: (requestId) => _session!.openProjectInApp(
        requestId: BigInt.from(requestId),
        projectId: projectId,
        appId: appId,
      ),
    );
  }

  /// Run one custom action inside `sectionId`'s task.
  @override
  Future<String> runProjectAction({
    required String projectId,
    required String sectionId,
    required String actionId,
  }) async {
    return _callStateAck<String, WorkerReply_RunProjectActionAck>(
      verb: 'runProjectAction',
      send: (requestId) => _session!.runProjectAction(
        requestId: BigInt.from(requestId),
        projectId: projectId,
        sectionId: sectionId,
        actionId: actionId,
      ),
      mapAck: (ack) => ack.tabId,
    );
  }

  @override
  Future<void> saveProjectAction({
    required String projectId,
    required ls.ProjectActionDto action,
    required bool saveGlobalCopy,
  }) async {
    return _callWireVoid<WorkerReply_SaveProjectActionAck>(
      verb: 'saveProjectAction',
      send: (requestId) => _session!.saveProjectAction(
        requestId: BigInt.from(requestId),
        projectId: projectId,
        action: action,
        saveGlobalCopy: saveGlobalCopy,
      ),
    );
  }

  @override
  Future<bool> deleteProjectAction({
    required String projectId,
    required String actionId,
  }) async {
    return _callWireAck<bool, WorkerReply_DeleteProjectActionAck>(
      verb: 'deleteProjectAction',
      send: (requestId) => _session!.deleteProjectAction(
        requestId: BigInt.from(requestId),
        projectId: projectId,
        actionId: actionId,
      ),
      mapAck: (ack) => ack.deleted,
    );
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
  /// this in place of fire-and-forget `session.foo()` so per-call
  /// replies (acks, error frames) land at the awaiting caller via
  /// the completer table — see `_dispatchWorkerReplyMessage`.
  ///
  /// `send` is the caller-supplied closure that performs the actual
  /// FRB call once the request_id has been registered in the
  /// dispatch map — taking it as a closure means each verb decides
  /// which `Control::*` variant + arguments to encode without this
  /// helper having to know about every variant. Callers receive the
  /// `request_id` so they can pass it to a Rust-side `send_control`
  /// equivalent (added per-verb in domain tasks). Each domain verb
  /// landing in `another-one-ojm.2..8` routes its reply through this
  /// helper.
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

  Future<R> _callAck<R, A extends WorkerReply>({
    required String verb,
    required Future<void> Function(int requestId) send,
    required R Function(A ack) mapAck,
    required R Function(WorkerReply_Err err) onErr,
  }) async {
    final reply = await _sendControlAndAwait(send);
    if (reply is A) {
      return mapAck(reply);
    }
    if (reply is WorkerReply_Err) {
      return onErr(reply);
    }
    _unexpectedReply(verb, reply);
  }

  Future<R> _callStateAck<R, A extends WorkerReply>({
    required String verb,
    required Future<void> Function(int requestId) send,
    required R Function(A ack) mapAck,
  }) {
    return _callAck<R, A>(
      verb: verb,
      send: send,
      mapAck: mapAck,
      onErr: _throwStateErr,
    );
  }

  Future<R> _callWireAck<R, A extends WorkerReply>({
    required String verb,
    required Future<void> Function(int requestId) send,
    required R Function(A ack) mapAck,
  }) {
    return _callAck<R, A>(
      verb: verb,
      send: send,
      mapAck: mapAck,
      onErr: _throwForErr,
    );
  }

  Future<void> _callStateVoid<A extends WorkerReply>({
    required String verb,
    required Future<void> Function(int requestId) send,
  }) {
    return _callStateAck<void, A>(verb: verb, send: send, mapAck: (_) {});
  }

  Future<void> _callWireVoid<A extends WorkerReply>({
    required String verb,
    required Future<void> Function(int requestId) send,
  }) {
    return _callWireAck<void, A>(verb: verb, send: send, mapAck: (_) {});
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
    final inserted = await _callAck<bool, WorkerReply_ProjectAdded>(
      verb: 'addProject',
      send: (id) =>
          _session!.addProject(requestId: BigInt.from(id), path: path),
      mapAck: (_) => true,
      onErr: (err) =>
          err.message.contains('project at this path already exists')
          ? false
          : throw StateError('addProject failed: ${err.message}'),
    );
    if (inserted) {
      _queueProjectListRefresh();
    }
    return inserted;
  }

  /// Remove a project from the daemon's store by id. Idempotent —
  /// unknown ids reply with `ProjectRemoved`, no error.
  @override
  Future<void> removeProject(String projectId) async {
    await _callAck<void, WorkerReply_ProjectRemoved>(
      verb: 'removeProject',
      send: (id) => _session!.removeProject(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
      mapAck: (_) {},
      onErr: (err) => throw StateError('removeProject failed: ${err.message}'),
    );
    _queueProjectListRefresh();
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
    return _callStateAck<String, WorkerReply_TaskCreated>(
      verb: 'createWorktreeTask',
      send: (id) async {
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
      },
      mapAck: (ack) => ack.task.sectionId,
    );
  }

  @override
  Future<bool> renameTask(String taskId, String newName) async {
    final changed = await _callStateAck<bool, WorkerReply_TaskRenamed>(
      verb: 'renameTask',
      send: (id) async {
        final session = _session;
        if (session == null) {
          throw StateError('IrohTransport not connected');
        }
        await session.renameTask(
          requestId: BigInt.from(id),
          taskId: taskId,
          newName: newName,
        );
      },
      mapAck: (ack) => ack.changed,
    );
    if (changed) {
      _queueProjectListRefresh();
    }
    return changed;
  }

  @override
  Future<bool> setTaskPinned(String taskId, bool pinned) async {
    final changed = await _callStateAck<bool, WorkerReply_TaskPinned>(
      verb: 'setTaskPinned',
      send: (id) async {
        final session = _session;
        if (session == null) {
          throw StateError('IrohTransport not connected');
        }
        await session.setTaskPinned(
          requestId: BigInt.from(id),
          taskId: taskId,
          pinned: pinned,
        );
      },
      mapAck: (ack) => ack.changed,
    );
    if (changed) {
      _queueProjectListRefresh();
    }
    return changed;
  }

  @override
  Future<bool> removeTask(String projectId, String taskId) async {
    final removed = await _callStateAck<bool, WorkerReply_TaskRemoved>(
      verb: 'removeTask',
      send: (id) async {
        final session = _session;
        if (session == null) {
          throw StateError('IrohTransport not connected');
        }
        await session.removeTask(
          requestId: BigInt.from(id),
          projectId: projectId,
          taskId: taskId,
        );
      },
      mapAck: (ack) => ack.removed,
    );
    if (removed) {
      _queueProjectListRefresh();
    }
    return removed;
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
    return _callWireAck<String, WorkerReply_SlugifyBranchNameAck>(
      verb: 'slugifyBranchName',
      send: (id) =>
          _session!.slugifyBranchName(requestId: BigInt.from(id), name: name),
      mapAck: (ack) => ack.slug,
    );
  }

  @override
  Future<List<String>> readProjectBranches(String projectId) async {
    return _callWireAck<List<String>, WorkerReply_ProjectBranchesAck>(
      verb: 'readProjectBranches',
      send: (id) => _session!.readProjectBranches(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
      mapAck: (ack) => ack.branches,
    );
  }

  @override
  Future<String?> primaryBranchForProject(String projectId) async {
    return _callWireAck<String?, WorkerReply_PrimaryBranchAck>(
      verb: 'primaryBranchForProject',
      send: (id) => _session!.primaryBranchForProject(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
      mapAck: (ack) => ack.branch,
    );
  }

  @override
  Future<String?> repoDefaultCommitAction(String projectId) async {
    return _callWireAck<String?, WorkerReply_RepoDefaultCommitActionAck>(
      verb: 'repoDefaultCommitAction',
      send: (id) => _session!.repoDefaultCommitAction(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
      mapAck: (ack) => ack.action,
    );
  }

  @override
  Future<ls.ActiveGitStateDto?> readActiveGitState(String projectId) async {
    return _callWireAck<ls.ActiveGitStateDto?, WorkerReply_ActiveGitStateAck>(
      verb: 'readActiveGitState',
      send: (id) => _session!.readActiveGitState(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
      mapAck: (ack) => ack.state,
    );
  }

  @override
  Future<List<ls.ChangedFileDto>?> readChangedFiles(String projectId) async {
    return _callWireAck<List<ls.ChangedFileDto>?, WorkerReply_ChangedFilesAck>(
      verb: 'readChangedFiles',
      send: (id) => _session!.readChangedFiles(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
      mapAck: (ack) => ack.files?.cast<ls.ChangedFileDto>(),
    );
  }

  @override
  Future<String?> readProjectGithubUrl(String projectId) async {
    return _callWireAck<String?, WorkerReply_ProjectGithubUrlAck>(
      verb: 'readProjectGithubUrl',
      send: (id) => _session!.readProjectGithubUrl(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
      mapAck: (ack) => ack.url,
    );
  }

  @override
  Future<ls.RecentCommitsView?> readRecentCommits({
    required String projectId,
    required int limit,
  }) async {
    return _callWireAck<ls.RecentCommitsView?, WorkerReply_RecentCommitsAck>(
      verb: 'readRecentCommits',
      send: (id) => _session!.readRecentCommits(
        requestId: BigInt.from(id),
        projectId: projectId,
        limit: limit,
      ),
      mapAck: (ack) => ack.view,
    );
  }

  @override
  Future<List<ls.BranchCompareFileDto>?> readCommitFileChanges({
    required String projectId,
    required String commitId,
  }) async {
    return _callWireAck<
      List<ls.BranchCompareFileDto>?,
      WorkerReply_CommitFileChangesAck
    >(
      verb: 'readCommitFileChanges',
      send: (id) => _session!.readCommitFileChanges(
        requestId: BigInt.from(id),
        projectId: projectId,
        commitId: commitId,
      ),
      mapAck: (ack) => ack.files?.cast<ls.BranchCompareFileDto>(),
    );
  }

  @override
  Future<ls.BranchCompareView?> readBranchCompareState({
    required String projectId,
    required String targetBranch,
  }) async {
    return _callWireAck<ls.BranchCompareView?, WorkerReply_BranchCompareAck>(
      verb: 'readBranchCompareState',
      send: (id) => _session!.readBranchCompareState(
        requestId: BigInt.from(id),
        projectId: projectId,
        targetBranch: targetBranch,
      ),
      mapAck: (ack) => ack.view,
    );
  }

  @override
  Future<ls.ResolvedProjectBranchSettingsDto?> readBranchSettings(
    String projectId,
  ) async {
    return _callWireAck<
      ls.ResolvedProjectBranchSettingsDto?,
      WorkerReply_BranchSettingsAck
    >(
      verb: 'readBranchSettings',
      send: (id) => _session!.readBranchSettings(
        requestId: BigInt.from(id),
        projectId: projectId,
      ),
      mapAck: (ack) => ack.settings,
    );
  }

  @override
  Future<bool> setBranchSetting({
    required String projectId,
    required String field,
    String? branchName,
  }) async {
    return _callWireAck<bool, WorkerReply_SetBranchSettingAck>(
      verb: 'setBranchSetting',
      send: (id) => _session!.setBranchSetting(
        requestId: BigInt.from(id),
        projectId: projectId,
        field: field,
        branchName: branchName,
      ),
      mapAck: (ack) => ack.changed,
    );
  }

  // ── Git mutation verbs (`another-one-ojm.5`) ───────────────────
  //
  // Each method allocates a request_id via [_sendControlAndAwait],
  // issues the matching `Control::*` frame, and unwraps the
  // `WorkerReply::*Ack`. On `WorkerReply::Err`, throws
  // [IrohWireException] so callers can `try/catch` and route to a
  // toast.

  Never _throwStateErr(WorkerReply_Err err) {
    throw StateError(err.message);
  }

  /// Map a `WorkerReply.err` payload to a thrown exception.
  Never _throwForErr(WorkerReply_Err err) {
    throw IrohWireException(message: err.message, kind: err.kind);
  }

  List<ls.ChangedFileDto> _changedFilesFromReply(
    WorkerReply reply, {
    required String verb,
  }) {
    return switch (reply) {
      WorkerReply_StageChangedFileAck(:final changedFiles) =>
        changedFiles.cast<ls.ChangedFileDto>(),
      WorkerReply_UnstageChangedFileAck(:final changedFiles) =>
        changedFiles.cast<ls.ChangedFileDto>(),
      WorkerReply_StageAllChangesAck(:final changedFiles) =>
        changedFiles.cast<ls.ChangedFileDto>(),
      WorkerReply_UnstageAllChangesAck(:final changedFiles) =>
        changedFiles.cast<ls.ChangedFileDto>(),
      WorkerReply_DiscardChangedFileAck(:final changedFiles) =>
        changedFiles.cast<ls.ChangedFileDto>(),
      WorkerReply_Err(:final message, :final kind) => _throwForErr(
        WorkerReply_Err(message: message, kind: kind),
      ),
      _ => throw StateError('$verb: unexpected reply variant $reply'),
    };
  }

  DiscardAllChangesResult _discardAllChangesFromReply(WorkerReply reply) {
    return switch (reply) {
      WorkerReply_DiscardAllChangesAck(:final changedFiles, :final failures) =>
        DiscardAllChangesResult(
          changedFiles: changedFiles.cast<ls.ChangedFileDto>(),
          failures: failures,
        ),
      WorkerReply_Err(:final message, :final kind) => _throwForErr(
        WorkerReply_Err(message: message, kind: kind),
      ),
      _ => throw StateError(
        'discardAllChanges: unexpected reply variant ${reply.runtimeType}',
      ),
    };
  }

  /// `another-one-ojm.5` — stage one changed file via the iroh wire.
  @override
  Future<List<ls.ChangedFileDto>> stageChangedFile({
    required String projectId,
    required String path,
    String? originalPath,
  }) async {
    final reply = await _sendControlAndAwait((id) async {
      await _session!.stageChangedFile(
        requestId: BigInt.from(id),
        projectId: projectId,
        path: path,
        originalPath: originalPath,
      );
    });
    return _changedFilesFromReply(reply, verb: 'stageChangedFile');
  }

  /// `another-one-ojm.5` — unstage one changed file. Same shape as
  /// [`stageChangedFile`] but `Control::UnstageChangedFile` →
  /// `WorkerReply::UnstageChangedFileAck`.
  @override
  Future<List<ls.ChangedFileDto>> unstageChangedFile({
    required String projectId,
    required String path,
    String? originalPath,
  }) async {
    final reply = await _sendControlAndAwait((id) async {
      await _session!.unstageChangedFile(
        requestId: BigInt.from(id),
        projectId: projectId,
        path: path,
        originalPath: originalPath,
      );
    });
    return _changedFilesFromReply(reply, verb: 'unstageChangedFile');
  }

  /// `another-one-ojm.5` — `git add -A` over the iroh wire.
  @override
  Future<List<ls.ChangedFileDto>> stageAllChanges(String projectId) async {
    final reply = await _sendControlAndAwait((id) async {
      await _session!.stageAllChanges(
        requestId: BigInt.from(id),
        projectId: projectId,
      );
    });
    return _changedFilesFromReply(reply, verb: 'stageAllChanges');
  }

  /// `another-one-ojm.5` — `git restore --staged -- .` over the
  /// iroh wire.
  @override
  Future<List<ls.ChangedFileDto>> unstageAllChanges(String projectId) async {
    final reply = await _sendControlAndAwait((id) async {
      await _session!.unstageAllChanges(
        requestId: BigInt.from(id),
        projectId: projectId,
      );
    });
    return _changedFilesFromReply(reply, verb: 'unstageAllChanges');
  }

  /// `another-one-ojm.5` — discard one file's working-tree changes
  /// over the iroh wire. Destructive — the calling UI gates this
  /// behind a confirmation modal before invoking.
  @override
  Future<List<ls.ChangedFileDto>> discardChangedFile({
    required String projectId,
    required String path,
    required bool untracked,
    String? originalPath,
  }) async {
    final reply = await _sendControlAndAwait((id) async {
      await _session!.discardChangedFile(
        requestId: BigInt.from(id),
        projectId: projectId,
        path: path,
        untracked: untracked,
        originalPath: originalPath,
      );
    });
    return _changedFilesFromReply(reply, verb: 'discardChangedFile');
  }

  @override
  Future<DiscardAllChangesResult> discardAllChanges({
    required String projectId,
    required List<ls.ChangedFileDto> files,
  }) async {
    final reply = await _sendControlAndAwait((id) async {
      await _session!.discardAllChanges(
        requestId: BigInt.from(id),
        projectId: projectId,
        files: files,
      );
    });
    return _discardAllChangesFromReply(reply);
  }

  /// `another-one-ojm.5` — run one of the titlebar git actions over
  /// the iroh wire. The returned `ls.ToolbarActionOutcomeDto` matches
  /// what `LocalTransport` produces, so the titlebar's snackbar
  /// rendering doesn't have to branch on transport.
  @override
  Future<ls.ToolbarActionOutcomeDto> runToolbarGitAction({
    required String projectId,
    required String actionId,
  }) async {
    return _callWireAck<
      ls.ToolbarActionOutcomeDto,
      WorkerReply_ToolbarActionOutcomeAck
    >(
      verb: 'runToolbarGitAction',
      send: (id) => _session!.runToolbarGitAction(
        requestId: BigInt.from(id),
        projectId: projectId,
        actionId: actionId,
      ),
      mapAck: (ack) => ls.ToolbarActionOutcomeDto(
        toastMessage: ack.outcome.toastMessage,
        warning: ack.outcome.warning,
        refreshGitState: ack.outcome.refreshGitState,
      ),
    );
  }

  /// `another-one-ojm.5` — create a branch from HEAD over the iroh
  /// wire. Returns the new worktree task's `sectionId` (empty string
  /// in current-task mode) so the caller can navigate to it. The
  /// ack also carries the post-mutation `projects` snapshot, which
  /// today the transport discards; consuming it in-band (e.g.
  /// pushing into [workerReplies]) is a follow-up hook.
  @override
  Future<String> createBranch({
    required String projectId,
    required String branchName,
    required bool useCurrentTask,
    required bool migrateChanges,
  }) async {
    return _callWireAck<String, WorkerReply_CreateBranchAck>(
      verb: 'createBranch',
      send: (id) => _session!.createBranch(
        requestId: BigInt.from(id),
        projectId: projectId,
        branchName: branchName,
        useCurrentTask: useCurrentTask,
        migrateChanges: migrateChanges,
      ),
      mapAck: (ack) {
        // Re-broadcast the post-mutation project snapshot so any
        // listener on `workerReplies` (the projects drawer's
        // FRB-side consumer) repaints in the same round-trip the
        // ack carried. Mirrors LocalSession::create_branch which
        // calls `self.list_projects()` after the mutation.
        if (!_workerReplies.isClosed) {
          _workerReplies.add(WorkerReply.projectList(projects: ack.projects));
        }
        return ack.sectionId;
      },
    );
  }

  /// `another-one-ojm.5` — spawn a review task for a PR over the iroh
  /// wire. Returns the new task's `sectionId` and re-broadcasts the
  /// post-mutation `projects` snapshot on [workerReplies] (same shape
  /// as [createBranch]).
  @override
  Future<String> createReviewTask({
    required String projectId,
    required int pullRequestNumber,
    required String headBranch,
    AgentProvider? agentProvider,
  }) async {
    return _callWireAck<String, WorkerReply_CreateReviewTaskAck>(
      verb: 'createReviewTask',
      send: (id) => _session!.createReviewTask(
        requestId: BigInt.from(id),
        projectId: projectId,
        pullRequestNumber: BigInt.from(pullRequestNumber),
        headBranch: headBranch,
        agentProvider: agentProvider,
      ),
      mapAck: (ack) {
        if (!_workerReplies.isClosed) {
          _workerReplies.add(WorkerReply.projectList(projects: ack.projects));
        }
        return ack.sectionId;
      },
    );
  }

  /// Throw a developer-friendly error for a settings reply that
  /// arrived in an unexpected variant.
  Never _unexpectedReply(String verb, WorkerReply reply) {
    if (reply is WorkerReply_Err) {
      throw StateError(
        '$verb failed on the daemon: ${reply.message} '
        '(err_kind=${reply.kind.name})',
      );
    }
    throw StateError(
      '$verb received unexpected WorkerReply variant '
      '(${reply.runtimeType})',
    );
  }

  // ── Settings → Git Actions (`another-one-ojm.8`) ────────────────

  @override
  Future<ls.GitActionScriptsView> readGitActionScripts() async {
    return _callStateAck<
      ls.GitActionScriptsView,
      WorkerReply_GitActionScriptsAck
    >(
      verb: 'readGitActionScripts',
      send: (id) => _session!.readGitActionScripts(requestId: BigInt.from(id)),
      mapAck: (ack) => ack.view,
    );
  }

  @override
  Future<bool> setGitCommitScript(String script) async {
    return _callStateAck<bool, WorkerReply_SetGitCommitScriptAck>(
      verb: 'setGitCommitScript',
      send: (id) => _session!.setGitCommitScript(
        requestId: BigInt.from(id),
        script: script,
      ),
      mapAck: (ack) => ack.changed,
    );
  }

  @override
  Future<bool> resetGitCommitScript() async {
    return _callStateAck<bool, WorkerReply_ResetGitCommitScriptAck>(
      verb: 'resetGitCommitScript',
      send: (id) => _session!.resetGitCommitScript(requestId: BigInt.from(id)),
      mapAck: (ack) => ack.changed,
    );
  }

  @override
  Future<bool> setGitPrScript(String script) async {
    return _callStateAck<bool, WorkerReply_SetGitPrScriptAck>(
      verb: 'setGitPrScript',
      send: (id) =>
          _session!.setGitPrScript(requestId: BigInt.from(id), script: script),
      mapAck: (ack) => ack.changed,
    );
  }

  @override
  Future<bool> resetGitPrScript() async {
    return _callStateAck<bool, WorkerReply_ResetGitPrScriptAck>(
      verb: 'resetGitPrScript',
      send: (id) => _session!.resetGitPrScript(requestId: BigInt.from(id)),
      mapAck: (ack) => ack.changed,
    );
  }

  // ── Settings → Keybindings (`another-one-ojm.8`) ────────────────

  @override
  Future<ls.ShortcutSettingsView> readShortcutSettings() async {
    return _callStateAck<
      ls.ShortcutSettingsView,
      WorkerReply_ShortcutSettingsAck
    >(
      verb: 'readShortcutSettings',
      send: (id) => _session!.readShortcutSettings(requestId: BigInt.from(id)),
      mapAck: (ack) => ack.view,
    );
  }

  @override
  Future<void> setShortcutBinding({
    required String actionId,
    required String binding,
  }) async {
    return _callStateVoid<WorkerReply_SetShortcutBindingAck>(
      verb: 'setShortcutBinding',
      send: (id) => _session!.setShortcutBinding(
        requestId: BigInt.from(id),
        actionId: actionId,
        binding: binding,
      ),
    );
  }

  @override
  Future<void> resetShortcutBinding(String actionId) async {
    return _callStateVoid<WorkerReply_ResetShortcutBindingAck>(
      verb: 'resetShortcutBinding',
      send: (id) => _session!.resetShortcutBinding(
        requestId: BigInt.from(id),
        actionId: actionId,
      ),
    );
  }

  // ── Settings → MCP (`another-one-ojm.8`) ────────────────────────

  @override
  Future<ls.McpSettingsView> readMcpSettings() async {
    return _callStateAck<ls.McpSettingsView, WorkerReply_McpSettingsAck>(
      verb: 'readMcpSettings',
      send: (id) => _session!.readMcpSettings(requestId: BigInt.from(id)),
      mapAck: (ack) => ack.view,
    );
  }

  @override
  Future<void> mcpAddFromCatalog(String catalogId) async {
    return _callStateVoid<WorkerReply_McpAddFromCatalogAck>(
      verb: 'mcpAddFromCatalog',
      send: (id) => _session!.mcpAddFromCatalog(
        requestId: BigInt.from(id),
        catalogId: catalogId,
      ),
    );
  }

  @override
  Future<void> mcpToggle({
    required String entryId,
    required String providerId,
    required bool enabled,
  }) async {
    return _callStateVoid<WorkerReply_McpToggleAck>(
      verb: 'mcpToggle',
      send: (id) => _session!.mcpToggle(
        requestId: BigInt.from(id),
        entryId: entryId,
        providerId: providerId,
        enabled: enabled,
      ),
    );
  }

  @override
  Future<void> mcpRemove(String entryId) async {
    return _callStateVoid<WorkerReply_McpRemoveAck>(
      verb: 'mcpRemove',
      send: (id) =>
          _session!.mcpRemove(requestId: BigInt.from(id), entryId: entryId),
    );
  }
}
