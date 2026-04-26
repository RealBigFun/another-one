// FutureProvider for per-commit file changes — the data source
// behind the Commits pane's expandable rows. Lazy: only fetches
// when a row gets expanded for the first time. Riverpod caches by
// (projectId, commitId) so repeated expand/collapse cycles don't
// re-shell-out.
//
// Mirrors GPUI's `commit_file_changes_states` map, which spawns a
// background worker per commit on first expand and caches the
// result indefinitely. The Riverpod cache here behaves the same
// way for the session — `ref.invalidate` to force a refetch.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show BranchCompareFileDto;
import 'local_connection_provider.dart';

class CommitFileChangesKey {
  const CommitFileChangesKey({
    required this.projectId,
    required this.commitId,
  });

  final String projectId;
  final String commitId;

  @override
  bool operator ==(Object other) =>
      other is CommitFileChangesKey &&
      other.projectId == projectId &&
      other.commitId == commitId;

  @override
  int get hashCode => Object.hash(projectId, commitId);
}

final commitFileChangesProvider = FutureProvider.family<
    List<BranchCompareFileDto>?, CommitFileChangesKey>((ref, key) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.readCommitFileChanges(
      projectId: key.projectId,
      commitId: key.commitId,
    );
  } on UnimplementedError {
    return null;
  }
});
