// Riverpod surface for the right sidebar's Commits pane.
//
// `recentCommitsProvider(projectId)` snapshots the most recent
// commits on the project's current branch — branch name + list +
// has-more flag. Calls into the active connection's
// `readRecentCommits`, which goes through
// `read_project_branch_commit_state` on the daemon side.
//
// Page size mirrors GPUI's `commit_page_size_for_project` (default
// `RECENT_COMMITS_PAGE_SIZE`, bumped by `load_more_commits` in
// `RECENT_COMMITS_PAGE_SIZE` increments). The "Load more" button
// in the pane bumps `commitPageSizeProvider(projectId)` and
// `ref.invalidate`s `recentCommitsProvider` to refetch.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show RecentCommitsView;
import 'local_connection_provider.dart';

const int kRecentCommitsPageSize = 25;

final commitPageSizeProvider = StateProvider.family<int, String>(
  (ref, projectId) => kRecentCommitsPageSize,
);

final recentCommitsProvider =
    FutureProvider.family<RecentCommitsView?, String>((ref, projectId) async {
  final connection = ref.watch(localConnectionProvider);
  final limit = ref.watch(commitPageSizeProvider(projectId));
  try {
    return await connection.readRecentCommits(
      projectId: projectId,
      limit: limit,
    );
  } on UnimplementedError {
    return null;
  }
});
