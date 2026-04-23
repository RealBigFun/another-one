// Transport abstraction for the sandbox terminal client.
//
// The goal is a narrow interface that knows nothing about the terminal widget
// or any particular wire protocol. A `TerminalTransport` is a one-shot:
// construct, connect, stream bytes, close. To reconnect or switch URLs, build
// a new transport.
//
// The current implementation speaks WebSocket (see transport_websocket.dart);
// a future impl will speak Iroh QUIC through a flutter_rust_bridge-generated
// Dart surface. The UI widget should be written against the interface only.

import 'dart:async';
import 'dart:typed_data';

enum TransportState { disconnected, connecting, connected, error }

class TransportStatus {
  final TransportState state;
  final String? detail;
  const TransportStatus(this.state, {this.detail});

  const TransportStatus.disconnected() : this(TransportState.disconnected);
  const TransportStatus.connecting() : this(TransportState.connecting);
  const TransportStatus.connected() : this(TransportState.connected);
  const TransportStatus.error(String message)
      : this(TransportState.error, detail: message);

  String get label => switch (state) {
        TransportState.disconnected => 'disconnected',
        TransportState.connecting => 'connecting…',
        TransportState.connected => 'connected',
        TransportState.error => detail == null ? 'error' : 'error: $detail',
      };

  bool get isConnected => state == TransportState.connected;
}

abstract class TerminalTransport {
  /// Bytes received from the remote PTY. Subscribers see every inbound
  /// frame; chunk boundaries are not guaranteed to align with lines.
  Stream<Uint8List> get incoming;

  /// Connection-state updates. Emits at least once when something changes
  /// and replays the latest value to late subscribers.
  Stream<TransportStatus> get status;

  /// The latest status, useful for rendering without waiting for the stream.
  TransportStatus get currentStatus;

  /// Starts the connection. Non-blocking; progress reports via [status].
  void connect();

  /// Sends raw bytes to the remote PTY's stdin.
  void sendBytes(List<int> bytes);

  /// Requests a PTY resize. Implementations decide how to encode this
  /// (WebSocket uses a JSON text frame; Iroh will use a framed control
  /// message on a side stream).
  void sendResize({required int cols, required int rows});

  /// Closes the connection and releases any underlying resources. Safe to
  /// call multiple times. After close, the transport is inert — build a
  /// new one to reconnect.
  Future<void> close();
}
