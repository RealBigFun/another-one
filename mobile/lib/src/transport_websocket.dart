// WebSocket implementation of TerminalTransport.
//
// Matches the daemon-sandbox WebSocket protocol: binary frames carry raw PTY
// bytes; a JSON text frame {"type":"resize","cols":C,"rows":R} requests a
// PTY resize. This transport is what the sandbox has been using since
// milestone 2; milestone 4+ will introduce an Iroh equivalent without
// touching the UI layer that consumes this interface.

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:web_socket_channel/status.dart' as ws_status;
import 'package:web_socket_channel/web_socket_channel.dart';

import 'transport.dart';

class WebSocketTransport implements TerminalTransport {
  final String url;

  final StreamController<Uint8List> _incoming =
      StreamController<Uint8List>.broadcast();
  final StreamController<TransportStatus> _status =
      StreamController<TransportStatus>.broadcast();

  WebSocketChannel? _channel;
  StreamSubscription<dynamic>? _sub;
  TransportStatus _current = const TransportStatus.disconnected();
  bool _closed = false;

  WebSocketTransport(this.url);

  @override
  Stream<Uint8List> get incoming => _incoming.stream;

  @override
  Stream<TransportStatus> get status => _status.stream;

  @override
  TransportStatus get currentStatus => _current;

  @override
  void connect() {
    if (_closed) return;
    _disconnectInner();
    _publish(const TransportStatus.connecting());
    try {
      final channel = WebSocketChannel.connect(Uri.parse(url));
      _channel = channel;
      _sub = channel.stream.listen(
        _onFrame,
        onError: (err) => _publish(TransportStatus.error(err.toString())),
        onDone: () => _publish(const TransportStatus.disconnected()),
        cancelOnError: true,
      );
    } catch (e) {
      _publish(TransportStatus.error(e.toString()));
    }
  }

  @override
  void sendBytes(List<int> bytes) {
    _channel?.sink.add(Uint8List.fromList(bytes));
  }

  @override
  void sendResize({required int cols, required int rows}) {
    _channel?.sink.add(
      jsonEncode({'type': 'resize', 'cols': cols, 'rows': rows}),
    );
  }

  @override
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    _disconnectInner();
    await _incoming.close();
    await _status.close();
  }

  void _onFrame(dynamic data) {
    Uint8List? bytes;
    if (data is Uint8List) {
      bytes = data;
    } else if (data is List<int>) {
      bytes = Uint8List.fromList(data);
    } else if (data is String) {
      bytes = Uint8List.fromList(utf8.encode(data));
    }
    if (bytes == null) return;
    _incoming.add(bytes);
    if (_current.state != TransportState.connected) {
      _publish(const TransportStatus.connected());
    }
  }

  void _disconnectInner() {
    _sub?.cancel();
    _sub = null;
    _channel?.sink.close(ws_status.goingAway);
    _channel = null;
  }

  void _publish(TransportStatus s) {
    _current = s;
    if (!_status.isClosed) _status.add(s);
  }
}
