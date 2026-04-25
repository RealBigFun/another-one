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
// connection (today: the local one) and invalidate the state
// provider so the button's primary icon updates immediately.
//
// Open-In is desktop-host-local (the iroh transport throws by
// design). The provider gracefully returns an empty state on
// transports that don't implement it, matching GPUI's "hide the
// button when no apps are enabled" behaviour.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show OpenInState;
import 'local_connection_provider.dart';

/// Fetches a fresh `OpenInState` from the active daemon connection.
/// Falls back to an empty state for transports that don't expose
/// Open-In (today: iroh) so consumers can render uniformly.
final openInStateProvider = FutureProvider<OpenInState>((ref) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.openInState();
  } on UnimplementedError {
    return const OpenInState(enabledApps: [], preferredAppId: null);
  }
});
