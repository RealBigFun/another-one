// Riverpod surface for the project page's Configuration panel.
//
// `branchSettingsProvider(projectId)` is a one-shot fetch of the
// resolved settings (configured + effective values + available
// branches). Callers `ref.invalidate` after a mutation to refetch.
//
// The panel's expand/collapse state and which dropdown is open are
// kept in-memory — same lifetime as GPUI's `project_page_config_*`
// fields on AnotherOneApp (resets on app restart).

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show ResolvedProjectBranchSettingsDto;
import 'local_connection_provider.dart';

final branchSettingsProvider =
    FutureProvider.family<ResolvedProjectBranchSettingsDto?, String>(
  (ref, projectId) async {
    final connection = ref.watch(localConnectionProvider);
    try {
      return await connection.readBranchSettings(projectId);
    } on UnimplementedError {
      return null;
    }
  },
);

/// Whether the Configuration panel header is expanded. Mirrors
/// GPUI's `project_page_config_panel_expanded` — same default
/// (collapsed) and same in-memory lifetime.
final configurationPanelExpandedProvider = StateProvider<bool>((_) => false);

/// Which branch-setting dropdown is open, if any. Only one row's
/// dropdown is visible at a time; opening a second one closes the
/// first. Matches GPUI's `project_page_config_dropdown:
/// `Option<ProjectBranchSettingField>`.
enum BranchSettingField { defaultBranch, defaultTargetBranch }

class _BranchSettingDropdownNotifier extends StateNotifier<BranchSettingField?> {
  _BranchSettingDropdownNotifier() : super(null);

  void toggle(BranchSettingField field) {
    state = state == field ? null : field;
  }

  void close() => state = null;
}

final branchSettingDropdownProvider = StateNotifierProvider<
    _BranchSettingDropdownNotifier, BranchSettingField?>(
  (_) => _BranchSettingDropdownNotifier(),
);
