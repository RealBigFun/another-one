import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'active_git_state_provider.dart';
import 'changed_files_provider.dart';
import 'pull_request_status_provider.dart';
import 'recent_commits_provider.dart';

/// Invalidate the shared git-derived surfaces that depend on the
/// active project's branch state. Titlebar git actions and the right
/// sidebar's undo flow both use this so commits, ahead/behind state,
/// pull-request status, and changed-files views stay in sync after a
/// mutating action completes.
void invalidateGitRefreshState(WidgetRef ref, String projectId) {
  ref.invalidate(changedFilesProvider(projectId));
  ref.invalidate(recentCommitsProvider(projectId));
  ref.invalidate(activeGitStateProvider(projectId));
  ref.invalidate(pullRequestStatusProvider(projectId));
}
