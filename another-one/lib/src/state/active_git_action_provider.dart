// In-memory tracker for which titlebar git action is currently
// running, scoped per project. Mirrors GPUI's
// `active_git_action: Option<ToolbarGitAction>` — drives:
//
//   * Spinner + action-specific label on the split-button body
//     ('Committing...', 'Force Pushing...', etc.)
//   * Disabled state on dropdown rows (toolbar_enabled gate)
//   * Danger-tinted text/icon when the running action is
//     destructive (force push, undo last commit)
//
// Lives outside the widget so the dropdown body and the dropdown
// rows can both observe it consistently. Resets to `null` after
// the bridge call resolves (success or failure).

import 'package:flutter_riverpod/flutter_riverpod.dart';

class _ActiveGitActionNotifier extends StateNotifier<String?> {
  _ActiveGitActionNotifier() : super(null);

  void start(String actionId) => state = actionId;
  void clear() => state = null;
}

final activeGitActionProvider = StateNotifierProvider.family<
    _ActiveGitActionNotifier, String?, String>(
  (_, projectId) => _ActiveGitActionNotifier(),
);
