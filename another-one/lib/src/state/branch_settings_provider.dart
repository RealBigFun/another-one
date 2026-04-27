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
import 'connection_future_provider.dart';

final branchSettingsProvider =
    makeConnectionFutureProviderFamily<
      ResolvedProjectBranchSettingsDto?,
      String
    >(
      read: (_, connection, projectId) =>
          connection.readBranchSettings(projectId),
      fallback: null,
    );

/// Whether the Configuration panel header is expanded. Mirrors
/// GPUI's `project_page_config_panel_expanded` — initialised to
/// `true` in `AnotherOneApp::new` (`app.rs:3622`), so the panel
/// shows the Default Branch / Default Target Branch rows on first
/// paint of every project page.
final configurationPanelExpandedProvider = StateProvider<bool>((_) => true);

/// Which branch-setting dropdown is open, if any. Only one row's
/// dropdown is visible at a time; opening a second one closes the
/// first. Matches GPUI's `project_page_config_dropdown:
/// `Option<ProjectBranchSettingField>`.
enum BranchSettingField { defaultBranch, defaultTargetBranch }

class _BranchSettingDropdownNotifier
    extends StateNotifier<BranchSettingField?> {
  _BranchSettingDropdownNotifier() : super(null);

  void toggle(BranchSettingField field) {
    state = state == field ? null : field;
  }

  void close() => state = null;
}

final branchSettingDropdownProvider =
    StateNotifierProvider<_BranchSettingDropdownNotifier, BranchSettingField?>(
      (_) => _BranchSettingDropdownNotifier(),
    );
