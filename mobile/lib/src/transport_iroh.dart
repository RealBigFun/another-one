// Iroh implementation of TerminalTransport.
//
// Wraps the flutter_rust_bridge-generated IrohSession (see rust/api/iroh_client.dart),
// which in turn wraps an iroh::Endpoint QUIC connection to a daemon speaking
// the `anotherone/pty/0` ALPN. Resize messages are currently dropped because
// the daemon's Iroh path doesn't frame control yet; that'll be added when
// the daemon-side protocol is wired up.

import 'dart:async';
import 'dart:typed_data';

import 'rust/api/iroh_client.dart';
import 'transport.dart';

class IrohTransport implements TerminalTransport {
  /// Hex-encoded EndpointId of the daemon to dial.
  final String endpointId;

  /// Direct `host:port` socket addresses of the daemon. The sandbox doesn't
  /// ship an address-lookup service, so the client must know at least one
  /// direct address to dial.
  final List<String> directAddrs;

  final StreamController<Uint8List> _incoming =
      StreamController<Uint8List>.broadcast();
  final StreamController<TransportStatus> _status =
      StreamController<TransportStatus>.broadcast();

  IrohSession? _session;
  StreamSubscription<Uint8List>? _incomingSub;
  TransportStatus _current = const TransportStatus.disconnected();
  bool _closed = false;

  IrohTransport(this.endpointId, {this.directAddrs = const []});

  @override
  Stream<Uint8List> get incoming => _incoming.stream;

  @override
  Stream<TransportStatus> get status => _status.stream;

  @override
  TransportStatus get currentStatus => _current;

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
      _publish(const TransportStatus.connected());
    } catch (e) {
      _publish(TransportStatus.error(e.toString()));
    }
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
    final s = _session;
    _session = null;
    if (s != null) {
      await s.close();
    }
    _publish(const TransportStatus.disconnected());
    await _incoming.close();
    await _status.close();
  }

  void _publish(TransportStatus s) {
    _current = s;
    if (!_status.isClosed) _status.add(s);
  }
}
