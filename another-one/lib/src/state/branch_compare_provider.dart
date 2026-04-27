// FutureProvider for the right sidebar's Compare pane data.
//
// Keyed by (projectId, targetBranch); Riverpod caches per key so
// flipping branches doesn't re-shell-out unless something
// mutates and the caller invalidates explicitly.

import '../rust/api/local_session.dart' show BranchCompareView;
import 'connection_future_provider.dart';

class BranchCompareKey {
  const BranchCompareKey({required this.projectId, required this.targetBranch});

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
    makeConnectionFutureProviderFamily<BranchCompareView?, BranchCompareKey>(
      read: (_, connection, key) => connection.readBranchCompareState(
        projectId: key.projectId,
        targetBranch: key.targetBranch,
      ),
      fallback: null,
    );
