// FutureProvider for the active project's current-branch PR
// status. Drives the titlebar git-actions dropdown's Create PR /
// Draft PR enabledness — when an open PR already exists for the
// branch, those rows are disabled.
//
// Refreshed on dropdown open (matches GPUI's
// refresh_active_project_pull_request_lookup) — callers
// `ref.invalidate` to re-fetch.

import '../rust/api/local_session.dart' show PullRequestStatusDto;
import 'connection_future_provider.dart';

final pullRequestStatusProvider =
    makeConnectionFutureProviderFamily<PullRequestStatusDto?, String>(
      read: (_, connection, projectId) =>
          connection.findPullRequestStatus(projectId),
      fallback: null,
    );
