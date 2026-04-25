// Currently-focused (section, tab) on the desktop's main pane.
//
// Sidebar clicks set this; the main area watches it and renders the
// matching terminal. `null` means "no tab selected" — the welcome
// placeholder stays visible.
//
// This is a per-window selection; if/when multi-window support
// lands, scope this provider to a window-id family. Today the
// Flutter desktop is single-window, so a top-level `StateProvider`
// is fine.

import 'package:flutter_riverpod/flutter_riverpod.dart';

class TabSelection {
  const TabSelection({required this.sectionId, required this.tabId});

  final String sectionId;
  final String tabId;

  @override
  bool operator ==(Object other) =>
      other is TabSelection &&
      other.sectionId == sectionId &&
      other.tabId == tabId;

  @override
  int get hashCode => Object.hash(sectionId, tabId);
}

final selectedTabProvider = StateProvider<TabSelection?>((_) => null);
