// Riverpod root for the desktop's local (in-process) daemon
// connection.
//
// On desktop platforms `main.dart` boots an embedded daemon via
// `bootEmbeddedDaemon()`. This provider builds a `LocalTransport`
// over that daemon, registers it in the `ConnectionManager`, and
// kicks off a project-list fetch.
//
// Mobile platforms never read this provider — they hold an
// `IrohTransport` instead. Reading it on mobile returns a
// disconnected transport that no-ops on every call; cheaper than
// gating every read site with a platform check, and the UI never
// surfaces it on mobile anyway.
//
// Lifecycle: tied to the provider container. `ref.onDispose`
// closes the transport, which detaches any attached tab and drops
// the FFI session — the daemon thread keeps running (it's owned by
// the bridge's `OnceLock`), but no Dart-side resources leak.

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/iroh_client.dart';
import '../transport.dart';
import '../transport_local.dart';
import 'connection_manager_provider.dart';

/// The desktop's `LocalTransport`. Created lazily on first read,
/// connected immediately, and registered in the
/// [`connectionManagerProvider`].
///
/// Idempotent: a second read returns the same instance. The provider
/// is `keepAlive` by default since it's read by the desktop shell
/// for the entire app lifetime.
final localConnectionProvider = Provider<LocalTransport>((ref) {
  final transport = LocalTransport();
  ref.read(connectionManagerProvider).add(transport);
  transport.connect();
  // First project-list fetch as soon as the transport reports
  // connected. Without this the sidebar starts empty even when the
  // daemon already has projects loaded.
  late final StreamSubscription<TransportStatus> sub;
  sub = transport.status.listen((status) {
    if (status.state == TransportState.connected) {
      transport.listProjects();
      sub.cancel();
    }
  });
  ref.onDispose(() {
    sub.cancel();
    transport.close();
  });
  return transport;
});

/// Stream of project lists as they arrive from the local daemon.
/// Re-emits whenever the daemon publishes a new `ProjectList`
/// (after a `listProjects()` call, or as projects mutate). Returns
/// an empty list before the first reply.
final desktopProjectsProvider = StreamProvider<List<ProjectSummary>>((ref) {
  final transport = ref.watch(localConnectionProvider);
  final controller = StreamController<List<ProjectSummary>>();
  controller.add(const []);
  final sub = transport.workerReplies.listen((reply) {
    if (reply is WorkerReply_ProjectList) {
      controller.add(reply.projects);
    }
  });
  ref.onDispose(() {
    sub.cancel();
    controller.close();
  });
  return controller.stream;
});
