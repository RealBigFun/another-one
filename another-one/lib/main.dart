// Entry point. All screens live under `lib/src/`.
//
// Boot order:
//   1. `WidgetsFlutterBinding.ensureInitialized()` — required
//      before any plugin call.
//   2. `initLogFile()` — truncate /tmp/aone-debug.log so each
//      `flutter run` session starts clean.
//   3. `FlutterError.onError` + `runZonedGuarded` — global
//      error sinks, both routed through the `aone.uncaught`
//      logger (DevTools + stderr + file tee).
//   4. `RustLib.init()` — bridge native init.
//   5. `setDataDir(...)` — pin iroh's secret key path.
//   6. `bootEmbeddedDaemon()` — desktop only.
//   7. `runApp(...)` — wrapped in `runGuardedApp` so async
//      errors after this point still land in the same sink.

import 'dart:async';
import 'dart:io' show Platform;
import 'dart:ui' show PlatformDispatcher;

import 'package:flutter/foundation.dart' show kIsWeb;
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

Future<void> main() async {
  // Capture every framework error (overflow assertions, missing
  // overrides, build-method exceptions) through our log sink.
  // The default handler dumps a multi-line `EXCEPTION CAUGHT BY
  // FRAMEWORK` blob — useful, but hard to grep from a tail.
  FlutterError.onError = (details) {
    const log = Log('aone.flutter');
    log.error(
      details.exceptionAsString(),
      error: details.exception,
      stackTrace: details.stack,
      fields: {
        if (details.library != null) 'library': details.library!,
        if (details.context != null)
          'context': details.context.toString(),
      },
    );
    // Still hand the error to the framework's default presenter
    // so the red-screen / console blob isn't suppressed in dev.
    FlutterError.presentError(details);
  };

  // Linux PlatformDispatcher catches everything that escapes
  // outside of Flutter's own zone (e.g. errors from a
  // platform-channel callback). Pipe it through the same sink.
  WidgetsFlutterBinding.ensureInitialized();
  await initLogFile();
  _bootLog.info('boot start', {
    'pid': '${Platform.numberOfProcessors} cpus',
    'os': Platform.operatingSystem,
  });
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
  runGuardedApp(() {
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
    );
  }
}
