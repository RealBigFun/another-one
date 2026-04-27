// FutureProvider for the user's preferred default commit action on
// the active project's root repo. Either `"commit"` /
// `"commit-and-push"` / null. Drives the titlebar primary-action
// selection (when there are unstaged changes, GPUI picks Commit
// vs Commit & Push by this preference).

import 'connection_future_provider.dart';

final repoDefaultCommitActionProvider =
    makeConnectionFutureProviderFamily<String?, String>(
      read: (_, connection, projectId) =>
          connection.repoDefaultCommitAction(projectId),
      fallback: null,
    );
