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

import '../state/active_project_page_provider.dart';
import '../state/left_sidebar_provider.dart';
import '../state/local_connection_provider.dart';
import '../state/right_sidebar_provider.dart';
import '../state/resolved_tab_selection_provider.dart';
import '../state/settings_provider.dart';
import '../state/tab_selection_provider.dart';
import '../tokens.dart';
import '../widgets/empty_state.dart';
import 'desktop_project_page/desktop_project_page.dart';
import 'desktop_right_sidebar/desktop_right_sidebar.dart';
import 'desktop_sidebar/desktop_sidebar.dart';
import 'desktop_terminal/desktop_tab_strip.dart';
import 'desktop_terminal/desktop_terminal_pane.dart';
import 'desktop_titlebar/desktop_titlebar.dart';
import 'settings_page/settings_page.dart';

class DesktopShell extends ConsumerWidget {
  const DesktopShell({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // Eagerly read the local connection so the daemon-backed
    // transport spins up before anything tries to render projects.
    ref.watch(localConnectionProvider);
    final leftOpen = ref.watch(leftSidebarOpenProvider);
    final rightOpen = ref.watch(rightSidebarOpenProvider);
    final settingsOpen = ref.watch(settingsOpenProvider);
    // GPUI's `app.rs` returns ONLY the settings page (plus the toast
    // layer) when `settings_open` is true — no left sidebar, no right
    // sidebar, no main row. Settings is a full draw-area takeover, not
    // a swap-the-center-pane mode. We mirror that by collapsing the
    // chrome row to the SettingsPage when the provider flips on. The
    // titlebar stays so the window keeps its drag region and controls
    // (GPUI hides it because its custom titlebar is part of
    // `main_row`; ours is a sibling of the body, so the equivalent
    // here is just to hide both sidebars).
    return Scaffold(
      backgroundColor: AppTokens.terminalBg,
      body: Column(
        children: [
          const DesktopTitlebar(),
          Expanded(
            child: settingsOpen
                ? const SettingsPage()
                : Row(
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
    final projectsLoaded =
        ref.watch(desktopProjectsLoadedProvider).valueOrNull ?? false;
    final resolvedSelection = ref.watch(resolvedSelectedTabProvider);

    if (selection != null && projectsLoaded && resolvedSelection != selection) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        final latest = ref.read(selectedTabProvider);
        if (latest == null || latest == resolvedSelection) return;
        final notifier = ref.read(selectedTabProvider.notifier);
        if (resolvedSelection == null) {
          notifier.clear();
        } else {
          notifier.set(resolvedSelection);
        }
      });
    }

    if (selection != null && !projectsLoaded) {
      return const ColoredBox(
        color: AppTokens.terminalBg,
        child: Center(child: CircularProgressIndicator()),
      );
    }

    if (resolvedSelection != null) {
      return Column(
        children: [
          DesktopTabStrip(selection: resolvedSelection),
          Expanded(child: DesktopTerminalPane(selection: resolvedSelection)),
        ],
      );
    }
    final projectPageId = ref.watch(activeProjectPageProvider);
    if (projectPageId != null) {
      // Pull the project record off the live project list. If the
      // id is stale (project was just removed) the page falls back
      // to the empty placeholder rather than crashing on `firstWhere`.
      final projects =
          ref.watch(desktopProjectsProvider).valueOrNull ?? const [];
      for (final project in projects) {
        if (project.id == projectPageId) {
          return DesktopProjectPage(project: project);
        }
      }
    }
    return Container(
      color: AppTokens.terminalBg,
      alignment: Alignment.center,
      child: const EmptyState(
        text: 'No project selected',
        padding: EdgeInsets.all(AppTokens.space10),
      ),
    );
  }
}
