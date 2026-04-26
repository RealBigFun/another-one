// FutureProvider for the right sidebar's Compare pane data.
//
// Keyed by (projectId, targetBranch); Riverpod caches per key so
// flipping branches doesn't re-shell-out unless something
// mutates and the caller invalidates explicitly.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show BranchCompareView;
import 'local_connection_provider.dart';

class BranchCompareKey {
  const BranchCompareKey({
    required this.projectId,
    required this.targetBranch,
  });

  final String projectId;
  final String targetBranch;

  @override
  bool operator ==(Object other) =>
      other is BranchCompareKey &&
      other.projectId == projectId &&
      other.targetBranch == targetBranch;

  @override
  int get hashCode => Object.hash(projectId, targetBranch);
}

final branchCompareProvider =
    FutureProvider.family<BranchCompareView?, BranchCompareKey>((ref, key) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.readBranchCompareState(
      projectId: key.projectId,
      targetBranch: key.targetBranch,
    );
  } on UnimplementedError {
    return null;
  }
});
