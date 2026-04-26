// In-memory expand/collapse state for the right-sidebar Commits
// pane's per-commit rows. Mirrors GPUI's
// `commit_row_expanded` predicate (backed by a HashSet on
// AnotherOneApp), same lifetime: persists for the running session,
// resets across app restarts.
//
// Keys are scoped to "{projectId}:{commitId}" so different projects'
// expand state doesn't collide.

import 'package:flutter_riverpod/flutter_riverpod.dart';

class _CommitRowExpandedNotifier extends StateNotifier<Set<String>> {
  _CommitRowExpandedNotifier() : super(const {});

  void toggle(String key) {
    state = state.contains(key)
        ? (state.toSet()..remove(key))
        : (state.toSet()..add(key));
  }
}

final commitRowExpandedProvider =
    StateNotifierProvider<_CommitRowExpandedNotifier, Set<String>>(
  (_) => _CommitRowExpandedNotifier(),
);
