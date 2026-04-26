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
import 'transport.dart';

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
      _workerRepliesSub = session.subscribeWorkerReplies().listen(
        _workerReplies.add,
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
