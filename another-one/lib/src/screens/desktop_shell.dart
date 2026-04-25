// Desktop shell — the top-level layout for tablet/desktop/wideDesktop
// breakpoints. Composes the chrome (titlebar, sidebar) around a main
// content slot keyed off the currently-selected tab.
//
// Submodules host the heavy lifting:
//   - `desktop_titlebar/desktop_titlebar.dart` — 38px chrome row
//   - `desktop_sidebar/desktop_sidebar.dart`   — project tree
//   - `desktop_terminal/desktop_terminal_pane.dart` — main terminal
//
// Visual + functional parity target: `desktop/src/{titlebar,
// left_sidebar,project_page}.rs`.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../state/left_sidebar_provider.dart';
import '../state/local_connection_provider.dart';
import '../state/tab_selection_provider.dart';
import '../tokens.dart';
import '../state/right_sidebar_provider.dart';
import '../widgets/empty_state.dart';
import 'desktop_right_sidebar/desktop_right_sidebar.dart';
import 'desktop_sidebar/desktop_sidebar.dart';
import 'desktop_terminal/desktop_tab_strip.dart';
import 'desktop_terminal/desktop_terminal_pane.dart';
import 'desktop_titlebar/desktop_titlebar.dart';

class DesktopShell extends ConsumerWidget {
  const DesktopShell({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // Eagerly read the local connection so the daemon-backed
    // transport spins up before anything tries to render projects.
    ref.watch(localConnectionProvider);
    final leftOpen = ref.watch(leftSidebarOpenProvider);
    final rightOpen = ref.watch(rightSidebarOpenProvider);
    return Scaffold(
      backgroundColor: AppTokens.terminalBg,
      body: Column(
        children: [
          const DesktopTitlebar(),
          Expanded(
            child: Row(
              children: [
                if (leftOpen) const DesktopSidebar(),
                const Expanded(child: _MainArea()),
                if (rightOpen) const DesktopRightSidebar(),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _MainArea extends ConsumerWidget {
  const _MainArea();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selection = ref.watch(selectedTabProvider);
    if (selection == null) {
      return Container(
        color: AppTokens.terminalBg,
        alignment: Alignment.center,
        child: const EmptyState(
          text: 'No project selected',
          padding: EdgeInsets.all(AppTokens.space10),
        ),
      );
    }
    return Column(
      children: [
        DesktopTabStrip(selection: selection),
        Expanded(child: DesktopTerminalPane(selection: selection)),
      ],
    );
  }
}

