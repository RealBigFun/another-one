// Right sidebar visibility + active-tab state.
//
// Mirrors `desktop/src/app.rs::RightSidebarMode` (WorkingTree /
// Commits / Checks / Compare). Stores the picked tab between
// app launches via SharedPreferences so the panel reopens to the
// last-used pane.

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

enum RightSidebarTab { changes, commits, checks, compare }

const String _prefsOpenKey = 'desktop.right_sidebar.open';
const String _prefsTabKey = 'desktop.right_sidebar.tab';

class _RightSidebarOpenNotifier extends StateNotifier<bool> {
  _RightSidebarOpenNotifier() : super(true) {
    _restore();
  }

  Future<void> _restore() async {
    final prefs = await SharedPreferences.getInstance();
    final stored = prefs.getBool(_prefsOpenKey);
    if (stored != null && stored != state) state = stored;
  }

  void set(bool value) {
    state = value;
    SharedPreferences.getInstance().then((p) => p.setBool(_prefsOpenKey, value));
  }

  void toggle() => set(!state);
}

class _RightSidebarTabNotifier extends StateNotifier<RightSidebarTab> {
  _RightSidebarTabNotifier() : super(RightSidebarTab.changes) {
    _restore();
  }

  Future<void> _restore() async {
    final prefs = await SharedPreferences.getInstance();
    final raw = prefs.getString(_prefsTabKey);
    final restored = RightSidebarTab.values.firstWhere(
      (t) => t.name == raw,
      orElse: () => RightSidebarTab.changes,
    );
    if (restored != state) state = restored;
  }

  void set(RightSidebarTab value) {
    state = value;
    SharedPreferences.getInstance()
        .then((p) => p.setString(_prefsTabKey, value.name));
  }
}

final rightSidebarOpenProvider =
    StateNotifierProvider<_RightSidebarOpenNotifier, bool>(
  (_) => _RightSidebarOpenNotifier(),
);

final rightSidebarTabProvider =
    StateNotifierProvider<_RightSidebarTabNotifier, RightSidebarTab>(
  (_) => _RightSidebarTabNotifier(),
);
