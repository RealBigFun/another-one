// In-flight tracker for the right-sidebar Changes pane's git
// mutations. Mirrors GPUI's `actions_busy` / `file_pending` /
// `project_mutations_pending` / `stage_all_pending` /
// `unstage_all_pending` predicates on `AnotherOneApp`.
//
// Per-file pending: paths currently mid-action (stage, unstage,
// discard).
// Project-wide pending: which whole-project mutation (stage all /
// unstage all / discard all) is mid-action. Multiple per-file
// actions can run concurrently in principle, but only one
// project-wide one at a time.
//
// Behavior driven by these flags:
//   * actionsBusy → action buttons render at 0.35 opacity / no
//     hover; matches GPUI's `enabled: !actions_busy` gate.
//   * filePending(path) → that row's action button gets replaced
//     with a spinner.
//   * stageAllPending / unstageAllPending → the matching section
//     header's all-action button gets replaced with a spinner.
//   * projectMutationsPending → per-file discard buttons disabled
//     (a project-wide mutation in flight blocks per-file too,
//     matching `enabled: !actions_busy && !project_mutations_pending`).

import 'package:flutter_riverpod/flutter_riverpod.dart';

enum ProjectAction { stageAll, unstageAll, discardAll }

class ChangedFilesPending {
  const ChangedFilesPending({
    this.files = const {},
    this.projectActions = const {},
  });

  final Set<String> files;
  final Set<ProjectAction> projectActions;

  bool get actionsBusy => files.isNotEmpty || projectActions.isNotEmpty;
  bool get projectMutationsPending => projectActions.isNotEmpty;

  bool isFilePending(String path) => files.contains(path);
  bool isProjectActionPending(ProjectAction action) =>
      projectActions.contains(action);

  ChangedFilesPending withFile(String path, {required bool pending}) {
    final next = files.toSet();
    if (pending) {
      next.add(path);
    } else {
      next.remove(path);
    }
    return ChangedFilesPending(files: next, projectActions: projectActions);
  }

  ChangedFilesPending withProjectAction(
    ProjectAction action, {
    required bool pending,
  }) {
    final next = projectActions.toSet();
    if (pending) {
      next.add(action);
    } else {
      next.remove(action);
    }
    return ChangedFilesPending(files: files, projectActions: next);
  }
}

class _ChangedFilesPendingNotifier extends StateNotifier<ChangedFilesPending> {
  _ChangedFilesPendingNotifier() : super(const ChangedFilesPending());

  void startFile(String path) =>
      state = state.withFile(path, pending: true);
  void endFile(String path) =>
      state = state.withFile(path, pending: false);

  void startProject(ProjectAction action) =>
      state = state.withProjectAction(action, pending: true);
  void endProject(ProjectAction action) =>
      state = state.withProjectAction(action, pending: false);
}

final changedFilesPendingProvider = StateNotifierProvider.family<
    _ChangedFilesPendingNotifier, ChangedFilesPending, String>(
  (_, projectId) => _ChangedFilesPendingNotifier(),
);
