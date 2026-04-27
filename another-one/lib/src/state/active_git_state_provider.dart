// FutureProvider for the active project's branch metadata —
// current branch + ahead/behind counts. Drives the titlebar's
// idle primary-action selection (Push when ahead, Pull when
// behind, etc.).
//
// One-shot fetch per project; the toolbar invalidates after a
// run_toolbar_git_action call when the outcome's
// refresh_git_state flag is set.

import '../rust/api/local_session.dart' show ActiveGitStateDto;
import 'connection_future_provider.dart';

final activeGitStateProvider =
    makeConnectionFutureProviderFamily<ActiveGitStateDto?, String>(
      read: (_, connection, projectId) =>
          connection.readActiveGitState(projectId),
      fallback: null,
    );
