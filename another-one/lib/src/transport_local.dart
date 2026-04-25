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
import 'rust/api/iroh_client.dart' show WorkerReply;
import 'rust/api/local_session.dart';
import 'transport.dart';

class LocalTransport implements TerminalTransport, DaemonConnection {
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
  Future<bool> addProject(String path) async {
    final session = _session;
    if (session == null) {
      throw StateError('addProject: LocalTransport not connected');
    }
    return session.addProject(path: path);
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
