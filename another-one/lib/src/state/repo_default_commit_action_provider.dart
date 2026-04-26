// FutureProvider for the user's preferred default commit action on
// the active project's root repo. Either `"commit"` /
// `"commit-and-push"` / null. Drives the titlebar primary-action
// selection (when there are unstaged changes, GPUI picks Commit
// vs Commit & Push by this preference).

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'local_connection_provider.dart';

final repoDefaultCommitActionProvider =
    FutureProvider.family<String?, String>((ref, projectId) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.repoDefaultCommitAction(projectId);
  } on UnimplementedError {
    return null;
  }
});
