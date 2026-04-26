// FutureProvider for the active project's branch metadata —
// current branch + ahead/behind counts. Drives the titlebar's
// idle primary-action selection (Push when ahead, Pull when
// behind, etc.).
//
// One-shot fetch per project; the toolbar invalidates after a
// run_toolbar_git_action call when the outcome's
// refresh_git_state flag is set.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show ActiveGitStateDto;
import 'local_connection_provider.dart';

final activeGitStateProvider =
    FutureProvider.family<ActiveGitStateDto?, String>((ref, projectId) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.readActiveGitState(projectId);
  } on UnimplementedError {
    return null;
  }
});
