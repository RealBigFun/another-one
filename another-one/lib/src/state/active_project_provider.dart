// Derives the currently-active project id from the selected tab.
//
// The desktop's `TabSelection` is `(sectionId, tabId)`; the project
// id is reachable by walking the project list and matching a task's
// `sectionId`. Surfaces useful for the titlebar (Open In / GitHub /
// Git Actions all hang off the active project).
//
// Returns `null` when no tab is selected or the selection refers to
// a project that's no longer in the list.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'local_connection_provider.dart';
import 'tab_selection_provider.dart';

final activeProjectIdProvider = Provider<String?>((ref) {
  final selection = ref.watch(selectedTabProvider);
  if (selection == null) return null;
  final projects =
      ref.watch(desktopProjectsProvider).valueOrNull ?? const [];
  for (final project in projects) {
    for (final task in project.tasks) {
      if (task.sectionId == selection.sectionId) {
        return project.id;
      }
    }
  }
  return null;
});
