// Riverpod surface for the right sidebar's Commits pane.
//
// `recentCommitsProvider(projectId)` snapshots the most recent
// commits on the project's current branch — branch name + list +
// has-more flag. Calls into the active connection's
// `readRecentCommits`, which goes through
// `read_project_branch_commit_state` on the daemon side.
//
// The page-size constant matches GPUI's
// `RECENT_COMMITS_PAGE_SIZE` so the two clients show the same
// cutoff. Future "Load more" pagination just bumps this on the
// caller side and re-fetches.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show RecentCommitsView;
import 'local_connection_provider.dart';

const int kRecentCommitsPageSize = 25;

final recentCommitsProvider =
    FutureProvider.family<RecentCommitsView?, String>((ref, projectId) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.readRecentCommits(
      projectId: projectId,
      limit: kRecentCommitsPageSize,
    );
  } on UnimplementedError {
    return null;
  }
});
