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
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:shared_preferences/shared_preferences.dart';
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
  // Initial placeholder used only on the very first launch before we've
  // loaded anything from SharedPreferences. Once a user connects
  // successfully via an `iroh://…` URL, we persist it under
  // `_prefsEndpointKey` and reuse it on subsequent launches — so the phone
  // only has to be paired once, even though this hardcoded string points
  // at the emulator's loopback WebSocket sandbox.
  static const String _defaultEndpoint = 'ws://10.0.2.2:5617/pty';
  static const String _prefsEndpointKey = 'last_endpoint';

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
    _restoreEndpoint();
  }

  /// Load the last-successful endpoint URL from SharedPreferences and
  /// drop it into the text field. First-run will find nothing and leave
  /// the compile-time default in place.
  Future<void> _restoreEndpoint() async {
    final prefs = await SharedPreferences.getInstance();
    final saved = prefs.getString(_prefsEndpointKey);
    if (saved != null && saved.trim().isNotEmpty && mounted) {
      setState(() {
        _endpointCtrl.text = saved;
      });
    }
  }

  /// Store `url` as the endpoint to prefer on next launch. Called the
  /// first time a connection reaches the `connected` state — we only
  /// persist URLs that actually work, so a bad paste doesn't poison the
  /// next launch.
  Future<void> _persistEndpoint(String url) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_prefsEndpointKey, url);
  }

  /// Factory for the active transport. Dispatches by URL scheme:
  ///   - `ws://…` / `wss://…` → WebSocket (local sandbox daemon)
  ///   - `iroh://<endpoint_id>?direct=host:port[,host:port...][&relay=url[,url...]]`
  ///     → Iroh QUIC via mobile_core. At least one `direct` or `relay`
  ///     entry is required because the sandbox has no address lookup.
  ///     On-LAN clients usually only need `direct`; off-LAN (cellular,
  ///     CGNAT) paths need `relay` so the dev relay mesh can forward.
  TerminalTransport _buildTransport(String endpoint) {
    final uri = Uri.parse(endpoint);
    if (uri.scheme == 'iroh') {
      final id = uri.host.isNotEmpty ? uri.host : uri.path.replaceAll('/', '');
      List<String> splitCsv(String? s) => (s ?? '')
          .split(',')
          .map((x) => x.trim())
          .where((x) => x.isNotEmpty)
          .toList();
      final addrs = splitCsv(uri.queryParameters['direct']);
      final relays = splitCsv(uri.queryParameters['relay']);
      return IrohTransport(id, directAddrs: addrs, relayUrls: relays);
    }
    return WebSocketTransport(endpoint);
  }

  Future<void> _connect() async {
    await _tearDownTransport();
    final url = _endpointCtrl.text.trim();
    final transport = _buildTransport(url);
    _transport = transport;

    _bytesSub = transport.incoming.listen((bytes) {
      _terminal.write(utf8.decode(bytes, allowMalformed: true));
    });
    bool savedOnce = false;
    _statusSub = transport.status.listen((s) {
      setState(() => _status = s);
      if (!savedOnce && s.state == TransportState.connected) {
        savedOnce = true;
        unawaited(_persistEndpoint(url));
      }
    });

    transport.connect();
    // Fire an initial resize so the daemon's PTY matches the Flutter grid.
    transport.sendResize(cols: _terminal.viewWidth, rows: _terminal.viewHeight);
  }

  Future<void> _disconnect() async {
    await _tearDownTransport();
    setState(() => _status = const TransportStatus.disconnected());
  }

  /// Push the QR scanner, wait for a result, and drop the decoded URL
  /// into the endpoint field. Does **not** auto-connect — the user sees
  /// the parsed URL and hits Connect themselves, which also makes it
  /// easy to spot a bad scan before wasting a round-trip.
  Future<void> _scanQr() async {
    final result = await Navigator.of(context).push<String>(
      MaterialPageRoute(builder: (_) => const _QrScanPage()),
    );
    if (result == null || result.trim().isEmpty) return;
    if (!mounted) return;
    setState(() {
      _endpointCtrl.text = result.trim();
    });
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
              onScan: _scanQr,
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
    required this.onScan,
  });

  final TextEditingController controller;
  final bool connected;
  final VoidCallback onConnect;
  final VoidCallback onDisconnect;
  final VoidCallback onScan;

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
            decoration: InputDecoration(
              labelText: 'Endpoint',
              border: const OutlineInputBorder(),
              isDense: true,
              suffixIcon: IconButton(
                tooltip: 'Scan pairing QR',
                icon: const Icon(Icons.qr_code_scanner),
                onPressed: onScan,
              ),
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

/// Full-screen camera QR scanner. Pops back with the first scanned
/// barcode's raw text as the `Navigator.pop` result. Designed for the
/// `iroh://…` pairing URL the daemon prints at startup, but doesn't
/// validate the scheme — caller decides what to do with bad input.
class _QrScanPage extends StatefulWidget {
  const _QrScanPage();

  @override
  State<_QrScanPage> createState() => _QrScanPageState();
}

class _QrScanPageState extends State<_QrScanPage> {
  final MobileScannerController _controller = MobileScannerController(
    detectionSpeed: DetectionSpeed.normal,
    formats: const [BarcodeFormat.qrCode],
  );
  bool _handed = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _onDetect(BarcodeCapture capture) {
    if (_handed) return;
    for (final barcode in capture.barcodes) {
      final raw = barcode.rawValue;
      if (raw != null && raw.isNotEmpty) {
        _handed = true;
        Navigator.of(context).pop(raw);
        return;
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Scan Pairing QR')),
      body: Stack(
        children: [
          MobileScanner(controller: _controller, onDetect: _onDetect),
          Align(
            alignment: Alignment.bottomCenter,
            child: Padding(
              padding: const EdgeInsets.all(24),
              child: Text(
                'Point at the QR printed by the daemon.\n'
                "The URL populates the endpoint field; tap Connect to dial.",
                textAlign: TextAlign.center,
                style: TextStyle(
                  color: Colors.white.withValues(alpha: 0.85),
                  fontSize: 13,
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
