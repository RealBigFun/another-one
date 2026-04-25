// Riverpod root for the multi-daemon connection manager.
//
// `ConnectionManager` (in `../connection.dart`) holds N
// `DaemonConnection`s — local FFI today, plus iroh peers as the
// migration progresses. Screens read it through this provider so
// the same instance flows everywhere without prop-drilling.
//
// Riverpod codegen is currently held back: `riverpod_generator`'s
// `source_gen ^2.0.0` pin conflicts with `freezed` 3.2.x's
// `source_gen >=3.0.0` (FRB pulls freezed transitively). Once
// upstream lifts the pin we'll switch this file over to
// `@Riverpod(keepAlive: true)`. Until then, plain `Provider`s are
// fine — the state taxonomy is small enough.
//
// Lifecycle: `keepAlive` would be a no-op here since the provider
// has no listeners until screens subscribe; the manager itself owns
// the close-all semantics via `ConnectionManager.closeAll()`.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../connection.dart';

/// The app-wide [ConnectionManager]. Keep one instance for the
/// lifetime of the process — closing it would tear down every live
/// daemon link.
final connectionManagerProvider = Provider<ConnectionManager>((ref) {
  final manager = ConnectionManager();
  ref.onDispose(() {
    // Best-effort: dispose only fires on container teardown (test
    // tear-downs, hot-restart). The fire-and-forget is intentional;
    // riverpod's onDispose is sync.
    manager.closeAll();
  });
  return manager;
});

/// Live list of registered [DaemonConnection]s. Re-emits on every
/// add/remove so the host switcher can rebuild without manual
/// `setState`.
final connectionsProvider = StreamProvider<List<DaemonConnection>>((ref) {
  final manager = ref.watch(connectionManagerProvider);
  return manager.changes;
});
