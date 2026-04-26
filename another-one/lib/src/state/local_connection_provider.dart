// Riverpod root for the desktop's loopback iroh `DaemonConnection`.
//
// Per the single-shared-implementation ADR
// (`docs/architecture/single-shared-implementation.md`,
// `another-one-arc`), the desktop UI talks to its own embedded
// daemon over iroh — same wire surface mobile uses against a remote
// daemon. The pre-ojm.9 path constructed a `LocalTransport` over an
// FFI session; this provider now constructs an `IrohTransport`
// against the daemon's published endpoint address. The only
// difference between desktop and mobile is which address we dial.
//
// The loopback address comes from
// [`loopbackSessionAddrProvider`], which `main.dart` overrides with
// the value of `awaitLoopbackSessionAddr()` after
// `bootEmbeddedDaemon()` returns and the daemon thread has bound
// its iroh endpoint. Reading this provider on a host that hasn't
// performed that override (e.g. mobile, where no daemon boots)
// throws — that's by design, because mobile doesn't read this
// provider.
//
// Self-trust: the daemon pre-allowlists the device's own NodeId at
// boot (see `another-one-bridge::embedded_daemon::run`), so the
// loopback dial skips the TOFU `Hello` dance and the user-facing
// pair nonce stays available for actual mobile pairing.

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../connection.dart';
import '../rust/api/embedded_daemon.dart' show LoopbackSessionAddr;
import '../rust/api/iroh_client.dart';
import '../transport.dart';
import '../transport_iroh.dart';
import 'connection_manager_provider.dart';

/// The embedded daemon's iroh address, populated once at boot.
/// `main.dart` overrides this with the awaited
/// `awaitLoopbackSessionAddr()` result before constructing the
/// root `ProviderScope`. Reading the unoverridden provider throws,
/// which is the desired failure mode on hosts that don't boot a
/// local daemon (mobile clients connect to a paired remote
/// instead).
final loopbackSessionAddrProvider = Provider<LoopbackSessionAddr>((ref) {
  throw StateError(
    'loopbackSessionAddrProvider read without an override — desktop '
    'main() must override this with awaitLoopbackSessionAddr() before '
    'building the ProviderScope. Mobile clients should not read this.',
  );
});

/// The desktop's loopback `DaemonConnection`, exposed as the abstract
/// type so the UI doesn't depend on the concrete transport class.
/// Created lazily on first read, connected immediately, and registered
/// in the [`connectionManagerProvider`].
///
/// Idempotent: a second read returns the same instance. The provider
/// is `keepAlive` by default since it's read by the desktop shell
/// for the entire app lifetime.
final localConnectionProvider = Provider<DaemonConnection>((ref) {
  final addr = ref.watch(loopbackSessionAddrProvider);
  final transport = IrohTransport(
    addr.endpointId,
    directAddrs: addr.directAddrs,
    relayUrls: addr.relayUrls,
    // No pair token: the embedded daemon allowlisted this device's
    // NodeId at boot, so the dial skips the TOFU Hello dance.
    pairToken: null,
    displayNameOverride: 'Local',
  );
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
