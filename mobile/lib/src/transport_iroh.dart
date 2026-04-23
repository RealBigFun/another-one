// Iroh implementation of TerminalTransport.
//
// Wraps the flutter_rust_bridge-generated IrohSession (see rust/api/iroh_client.dart),
// which in turn wraps an iroh::Endpoint QUIC connection to a daemon speaking
// the `anotherone/pty/0` ALPN. Data and control (resize) frames share the
// same bidirectional stream via the length-prefixed framing defined in
// daemon-sandbox/src/frame.rs — resizes are delivered, not dropped.

import 'dart:async';
import 'dart:typed_data';

import 'rust/api/iroh_client.dart';
import 'transport.dart';

class IrohTransport implements TerminalTransport {
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

  final StreamController<Uint8List> _incoming =
      StreamController<Uint8List>.broadcast();
  final StreamController<TransportStatus> _status =
      StreamController<TransportStatus>.broadcast();
  // Worker replies are domain-data pushes from the daemon (git state,
  // future: PR status, etc.) — distinct from PTY bytes in semantics and
  // rate, so they get their own broadcast stream rather than being
  // union'd into [incoming]. Null WorkerReply sentinel never flows —
  // the Rust side guarantees every `add` carries a real variant.
  final StreamController<WorkerReply> _workerReplies =
      StreamController<WorkerReply>.broadcast();

  IrohSession? _session;
  StreamSubscription<Uint8List>? _incomingSub;
  StreamSubscription<WorkerReply>? _workerRepliesSub;
  TransportStatus _current = const TransportStatus.disconnected();
  bool _closed = false;

  IrohTransport(
    this.endpointId, {
    this.directAddrs = const [],
    this.relayUrls = const [],
  });

  @override
  Stream<Uint8List> get incoming => _incoming.stream;

  @override
  Stream<TransportStatus> get status => _status.stream;

  @override
  TransportStatus get currentStatus => _current;

  /// Daemon-pushed worker replies (git state etc.) for the currently
  /// watched project. Call [watchProject] first; nothing arrives on
  /// this stream until the daemon has a subscription to forward.
  Stream<WorkerReply> get workerReplies => _workerReplies.stream;

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
        onError: (err) => _publish(TransportStatus.error(err.toString())),
        onDone: () => _publish(const TransportStatus.disconnected()),
        cancelOnError: true,
      );
      // Errors here don't tear down the transport — worker-reply
      // delivery is best-effort relative to the core PTY path.
      _workerRepliesSub = session.subscribeWorkerReplies().listen(
        _workerReplies.add,
        onError: (_) {},
      );
      _publish(const TransportStatus.connected());
    } catch (e) {
      _publish(TransportStatus.error(e.toString()));
    }
  }

  /// Ask the daemon to start forwarding git state (and later, more
  /// worker replies) for [projectPath]. Safe to call multiple times —
  /// the daemon treats each call as a fresh subscription, replacing
  /// any prior one for this session.
  void watchProject(String projectPath) {
    final session = _session;
    if (session == null) return;
    unawaited(session.watchProject(projectPath: projectPath));
  }

  /// Ask the daemon to send its project list. The response arrives on
  /// [workerReplies] as a `WorkerReply_ProjectList`. Returns a
  /// future for callers that want to surface send errors; most call
  /// sites can ignore it (the list will simply not arrive).
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
  Future<void> detachTab() async {
    final session = _session;
    if (session == null) return;
    await session.detachTab();
  }

  /// Resize the currently-attached tab's PTY. Unlike [sendResize],
  /// this targets the tab on the daemon's side, not a single-session
  /// PTY — required when the daemon is bridging into a
  /// desktop-hosted live tab.
  Future<void> tabResize({required int cols, required int rows}) async {
    final session = _session;
    if (session == null) return;
    await session.tabResize(cols: cols, rows: rows);
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
}
