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

import '../state/local_connection_provider.dart';
import '../state/tab_selection_provider.dart';
import '../tokens.dart';
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
    return const Scaffold(
      backgroundColor: AppTokens.terminalBg,
      body: Column(
        children: [
          DesktopTitlebar(),
          Expanded(
            child: Row(
              children: [
                DesktopSidebar(),
                Expanded(child: _MainArea()),
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
        child: const _WelcomePlaceholder(),
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

/// Shown until a tab is selected — picks up the GPUI desktop's
/// "no project active" empty state (project_page.rs renders a
/// similarly-centred placeholder when its lookup fails). Mirrors the
/// "Project not found" copy + dark background.
class _WelcomePlaceholder extends StatelessWidget {
  const _WelcomePlaceholder();

  @override
  Widget build(BuildContext context) {
    return const Padding(
      padding: EdgeInsets.all(AppTokens.space10),
      child: Text(
        'No project selected',
        style: TextStyle(
          fontSize: AppTokens.fontBodyLg,
          color: AppTokens.textMuted,
        ),
      ),
    );
  }
}
