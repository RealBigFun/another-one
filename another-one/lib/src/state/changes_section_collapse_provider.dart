// In-memory collapse state for the right-sidebar Changes pane's
// section headers. Mirrors GPUI's `collapsed_change_sections`
// HashSet — same lifetime: persists for the running session, resets
// across app restarts.
//
// Keys are scoped to "{projectId}:{sectionKey}" so multiple
// projects' panes don't share state when the active project flips.

import 'package:flutter_riverpod/flutter_riverpod.dart';

class _ChangesSectionCollapseNotifier extends StateNotifier<Set<String>> {
  _ChangesSectionCollapseNotifier() : super(const {});

  void toggle(String key) {
    state = state.contains(key)
        ? (state.toSet()..remove(key))
        : (state.toSet()..add(key));
  }
}

final changesSectionCollapseProvider = StateNotifierProvider<
    _ChangesSectionCollapseNotifier, Set<String>>(
  (_) => _ChangesSectionCollapseNotifier(),
);
