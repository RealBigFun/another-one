import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/iroh_client.dart';
import 'local_connection_provider.dart';
import 'tab_selection_provider.dart';

TabSelection? resolveLiveTabSelection(
  List<ProjectSummary> projects,
  TabSelection? selection,
) {
  if (selection == null) return null;
  final selectedTask = _findTaskBySectionId(projects, selection.sectionId);
  if (selectedTask != null) {
    final tabId = _resolveTabId(selectedTask, preferredTabId: selection.tabId);
    if (tabId != null) {
      return TabSelection(sectionId: selectedTask.sectionId, tabId: tabId);
    }
  }
  return _fallbackTabSelection(projects);
}

final resolvedSelectedTabProvider = Provider<TabSelection?>((ref) {
  final selection = ref.watch(selectedTabProvider);
  if (selection == null) return null;
  final loaded = ref.watch(desktopProjectsLoadedProvider).valueOrNull ?? false;
  if (!loaded) return selection;
  final projects = ref.watch(desktopProjectsProvider).valueOrNull ?? const [];
  return resolveLiveTabSelection(projects, selection);
});

TaskSummary? _findTaskBySectionId(
  List<ProjectSummary> projects,
  String sectionId,
) {
  for (final project in projects) {
    for (final task in project.tasks) {
      if (task.sectionId == sectionId) return task;
    }
  }
  return null;
}

TabSelection? _fallbackTabSelection(List<ProjectSummary> projects) {
  for (final project in projects) {
    for (final task in project.tasks) {
      final tabId = _resolveTabId(task);
      if (tabId != null) {
        return TabSelection(sectionId: task.sectionId, tabId: tabId);
      }
    }
  }
  return null;
}

String? _resolveTabId(TaskSummary task, {String? preferredTabId}) {
  if (preferredTabId != null &&
      task.tabs.any((tab) => tab.id == preferredTabId)) {
    return preferredTabId;
  }
  if (task.activeTabId.isNotEmpty &&
      task.tabs.any((tab) => tab.id == task.activeTabId)) {
    return task.activeTabId;
  }
  if (task.tabs.isNotEmpty) {
    return task.tabs.first.id;
  }
  return null;
}
