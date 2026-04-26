// Direct-deeplink surface router for visual review.
//
// Set `ANOTHER_ONE_SURFACE=...` at build time
// (`flutter run --dart-define=ANOTHER_ONE_SURFACE=desktop-titlebar`)
// to bypass the regular app routing and boot straight to a specific
// widget. Useful for screenshot-based parity comparisons against
// the GPUI build — open the live GPUI app and the Flutter build
// of the same surface side-by-side, eyeball, iterate.
//
// Surface registry below; unknown / unset values fall through to
// the regular `AppRoot`. Widget-only surfaces wrap the target in a
// minimal Scaffold with the dark-chrome bg so the screenshot
// doesn't carry stray Material defaults. Modal-based surfaces
// boot the desktop shell and fire `showXModal` after the first
// frame so the modal opens on top of the regular chrome.
//
// Add a new surface by adding an arm to `surfaceFor` — keep
// surface names kebab-case so they're shell-safe.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'screens/desktop_project_page/desktop_project_page.dart';
import 'screens/desktop_right_sidebar/desktop_right_sidebar.dart';
import 'screens/desktop_shell.dart';
import 'screens/desktop_sidebar/desktop_sidebar.dart';
import 'screens/desktop_titlebar/desktop_titlebar.dart';
import 'screens/new_task/new_task_modal.dart';
import 'screens/pair_mobile/pair_mobile_modal.dart';
import 'state/active_project_page_provider.dart';
import 'state/local_connection_provider.dart';
import 'state/settings_provider.dart';
import 'tokens.dart';

/// Build-time env flag — `--dart-define=ANOTHER_ONE_SURFACE=foo`.
/// Empty string when unset (the default-and-only state for normal
/// runs).
const String kSurface =
    String.fromEnvironment('ANOTHER_ONE_SURFACE', defaultValue: '');

/// Returns a surface widget when [name] matches a registered
/// preview, or `null` when the caller should fall back to the
/// regular [`AppRoot`]. Each preview is self-contained — it owns
/// its own Scaffold + dark bg so callers can wrap directly in a
/// `MaterialApp` without further chrome.
Widget? surfaceFor(String name) {
  return switch (name) {
    '' => null,
    'desktop-shell' => const DesktopShell(),
    'desktop-titlebar' =>
      const _SurfacePreview(child: DesktopTitlebar()),
    'desktop-sidebar' => const _SurfacePreview(child: DesktopSidebar()),
    'desktop-right-sidebar' =>
      const _SurfacePreview(child: DesktopRightSidebar()),
    'pair-mobile' => const _ModalLauncher(_ModalKind.pairMobile),
    'new-task-first-project' =>
      const _ModalLauncher(_ModalKind.newTaskFirstProject),
    'project-page-first' => const _ProjectPageLauncher(),
    'settings' => const _SettingsLauncher(),
    'settings-agents' =>
      const _SettingsLauncher(section: SettingsSection.agents),
    'settings-open-in' =>
      const _SettingsLauncher(section: SettingsSection.openIn),
    'settings-git-actions' =>
      const _SettingsLauncher(section: SettingsSection.gitActions),
    'settings-keybindings' =>
      const _SettingsLauncher(section: SettingsSection.keybindings),
    'settings-mcp' =>
      const _SettingsLauncher(section: SettingsSection.mcp),
    _ => _UnknownSurface(name: name),
  };
}

/// Wraps a widget-only preview in the dark Scaffold the shell uses.
/// The child sits at the top of the body and stretches to the
/// window's full width — that way titlebars (no intrinsic width)
/// render at their real size, and sidebars (explicit width) keep
/// their natural 280/320 px without forcing the window narrow.
///
/// Eagerly reads the `localConnectionProvider` so the embedded
/// daemon spins up + the project-list stream starts emitting —
/// otherwise the sidebar / right-sidebar previews would render
/// empty.
class _SurfacePreview extends ConsumerWidget {
  const _SurfacePreview({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.watch(localConnectionProvider);
    return Scaffold(
      backgroundColor: AppTokens.terminalBg,
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [child],
      ),
    );
  }
}

