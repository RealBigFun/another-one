// Derives the currently-active project id from whichever surface
// owns the focus.
//
// The desktop has two mutually-exclusive focus modes:
//
//   * A tab is selected — `selectedTabProvider`'s `(sectionId,
//     tabId)`. The project id is the one whose task carries the
//     matching `sectionId`.
//   * A project's overview page is focused —
//     `activeProjectPageProvider` holds the id directly.
//
// Surfaces useful for the titlebar (Open In / GitHub / Git Actions),
// the right sidebar (Changes / Commits / Checks), and any other
// chrome that hangs off "the project we're looking at right now".
//
// Returns `null` when neither focus is set, or when the tab refers
// to a project that's no longer in the list (race during removal).

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'active_project_page_provider.dart';
import 'local_connection_provider.dart';
import 'tab_selection_provider.dart';

final activeProjectIdProvider = Provider<String?>((ref) {
  final selection = ref.watch(selectedTabProvider);
  if (selection != null) {
    final projects =
        ref.watch(desktopProjectsProvider).valueOrNull ?? const [];
    for (final project in projects) {
      for (final task in project.tasks) {
        if (task.sectionId == selection.sectionId) {
          return project.id;
        }
      }
    }
  }
  return ref.watch(activeProjectPageProvider);
});

/// Branch name in focus right now — the active task's branch when a
/// task is selected, or the project's current branch otherwise.
/// Mirrors GPUI's fallback chain in `branch_commit_row`'s caller
/// (`commit_state.current_branch.unwrap_or(active_section.branch_name)`).
/// Returns `null` when nothing is focused or the project has no
/// current branch (detached HEAD, fresh clone).
final activeBranchNameProvider = Provider<String?>((ref) {
  final projects =
      ref.watch(desktopProjectsProvider).valueOrNull ?? const [];
  final selection = ref.watch(selectedTabProvider);
  if (selection != null) {
    for (final project in projects) {
      for (final task in project.tasks) {
        if (task.sectionId == selection.sectionId) {
          return task.branchName;
        }
      }
    }
  }
  final pageId = ref.watch(activeProjectPageProvider);
  if (pageId != null) {
    for (final project in projects) {
      if (project.id == pageId) {
        return project.currentBranch;
      }
    }
  }
  return null;
});
