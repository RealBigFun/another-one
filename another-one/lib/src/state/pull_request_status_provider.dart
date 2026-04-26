// FutureProvider for the active project's current-branch PR
// status. Drives the titlebar git-actions dropdown's Create PR /
// Draft PR enabledness — when an open PR already exists for the
// branch, those rows are disabled.
//
// Refreshed on dropdown open (matches GPUI's
// refresh_active_project_pull_request_lookup) — callers
// `ref.invalidate` to re-fetch.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show PullRequestStatusDto;
import 'local_connection_provider.dart';

final pullRequestStatusProvider =
    FutureProvider.family<PullRequestStatusDto?, String>(
        (ref, projectId) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.findPullRequestStatus(projectId);
  } on UnimplementedError {
    return null;
  }
});
