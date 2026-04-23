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

import 'src/rust/frb_generated.dart';
import 'src/transport.dart';
import 'src/transport_iroh.dart';
import 'src/transport_websocket.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  runApp(const SandboxApp());
}

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
  //
  // For Iroh, paste an `iroh://<endpoint_id>?direct=host:port[,host:port…]`
  // URL. The daemon prints its EndpointAddr on startup; on the emulator the
  // host is reachable at `10.0.2.2` and the daemon's UDP port is the number
  // in the Ip(192.168.x.y:PORT) line of the online log.
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

  /// Factory for the active transport. Dispatches by URL scheme:
  ///   - `ws://…` / `wss://…` → WebSocket (local sandbox daemon)
  ///   - `iroh://<endpoint_id>?direct=host:port[,host:port...]` → Iroh QUIC
  ///     via mobile_core. At least one direct addr is required because the
  ///     sandbox doesn't ship an address-lookup service.
  TerminalTransport _buildTransport(String endpoint) {
    final uri = Uri.parse(endpoint);
    if (uri.scheme == 'iroh') {
      final id = uri.host.isNotEmpty ? uri.host : uri.path.replaceAll('/', '');
      final direct = uri.queryParameters['direct'];
      final addrs = (direct ?? '')
          .split(',')
          .map((s) => s.trim())
          .where((s) => s.isNotEmpty)
          .toList();
      return IrohTransport(id, directAddrs: addrs);
    }
    return WebSocketTransport(endpoint);
  }

  Future<void> _connect() async {
    await _tearDownTransport();
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
    // Snapshot all owned state synchronously before any await, so a second
    // call (or a reassignment of _transport) can't accidentally close the
    // wrong thing. Previously `_connect()` fired a non-awaited teardown then
    // swapped _transport; the teardown's continuation then read the new
    // transport and closed it — race fixed.
    final bytesSub = _bytesSub;
    final statusSub = _statusSub;
    final t = _transport;
    _bytesSub = null;
    _statusSub = null;
    _transport = null;
    await bytesSub?.cancel();
    await statusSub?.cancel();
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
