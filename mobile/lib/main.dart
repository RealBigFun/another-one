// Sandbox mobile client for the AnotherOne companion daemon.
// Connects to a raw-PTY WebSocket (see daemon-sandbox/) and displays the byte
// stream. No alacritty_terminal yet — milestone 2 proves the transport shape;
// milestone 3 swaps this raw-text view for a real VT100 grid.

import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:web_socket_channel/web_socket_channel.dart';
import 'package:web_socket_channel/status.dart' as ws_status;

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
  final TextEditingController _inputCtrl = TextEditingController();
  final ScrollController _scrollCtrl = ScrollController();

  WebSocketChannel? _channel;
  StreamSubscription<dynamic>? _sub;
  final StringBuffer _buffer = StringBuffer();
  String _status = 'disconnected';

  void _connect() {
    _disconnect();
    final url = _urlCtrl.text.trim();
    try {
      final channel = WebSocketChannel.connect(Uri.parse(url));
      setState(() {
        _channel = channel;
        _status = 'connecting…';
        _buffer.clear();
      });
      _sub = channel.stream.listen(
        (data) {
          if (data is List<int>) {
            setState(() {
              _buffer.write(utf8.decode(data, allowMalformed: true));
              if (_status.startsWith('connecting')) _status = 'connected';
            });
            _scrollToBottom();
          } else if (data is String) {
            setState(() => _buffer.writeln('[text-frame] $data'));
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

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!_scrollCtrl.hasClients) return;
      _scrollCtrl.jumpTo(_scrollCtrl.position.maxScrollExtent);
    });
  }

  void _sendBytes(List<int> bytes) {
    final ch = _channel;
    if (ch == null) {
      setState(() => _status = 'not connected');
      return;
    }
    ch.sink.add(bytes);
  }

  void _sendLine() {
    final text = _inputCtrl.text;
    _sendBytes(utf8.encode('$text\r'));
    _inputCtrl.clear();
  }

  void _clearBuffer() {
    setState(() => _buffer.clear());
  }

  @override
  void dispose() {
    _disconnect();
    _urlCtrl.dispose();
    _inputCtrl.dispose();
    _scrollCtrl.dispose();
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
          IconButton(
            icon: const Icon(Icons.clear_all),
            tooltip: 'Clear buffer',
            onPressed: _clearBuffer,
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
              child: Container(
                color: const Color(0xFF0C0C0C),
                width: double.infinity,
                padding: const EdgeInsets.all(8),
                child: SingleChildScrollView(
                  controller: _scrollCtrl,
                  child: SelectableText(
                    _buffer.isEmpty
                        ? '(waiting for bytes…)'
                        : _buffer.toString(),
                    style: const TextStyle(
                      fontFamily: 'monospace',
                      color: Color(0xFFE5E5E5),
                      fontSize: 12,
                      height: 1.3,
                    ),
                  ),
                ),
              ),
            ),
            Container(
              decoration: BoxDecoration(
                border: Border(top: BorderSide(color: Colors.grey.shade800)),
              ),
              padding: const EdgeInsets.all(8),
              child: Column(children: [
                SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: Row(children: [
                    _chord('Esc', const [0x1B]),
                    _chord('Tab', const [0x09]),
                    _chord('Ctrl-C', const [0x03]),
                    _chord('Ctrl-D', const [0x04]),
                    _chord('Ctrl-L', const [0x0C]),
                    _chord('↑', const [0x1B, 0x5B, 0x41]),
                    _chord('↓', const [0x1B, 0x5B, 0x42]),
                    _chord('←', const [0x1B, 0x5B, 0x44]),
                    _chord('→', const [0x1B, 0x5B, 0x43]),
                  ]),
                ),
                const SizedBox(height: 8),
                Row(children: [
                  Expanded(
                    child: TextField(
                      controller: _inputCtrl,
                      autocorrect: false,
                      enableSuggestions: false,
                      smartDashesType: SmartDashesType.disabled,
                      smartQuotesType: SmartQuotesType.disabled,
                      textInputAction: TextInputAction.send,
                      style: const TextStyle(fontFamily: 'monospace'),
                      decoration: const InputDecoration(
                        hintText: 'type a command; Enter sends + \\r',
                        border: OutlineInputBorder(),
                        isDense: true,
                      ),
                      onSubmitted: (_) => _sendLine(),
                    ),
                  ),
                  IconButton(
                    icon: const Icon(Icons.send),
                    tooltip: 'Send line',
                    onPressed: _sendLine,
                  ),
                ]),
              ]),
            ),
          ],
        ),
      ),
    );
  }

  Widget _chord(String label, List<int> bytes) => Padding(
        padding: const EdgeInsets.symmetric(horizontal: 2),
        child: OutlinedButton(
          onPressed: () => _sendBytes(bytes),
          style: OutlinedButton.styleFrom(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
            minimumSize: Size.zero,
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          child:
              Text(label, style: const TextStyle(fontSize: 12, fontFamily: 'monospace')),
        ),
      );
}
