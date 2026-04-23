// Root widget. Owns:
//   - the saved endpoint URL (via SharedPreferences),
//   - the `IrohTransport` lifecycle,
//   - the `List<ProjectSummary>` surfaced to the drawer.
//
// No visible Connect/Disconnect button: if an endpoint is saved we
// connect automatically on app start. If nothing's saved we show the
// pairing screen and kick off a connection as soon as a scan/paste
// hands us a URL.

import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'pair_device_page.dart';
import 'projects_drawer_page.dart';
import 'rust/api/iroh_client.dart';
import 'settings_page.dart';
import 'transport.dart';
import 'transport_iroh.dart';

class AppRoot extends StatefulWidget {
  const AppRoot({super.key});

  @override
  State<AppRoot> createState() => _AppRootState();
}

class _AppRootState extends State<AppRoot> {
  static const String _prefsEndpointKey = 'last_endpoint';

  String _endpoint = '';
  bool _prefsLoaded = false;

  IrohTransport? _transport;
  StreamSubscription<TransportStatus>? _statusSub;
  StreamSubscription<WorkerReply>? _workerRepliesSub;
  // Drain subscription — prevents the broadcast stream from backing
  // up while nothing else is listening. The drawer doesn't render
  // PTY bytes; only TaskPage does, via its own subscription.
  StreamSubscription<Uint8List>? _bytesDrainSub;

  List<ProjectSummary> _projects = const [];

  @override
  void initState() {
    super.initState();
    _loadPrefs();
  }

  Future<void> _loadPrefs() async {
    final prefs = await SharedPreferences.getInstance();
    final saved = prefs.getString(_prefsEndpointKey) ?? '';
    if (!mounted) return;
    setState(() {
      _endpoint = saved;
      _prefsLoaded = true;
    });
    if (saved.isNotEmpty) {
      _connect(saved);
    }
  }

  Future<void> _persistEndpoint(String url) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_prefsEndpointKey, url);
  }

  Future<void> _clearEndpoint() async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_prefsEndpointKey);
  }

  /// Builds + connects a new transport for [endpoint]. Tears down any
  /// previous transport first.
  void _connect(String endpoint) {
    _tearDownTransport();
    final transport = _buildTransport(endpoint);
    if (transport == null) return;
    _transport = transport;

    // Keep the broadcast stream drained when nothing renders bytes.
    _bytesDrainSub = transport.incoming.listen((_) {});

    _statusSub = transport.status.listen((s) {
      if (!mounted) return;
      if (s.state == TransportState.connected) {
        // First successful connect → refresh project list.
        unawaited(transport.listProjects().catchError((_) {}));
      }
    });

    _workerRepliesSub = transport.workerReplies.listen((reply) {
      if (!mounted) return;
      if (reply is WorkerReply_ProjectList) {
        setState(() => _projects = reply.projects);
      }
      // GitRefresh / PullRequestStatus: not surfaced in the drawer —
      // TaskPage / future pages can subscribe themselves.
    });

    transport.connect();
  }

  IrohTransport? _buildTransport(String endpoint) {
    // We only speak iroh on mobile; the legacy ws path was only ever
    // used against the standalone daemon-sandbox binary.
    final uri = Uri.tryParse(endpoint);
    if (uri == null || uri.scheme != 'iroh') return null;
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

  void _tearDownTransport() {
    final t = _transport;
    final byteSub = _bytesDrainSub;
    final statusSub = _statusSub;
    final repliesSub = _workerRepliesSub;
    _transport = null;
    _bytesDrainSub = null;
    _statusSub = null;
    _workerRepliesSub = null;
    unawaited(byteSub?.cancel());
    unawaited(statusSub?.cancel());
    unawaited(repliesSub?.cancel());
    unawaited(t?.close());
  }

  Future<void> _onPaired(String url) async {
    await _persistEndpoint(url);
    if (!mounted) return;
    setState(() {
      _endpoint = url;
      _projects = const [];
    });
    _connect(url);
  }

  Future<void> _unlink() async {
    _tearDownTransport();
    await _clearEndpoint();
    if (!mounted) return;
    // Pop settings page if it's on top so the user lands back on the
    // pair screen.
    Navigator.of(context).popUntil((r) => r.isFirst);
    setState(() {
      _endpoint = '';
      _projects = const [];
    });
  }

  Future<void> _replaceEndpoint(String url) async {
    await _persistEndpoint(url);
    if (!mounted) return;
    setState(() {
      _endpoint = url;
      _projects = const [];
    });
    _connect(url);
  }

  Future<void> _refreshProjects() async {
    final t = _transport;
    if (t == null) return;
    try {
      await t.listProjects();
    } catch (_) {
      // Swallow — surfacing transport errors as a snackbar is more
      // disruptive than the retry cost.
    }
  }

  @override
  void dispose() {
    _tearDownTransport();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!_prefsLoaded) {
      return const Scaffold(
        body: Center(child: CircularProgressIndicator()),
      );
    }

    if (_endpoint.isEmpty) {
      return PairDevicePage(onPaired: _onPaired);
    }

    final transport = _transport;
    if (transport == null) {
      // Endpoint saved but iroh-parse failed (unlikely unless the
      // saved URL is malformed). Fall back to the pair screen so the
      // user can re-pair.
      return PairDevicePage(onPaired: _onPaired);
    }

    return ProjectsDrawerPage(
      transport: transport,
      projects: _projects,
      onRefresh: _refreshProjects,
      onOpenSettings: () {
        Navigator.of(context).push(
          MaterialPageRoute(
            builder: (_) => SettingsPage(
              endpoint: _endpoint,
              onUnlink: _unlink,
              onReplaceEndpoint: _replaceEndpoint,
            ),
          ),
        );
      },
    );
  }
}