enum _ModalKind { pairMobile, newTaskFirstProject }

/// Boots the desktop shell and fires the matching modal once the
/// first frame settles. Used for surfaces that only exist as a
/// modal on top of the regular chrome (pair-mobile, new-task).
class _ModalLauncher extends ConsumerStatefulWidget {
  const _ModalLauncher(this.kind);

  final _ModalKind kind;

  @override
  ConsumerState<_ModalLauncher> createState() => _ModalLauncherState();
}

class _ModalLauncherState extends ConsumerState<_ModalLauncher> {
  bool _opened = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      if (_opened || !mounted) return;
      _opened = true;
      switch (widget.kind) {
        case _ModalKind.pairMobile:
          await showPairMobileModal(context);
        case _ModalKind.newTaskFirstProject:
          // Wait one tick for the local connection to stream a
          // project list, then pick the first one. The shell is
          // already rendering behind us so the modal opens on top.
          final projects =
              ref.read(desktopProjectsProvider).valueOrNull ?? const [];
          if (projects.isEmpty || !mounted) return;
          await showNewTaskModal(context, project: projects.first);
      }
    });
  }

  @override
  Widget build(BuildContext context) => const DesktopShell();
}

/// Renders the project page in isolation against the first
/// project the local daemon publishes — no sidebar, no titlebar,
/// just the page itself for parity-screenshot work. Watches the
/// project stream so it picks up the first arrival; the empty
/// state shows until then.
class _ProjectPageLauncher extends ConsumerWidget {
  const _ProjectPageLauncher();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.watch(localConnectionProvider);
    final projects =
        ref.watch(desktopProjectsProvider).valueOrNull ?? const [];
    if (projects.isEmpty) {
      return const Scaffold(
        backgroundColor: Color(0xFF1E1F22),
        body: Center(
          child: Text(
            'Waiting for the local daemon to publish a project…',
            style: TextStyle(color: AppTokens.textMuted),
          ),
        ),
      );
    }
    // Set the active project page so any sidebar/titlebar surfaces
    // rendered alongside this preview reflect the same state.
    final first = projects.first;
    Future.microtask(() => ref
        .read(activeProjectPageProvider.notifier)
        .state = first.id);
    return Scaffold(
      backgroundColor: const Color(0xFF1E1F22),
      body: DesktopProjectPage(project: first),
    );
  }
}

/// Boots the desktop shell and forces `settingsOpenProvider=true`
/// so the settings page renders in place of the main pane. The
/// optional [section] picks the active sub-page (Agents by
/// default).
class _SettingsLauncher extends ConsumerStatefulWidget {
  const _SettingsLauncher({this.section});

  final SettingsSection? section;

  @override
  ConsumerState<_SettingsLauncher> createState() =>
      _SettingsLauncherState();
}

class _SettingsLauncherState extends ConsumerState<_SettingsLauncher> {
  bool _opened = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_opened || !mounted) return;
      _opened = true;
      ref.read(settingsOpenProvider.notifier).state = true;
      if (widget.section != null) {
        ref.read(settingsSectionProvider.notifier).state =
            widget.section!;
      }
    });
  }

  @override
  Widget build(BuildContext context) => const DesktopShell();
}

/// Surfaced when `ANOTHER_ONE_SURFACE=<unknown>` — clear visual
/// signal so a typo doesn't silently pass through to the default
/// shell and confuse a screenshot review.
class _UnknownSurface extends StatelessWidget {
  const _UnknownSurface({required this.name});

  final String name;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: AppTokens.terminalBg,
      body: Center(
        child: Text(
          'Unknown ANOTHER_ONE_SURFACE: "$name"',
          style: const TextStyle(
            color: AppTokens.textPrimary,
            fontSize: AppTokens.fontBodyLg,
          ),
        ),
      ),
    );
  }
}
