// Mobile client for the AnotherOne companion daemon. Entry point
// only — all screens live under `lib/src/`:
//
//   - `app_root.dart` — routes between Pair and Drawer based on
//     whether an endpoint is saved.
//   - `pair_device_page.dart` — onboarding / QR pairing.
//   - `projects_drawer_page.dart` — home: expandable project list.
//   - `task_page.dart` — per-task terminal with tab strip.
//   - `settings_page.dart` — unlink / rescan.
//   - `qr_scan_page.dart` — pairing QR scanner, pushed from either.

import 'dart:io' show Platform;

import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

import 'src/app_root.dart';
import 'src/benchmark_page.dart';
import 'src/rust/api/embedded_daemon.dart' as embedded_daemon;
import 'src/rust/api/iroh_client.dart';
import 'src/rust/frb_generated.dart';
import 'src/theme.dart';

/// Activated by `flutter run/build … --dart-define=BENCHMARK=true`.
/// Launches the standalone xterm.dart throughput benchmark instead
/// of the normal pair-and-attach flow. Phase 0 de-risk gate; off in
/// every regular build.
const bool kBenchmarkMode = bool.fromEnvironment('BENCHMARK');

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  // Point another-one-bridge at a persistent location for the iroh secret
  // key before any `ironConnect` can be called. Without this, every
  // app restart generates a new EndpointId and breaks TOFU pairing.
  final supportDir = await getApplicationSupportDirectory();
  setDataDir(path: supportDir.path);
  // Boot the embedded iroh daemon on desktop platforms only.
  // Mobile clients connect to remote daemons over iroh; running an
  // embedded daemon there would just chew battery for no consumer.
  if (_isDesktop) {
    try {
      await embedded_daemon.bootEmbeddedDaemon();
    } catch (e) {
      // Surface but don't block UI — the pair-mobile modal will show
      // its empty state until a retry succeeds.
      debugPrint('embedded daemon boot failed: $e');
    }
  }
  runApp(const ProviderScope(child: AnotherOneApp()));
}

bool get _isDesktop {
  if (kIsWeb) return false;
  return Platform.isLinux || Platform.isMacOS || Platform.isWindows;
}

class AnotherOneApp extends StatelessWidget {
  const AnotherOneApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: 'AnotherOne',
        theme: buildAppTheme(),
        home: kBenchmarkMode ? const BenchmarkPage() : const AppRoot(),
      );
}
