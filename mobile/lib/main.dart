// Sandbox mobile client for the AnotherOne companion daemon.
//
// The widget here is deliberately transport-agnostic: it talks to a
// `TerminalTransport` interface and wires byte streams into xterm.dart's
// Terminal engine. Swapping WebSocket for an Iroh-backed transport later is
// a one-line change at `_buildTransport` below; nothing else in this file
// should need to move.

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:xterm/xterm.dart';

import 'src/transport.dart';
import 'src/transport_websocket.dart';

void main() => runApp(const SandboxApp());

class SandboxApp extends StatelessWidget {
  const SandboxApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: 'AnotherOne Sandbox',
        theme: ThemeData.dark(useMaterial3: true),
        home: const TerminalPage(),
      );
}

class TerminalPage extends StatefulWidget {
  const TerminalPage({super.key});

  @override
  State<TerminalPage> createState() => _TerminalPageState();
}

class _TerminalPageState extends State<TerminalPage> {
  // Android emulator's host-loopback alias; swap for your LAN IP on a device.
  static const String _defaultEndpoint = 'ws://10.0.2.2:5617/pty';

  final TextEditingController _endpointCtrl =
      TextEditingController(text: _defaultEndpoint);

  late final Terminal _terminal;
  late final TerminalController _terminalController;
  final FocusNode _terminalFocus = FocusNode();

  TerminalTransport? _transport;
  StreamSubscription<Uint8List>? _bytesSub;
  StreamSubscription<TransportStatus>? _statusSub;
  TransportStatus _status = const TransportStatus.disconnected();

  @override
  void initState() {
    super.initState();
    _terminal = Terminal(maxLines: 10000);
    _terminalController = TerminalController();
    _terminal.onOutput = (data) {
      _transport?.sendBytes(utf8.encode(data));
    };
    _terminal.onResize = (width, height, _, _) {
      _transport?.sendResize(cols: width, rows: height);
    };
  }

  /// Factory for the active transport. Kept as a single choice point so
  /// introducing Iroh (or any other) transport later is a local change.
  TerminalTransport _buildTransport(String endpoint) {
    return WebSocketTransport(endpoint);
  }

  void _connect() {
    _tearDownTransport();
    final transport = _buildTransport(_endpointCtrl.text.trim());
    _transport = transport;

    _bytesSub = transport.incoming.listen((bytes) {
      _terminal.write(utf8.decode(bytes, allowMalformed: true));
    });
    _statusSub = transport.status.listen((s) {
      setState(() => _status = s);
    });

    transport.connect();
    // Fire an initial resize so the daemon's PTY matches the Flutter grid.
    transport.sendResize(cols: _terminal.viewWidth, rows: _terminal.viewHeight);
  }

  Future<void> _disconnect() async {
    await _tearDownTransport();
    setState(() => _status = const TransportStatus.disconnected());
  }

  Future<void> _tearDownTransport() async {
    await _bytesSub?.cancel();
    _bytesSub = null;
    await _statusSub?.cancel();
    _statusSub = null;
    final t = _transport;
    _transport = null;
    await t?.close();
  }

  @override
  void dispose() {
    _tearDownTransport();
    _endpointCtrl.dispose();
    _terminalFocus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Sandbox Terminal'),
        actions: [
          Center(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: Text(
                _status.label,
                style: const TextStyle(fontSize: 12, fontFamily: 'monospace'),
              ),
            ),
          ),
        ],
      ),
      body: SafeArea(
        child: Column(
          children: [
            _EndpointRow(
              controller: _endpointCtrl,
              connected: _status.isConnected,
              onConnect: _connect,
              onDisconnect: _disconnect,
            ),
            Expanded(
              child: GestureDetector(
                onTap: _terminalFocus.requestFocus,
                child: TerminalView(
                  _terminal,
                  controller: _terminalController,
                  focusNode: _terminalFocus,
                  autofocus: true,
                  backgroundOpacity: 1.0,
                  padding: const EdgeInsets.all(6),
                  textStyle: const TerminalStyle(
                    fontFamily: 'monospace',
                    fontSize: 12,
                  ),
                ),
              ),
            ),
            _ChordBar(
              onSend: (bytes) {
                _transport?.sendBytes(bytes);
                _terminalFocus.requestFocus();
              },
            ),
          ],
        ),
      ),
    );
  }
}

class _EndpointRow extends StatelessWidget {
  const _EndpointRow({
    required this.controller,
    required this.connected,
    required this.onConnect,
    required this.onDisconnect,
  });

  final TextEditingController controller;
  final bool connected;
  final VoidCallback onConnect;
  final VoidCallback onDisconnect;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(8),
      child: Row(children: [
        Expanded(
          child: TextField(
            controller: controller,
            autocorrect: false,
            enableSuggestions: false,
            smartDashesType: SmartDashesType.disabled,
            smartQuotesType: SmartQuotesType.disabled,
            style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
            decoration: const InputDecoration(
              labelText: 'Endpoint',
              border: OutlineInputBorder(),
              isDense: true,
            ),
          ),
        ),
        const SizedBox(width: 8),
        FilledButton.icon(
          onPressed: connected ? onDisconnect : onConnect,
          icon: Icon(connected ? Icons.link_off : Icons.link),
          label: Text(connected ? 'Disconnect' : 'Connect'),
        ),
      ]),
    );
  }
}

class _ChordBar extends StatelessWidget {
  const _ChordBar({required this.onSend});

  final void Function(List<int> bytes) onSend;

  static const List<(String, List<int>)> _chords = [
    ('Esc', [0x1B]),
    ('Tab', [0x09]),
    ('Ctrl-C', [0x03]),
    ('Ctrl-D', [0x04]),
    ('Ctrl-L', [0x0C]),
    ('Ctrl-R', [0x12]),
    ('↑', [0x1B, 0x5B, 0x41]),
    ('↓', [0x1B, 0x5B, 0x42]),
    ('←', [0x1B, 0x5B, 0x44]),
    ('→', [0x1B, 0x5B, 0x43]),
  ];

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: Colors.grey.shade800)),
      ),
      padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            for (final (label, bytes) in _chords)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 2),
                child: OutlinedButton(
                  onPressed: () => onSend(bytes),
                  style: OutlinedButton.styleFrom(
                    padding:
                        const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                    minimumSize: Size.zero,
                    tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  ),
                  child: Text(label,
                      style: const TextStyle(
                          fontSize: 12, fontFamily: 'monospace')),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
