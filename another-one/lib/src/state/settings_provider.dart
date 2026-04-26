// Settings page open/closed + active section.
//
// `settingsOpenProvider` flips to `true` when the sidebar footer
// gear is clicked, displacing the main pane until the user clicks
// "Back to app". `settingsSectionProvider` tracks which sub-page
// is rendered on the right (Agents / Open In / Git Actions /
// Keybindings / MCP).

import 'package:flutter_riverpod/flutter_riverpod.dart';

enum SettingsSection {
  agents,
  openIn,
  gitActions,
  keybindings,
  mcp,
}

extension SettingsSectionLabel on SettingsSection {
  String get label => switch (this) {
        SettingsSection.agents => 'Agents',
        SettingsSection.openIn => 'Open In',
        SettingsSection.gitActions => 'Git Actions',
        SettingsSection.keybindings => 'Keybindings',
        SettingsSection.mcp => 'MCP',
      };
}

final settingsOpenProvider = StateProvider<bool>((_) => false);

final settingsSectionProvider =
    StateProvider<SettingsSection>((_) => SettingsSection.agents);
