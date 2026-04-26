// Riverpod surface for the right sidebar's Checks pane.
//
// `prChecksProvider(projectId)` reads the CI check runs for the
// project's current PR via `readPullRequestChecks`. Three-state
// async result mirrors the bridge's `Result<Option<List>>`:
//
//   * AsyncData(Some(list))  — the PR exists; these are its
//                              checks (possibly empty list).
//   * AsyncData(null)        — no PR for the current branch.
//   * AsyncError             — gh CLI missing, network failure.
//
// One-shot: callers `ref.invalidate` to refetch on demand. A
// streaming/poll variant can land if the pane needs live status
// without manual refresh.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show CheckDto;
import 'local_connection_provider.dart';

final prChecksProvider =
    FutureProvider.family<List<CheckDto>?, String>((ref, projectId) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.readPullRequestChecks(projectId);
  } on UnimplementedError {
    return null;
  }
});
