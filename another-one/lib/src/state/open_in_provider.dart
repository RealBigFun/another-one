// Riverpod surface for the host's "Open In" configuration.
//
// `openInStateProvider` returns a one-shot snapshot of the
// installed-and-enabled apps + the preferred default. Cheap to
// recompute (it's a `$PATH` walk on Linux and a project-store read),
// so callers that want to reflect a freshly-set preference just
// `ref.invalidate(openInStateProvider)` after the mutation — no
// streaming subscription needed.
//
// `openProjectInActiveApp` and `openProjectInApp` are action helpers
// the titlebar dispatches; they call through to the active
// connection and invalidate the state provider so the button's
// primary icon updates immediately.
//
// Open-In is daemon-host-local: reads and launches round-trip to the
// host that owns the project. The provider still falls back to an
// empty state on transports that do not expose Open-In, matching
// GPUI's "hide the button when no apps are enabled" behaviour.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../connection.dart';
import '../rust/api/local_session.dart' show OpenInState;
import '../transport.dart';
import 'local_connection_provider.dart';

/// Fetches a fresh `OpenInState` from the active daemon connection.
/// Falls back to an empty state for transports that don't expose
/// Open-In so consumers can render uniformly.
Future<void> waitForConnectedDaemon(DaemonConnection connection) async {
  final current = connection.currentStatus;
  if (current.isConnected) {
    return;
  }
  if (current.state != TransportState.connecting) {
    throw StateError('Daemon not connected: ${current.label}');
  }
  final status = await connection.status.firstWhere(
    (status) => status.state != TransportState.connecting,
  );
  if (!status.isConnected) {
    throw StateError('Daemon not connected: ${status.label}');
  }
}

final openInStateProvider = FutureProvider<OpenInState>((ref) async {
  final connection = ref.watch(localConnectionProvider);
  await waitForConnectedDaemon(connection);
  try {
    return await connection.openInState();
  } on UnimplementedError {
    return const OpenInState(enabledApps: [], preferredAppId: null);
  }
});
