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
  /// reply. Domain verbs landing in `another-one-ojm.2..8` will use
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
  /// First adopters: the git-state read verbs landed in
  /// `another-one-ojm.4`. Each Rust-side helper on `IrohSession`
  /// (e.g. `slugifyBranchName`) returns the freshly-allocated
  /// `request_id`; the Dart layer registers a completer keyed on it
  /// after the FRB call resolves and the recv loop dispatches the
  /// matching reply via [_dispatchWorkerReplyMessage].
  Future<WorkerReply> _sendControlAndAwait(
    Future<BigInt> Function() send,
  ) async {
    final session = _session;
    if (session == null) {
      throw StateError('IrohTransport not connected');
    }
    final completer = Completer<WorkerReply>();
    final BigInt rawId = await send();
    final id = rawId.toInt();
    // The daemon may have replied between `send()` resolving and
    // this line — the recv loop dispatches via [_pending] so a
    // completer that doesn't exist yet means the reply is dropped.
    // The race window is just a few microseconds (the FRB return
    // path) versus the QUIC + daemon round-trip dominating the
    // timeline; if it ever bites in practice we'll switch to a
    // "register-then-send" pair that takes the id as an argument.
    _pending[id] = completer;
    return completer.future;
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
      () => _session!.slugifyBranchName(name: name),
    );
    return reply.maybeWhen(
      slugifyBranchNameAck: (slug) => slug,
      err: _throwErr,
      orElse: () => throw StateError(
        'slugifyBranchName: unexpected reply variant ${reply.runtimeType}',
      ),
    );
  }

  /// Common Err-frame handler used by every read verb override.
  /// Throws so the caller's `try/catch` (or Riverpod async value's
  /// error state) sees a thrown failure rather than a typed `null`
  /// — same surface FRB exposes for `LocalSession`'s
  /// `anyhow::Result` errors.
  Never _throwErr(String message, ErrKind kind) {
    throw IrohWireException(message: message, kind: kind);
  }
}
