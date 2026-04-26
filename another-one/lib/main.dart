// Entry point. All screens live under `lib/src/`.
//
// Boot order (every step except `FlutterError.onError` runs
// inside the guarded zone so `ensureInitialized` and `runApp`
// share the same one — Flutter asserts on the mismatch):
//
//   0. `FlutterError.onError` — render / build / layout
//      assertion sink. Set before the zone so even framework
//      asserts triggered during boot routing land in our log.
//   1. (inside guarded zone)
//   2.   `WidgetsFlutterBinding.ensureInitialized()`.
//   3.   `initLogFile()` — truncate `/tmp/aone-debug.log`.
//   4.   `PlatformDispatcher.instance.onError` — catches
//        async errors that escape Flutter's zone.
//   5.   `RustLib.init()` — bridge native init.
//   6.   `setDataDir(...)` — pin iroh's secret key path.
//   7.   `bootEmbeddedDaemon()` — desktop only.
//   8.   `runApp(...)`.

import 'dart:io' show Platform;
import 'dart:ui' show PlatformDispatcher;

import 'package:flutter/foundation.dart' show DiagnosticsTreeStyle, kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

import 'src/app_root.dart';
import 'src/benchmark_page.dart';
import 'src/log.dart';
import 'src/rust/api/embedded_daemon.dart' as embedded_daemon;
import 'src/rust/api/iroh_client.dart';
import 'src/rust/frb_generated.dart';
import 'src/surface_router.dart';
import 'src/theme.dart';

/// Activated by `flutter run/build … --dart-define=BENCHMARK=true`.
/// Launches the standalone xterm.dart throughput benchmark instead
/// of the normal pair-and-attach flow. Phase 0 de-risk gate; off in
/// every regular build.
const bool kBenchmarkMode = bool.fromEnvironment('BENCHMARK');

const _bootLog = Log('aone.boot');

void main() {
  // Capture every framework error (overflow assertions, missing
  // overrides, build-method exceptions) through our log sink.
  // Set before the zone so an assert fired during boot routing
  // still lands in the log; the handler itself doesn't read
  // zone-specific state.
  FlutterError.onError = (details) {
    const log = Log('aone.flutter');
    // For RenderFlex overflows the framework's default summary is
    // just "A RenderFlex overflowed by N pixels on the right" with
    // no widget-tree path. The DiagnosticsNode chain (built from
    // `details.toDiagnosticsNode(...).toStringDeep`) names the
    // offending widget hierarchy + offers the "creator" callsite,
    // which is what we actually need to track these down.
    final diag = details
        .toDiagnosticsNode(style: DiagnosticsTreeStyle.error)
        .toStringDeep();
    log.error(
      details.exceptionAsString(),
      error: details.exception,
      stackTrace: details.stack,
      fields: {
        if (details.library != null) 'library': details.library!,
        if (details.context != null)
          'context': details.context.toString(),
        'tree': diag,
      },
    );
    // Still hand the error to the framework's default presenter
    // so the red-screen / console blob isn't suppressed in dev.
    FlutterError.presentError(details);
  };

  runGuardedApp(() async {
    WidgetsFlutterBinding.ensureInitialized();
    await initLogFile();
    _bootLog.info('boot start', {
      'cpus': Platform.numberOfProcessors,
      'os': Platform.operatingSystem,
    });

    // Async errors that escape Flutter's zone (e.g. errors from
    // a platform-channel callback). Goes through the same sink.
    PlatformDispatcher.instance.onError = (error, stack) {
      const log = Log('aone.platform');
      log.error('platform dispatcher error', error: error, stackTrace: stack);
      return true; // mark handled
    };

    await RustLib.init();
    // Point another-one-bridge at a persistent location for the
    // iroh secret key before any `irohConnect` can be called.
    // Without this, every app restart generates a new EndpointId
    // and breaks TOFU pairing.
    final supportDir = await getApplicationSupportDirectory();
    setDataDir(path: supportDir.path);
    // Boot the embedded iroh daemon on desktop platforms only.
    // Mobile clients connect to remote daemons over iroh; running
    // an embedded daemon there would just chew battery for no
    // consumer.
    if (_isDesktop) {
      try {
        await embedded_daemon.bootEmbeddedDaemon();
        _bootLog.info('embedded daemon up');
      } catch (e, s) {
        // Surface but don't block UI — the pair-mobile modal will
        // show its empty state until a retry succeeds.
        _bootLog.error(
          'embedded daemon boot failed',
          error: e,
          stackTrace: s,
        );
      }
    }
    runApp(const ProviderScope(child: AnotherOneApp()));
  });
}

bool get _isDesktop {
  if (kIsWeb) return false;
  return Platform.isLinux || Platform.isMacOS || Platform.isWindows;
}

class AnotherOneApp extends StatelessWidget {
  const AnotherOneApp({super.key});

  @override
  Widget build(BuildContext context) {
    // Surface flag wins over benchmark and the regular AppRoot — it
    // exists for visual review, so a typo failing through to the
    // shell would defeat the point.
    final surface = surfaceFor(kSurface);
    final home = surface ??
        (kBenchmarkMode ? const BenchmarkPage() : const AppRoot());
    return MaterialApp(
      title: 'AnotherOne',
      theme: buildAppTheme(),
      home: home,
      // Kill Material's diagonal "DEBUG" ribbon — it never
      // existed in the GPUI build, and we run debug builds
      // most of the time during development.
      debugShowCheckedModeBanner: false,
    );
  }
}
