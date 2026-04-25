// Left sidebar visibility — collapses the project tree on demand.
//
// Mirrors `desktop/src/app.rs::sidebar_open` plus the persistence
// shape from `right_sidebar_provider.dart`: SharedPreferences-backed
// boolean restored eagerly on construction, fire-and-forget save on
// each change. Default open since first-run users haven't opted out.

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

const String _prefsOpenKey = 'desktop.left_sidebar.open';

class _LeftSidebarOpenNotifier extends StateNotifier<bool> {
  _LeftSidebarOpenNotifier() : super(true) {
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

final leftSidebarOpenProvider =
    StateNotifierProvider<_LeftSidebarOpenNotifier, bool>(
  (_) => _LeftSidebarOpenNotifier(),
);
