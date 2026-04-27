// Riverpod surface for the right sidebar's Changes pane.
//
// `changedFilesProvider(projectId)` snapshots the working-tree
// state of a project — the list of files with index/worktree
// status + diff counts. It still supports invalidation-driven
// refetches, but it can also accept the daemon's inline mutation
// snapshots directly so stage/unstage/discard actions do not need an
// immediate second `readChangedFiles()` round trip.

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../connection.dart';
import '../rust/api/local_session.dart' show ChangedFileDto;
import 'local_connection_provider.dart';

class _ChangedFilesNotifier
    extends StateNotifier<AsyncValue<List<ChangedFileDto>>> {
  _ChangedFilesNotifier(this.ref, this.projectId)
    : super(const AsyncLoading()) {
    unawaited(refresh());
  }

  final Ref ref;
  final String projectId;

  Future<void> refresh() async {
    state = const AsyncLoading();
    state = await AsyncValue.guard(() async {
      final connection = ref.read(localConnectionProvider);
      return _readChangedFiles(connection);
    });
  }

  void replaceSnapshot(List<ChangedFileDto> files) {
    state = AsyncData(files);
  }

  Future<List<ChangedFileDto>> _readChangedFiles(
    DaemonConnection connection,
  ) async {
    try {
      return await connection.readChangedFiles(projectId) ?? const [];
    } on UnimplementedError {
      return const [];
    }
  }
}

final changedFilesProvider =
    StateNotifierProvider.family<
      _ChangedFilesNotifier,
      AsyncValue<List<ChangedFileDto>>,
      String
    >((ref, projectId) => _ChangedFilesNotifier(ref, projectId));
