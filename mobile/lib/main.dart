// Sandbox mobile client for the AnotherOne companion daemon (milestone 3).
// Uses xterm.dart's Terminal engine + TerminalView for real VT100/xterm-256
// interpretation and cell-grid rendering. Milestone 2 shipped raw-text pass-
// through; this upgrade makes the display actually readable.

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:web_socket_channel/web_socket_channel.dart';
import 'package:web_socket_channel/status.dart' as ws_status;
import 'package:xterm/xterm.dart';

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
  static const String _defaultUrl = 'ws://10.0.2.2:5617/pty';

  final TextEditingController _urlCtrl =
      TextEditingController(text: _defaultUrl);

  late final Terminal _terminal;
  late final TerminalController _terminalController;
  final FocusNode _terminalFocus = FocusNode();

  WebSocketChannel? _channel;
  StreamSubscription<dynamic>? _sub;
  String _status = 'disconnected';

  @override
  void initState() {
    super.initState();
    _terminal = Terminal(maxLines: 10000);
    _terminalController = TerminalController();

    // User input from the terminal (keyboard, paste, chord taps via terminal
    // APIs) is emitted here as a String; forward the bytes to the daemon.
    _terminal.onOutput = (data) {
      _sendBytes(utf8.encode(data));
    };

    // When the terminal widget decides on a grid size, tell the PTY.
    _terminal.onResize = (w, h, pw, ph) {
      _sendResize(cols: w, rows: h);
    };
  }

  void _connect() {
    _disconnect();
    final url = _urlCtrl.text.trim();
    try {
      final channel = WebSocketChannel.connect(Uri.parse(url));
      setState(() {
        _channel = channel;
        _status = 'connecting…';
      });
      // Tell the daemon about our grid size as soon as we connect.
      // onResize has already fired at least once by now (first layout pass).
      _sendResize(cols: _terminal.viewWidth, rows: _terminal.viewHeight);

      _sub = channel.stream.listen(
        (data) {
          Uint8List bytes;
          if (data is Uint8List) {
            bytes = data;
          } else if (data is List<int>) {
            bytes = Uint8List.fromList(data);
          } else if (data is String) {
            bytes = Uint8List.fromList(utf8.encode(data));
          } else {
            return;
          }
          // xterm.dart's Terminal.write accepts a String; decode as best we can.
          _terminal.write(utf8.decode(bytes, allowMalformed: true));
          if (_status.startsWith('connecting')) {
            setState(() => _status = 'connected');
          }
        },
        onError: (err) => setState(() => _status = 'error: $err'),
        onDone: () => setState(() => _status = 'disconnected'),
        cancelOnError: true,
      );
    } catch (e) {
      setState(() => _status = 'connect failed: $e');
    }
  }

  void _disconnect() {
    _sub?.cancel();
    _sub = null;
    _channel?.sink.close(ws_status.goingAway);
    _channel = null;
  }

  void _sendBytes(List<int> bytes) {
    final ch = _channel;
    if (ch == null) return;
    ch.sink.add(Uint8List.fromList(bytes));
  }

  void _sendResize({required int cols, required int rows}) {
    final ch = _channel;
    if (ch == null) return;
    ch.sink.add(jsonEncode({
      'type': 'resize',
      'cols': cols,
      'rows': rows,
    }));
  }

  @override
  void dispose() {
    _disconnect();
    _urlCtrl.dispose();
    _terminalFocus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final connected = _channel != null && _status == 'connected';
    return Scaffold(
      appBar: AppBar(
        title: const Text('Sandbox Terminal'),
        actions: [
          Center(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: Text(
                _status,
                style: const TextStyle(fontSize: 12, fontFamily: 'monospace'),
              ),
            ),
          ),
        ],
      ),
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.all(8),
              child: Row(children: [
                Expanded(
                  child: TextField(
                    controller: _urlCtrl,
                    autocorrect: false,
                    enableSuggestions: false,
                    smartDashesType: SmartDashesType.disabled,
                    smartQuotesType: SmartQuotesType.disabled,
                    style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
                    decoration: const InputDecoration(
                      labelText: 'WebSocket URL',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                FilledButton.icon(
                  onPressed: connected ? _disconnect : _connect,
                  icon: Icon(connected ? Icons.link_off : Icons.link),
                  label: Text(connected ? 'Disconnect' : 'Connect'),
                ),
              ]),
            ),
            Expanded(
              child: GestureDetector(
                onTap: () => _terminalFocus.requestFocus(),
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
            // Mobile chord bar — the soft keyboard has no Esc/Ctrl/arrows.
            Container(
              decoration: BoxDecoration(
                border: Border(top: BorderSide(color: Colors.grey.shade800)),
              ),
              padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
              child: SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(children: [
                  _chord('Esc', const [0x1B]),
                  _chord('Tab', const [0x09]),
                  _chord('Ctrl-C', const [0x03]),
                  _chord('Ctrl-D', const [0x04]),
                  _chord('Ctrl-L', const [0x0C]),
                  _chord('Ctrl-R', const [0x12]),
                  _chord('↑', const [0x1B, 0x5B, 0x41]),
                  _chord('↓', const [0x1B, 0x5B, 0x42]),
                  _chord('←', const [0x1B, 0x5B, 0x44]),
                  _chord('→', const [0x1B, 0x5B, 0x43]),
                ]),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _chord(String label, List<int> bytes) => Padding(
        padding: const EdgeInsets.symmetric(horizontal: 2),
        child: OutlinedButton(
          onPressed: () {
            _sendBytes(bytes);
            _terminalFocus.requestFocus();
          },
          style: OutlinedButton.styleFrom(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
            minimumSize: Size.zero,
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          child: Text(label,
              style: const TextStyle(fontSize: 12, fontFamily: 'monospace')),
        ),
      );
}
