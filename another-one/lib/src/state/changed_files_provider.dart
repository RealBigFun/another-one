// Riverpod surface for the right sidebar's Changes pane.
//
// `changedFilesProvider(projectId)` snapshots the working-tree
// state of a project — the list of files with index/worktree
// status + diff counts. Calls into the active connection's
// `readChangedFiles`, which goes through `read_project_git_state`
// on the daemon side (spawn_blocking under the hood, so the FRB
// caller's tokio runtime stays free).
//
// One-shot, not streaming: a future commit can wire up periodic
// refresh or a daemon push, but for now the UI invalidates the
// provider after a known-mutation (commit, branch switch) and on
// pane-show.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show ChangedFileDto;
import 'local_connection_provider.dart';

final changedFilesProvider =
    FutureProvider.family<List<ChangedFileDto>, String>((ref, projectId) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    final files = await connection.readChangedFiles(projectId);
    return files ?? const [];
  } on UnimplementedError {
    return const [];
  }
});
