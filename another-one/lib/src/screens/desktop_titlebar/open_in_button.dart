// Split out of `desktop_titlebar.dart` so the chrome shell stays
// focused on titlebar layout rather than the Open-In split-button's
// menu plumbing. `part of` keeps `_OpenInButton` and `_MenuRow`
// underscore-private without forcing them into a public export.

part of 'desktop_titlebar.dart';

/// Split-button "Open In" — primary half launches the active
/// project in the preferred app, chevron half toggles a dropdown of
/// every enabled app. Mirrors `desktop/src/titlebar.rs`'s
/// `titlebar_open_in_button` + `titlebar_open_in_overlay` (GPUI's
/// version paints them as two top-level chrome elements; here they
/// fuse into one widget via `OverlayPortal`).
///
/// Hidden only when there's no active project. With zero enabled
/// apps, the control stays visible and routes to Settings -> Open In
/// so the user still has an in-context recovery path.
class _OpenInButton extends ConsumerWidget {
  const _OpenInButton();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projectId = ref.watch(activeProjectIdProvider);
    final state = ref.watch(openInStateProvider).valueOrNull;
    if (projectId == null || state == null) {
      return const SizedBox.shrink();
    }

    final preferredIconPath = _resolvePreferredIcon(state);
    final menuOpen =
        ref.watch(_activeTitlebarDropdownProvider) == _TitlebarDropdown.openIn;
    return _TitlebarSplitButton(
      buttonWidth: _buttonW,
      menuWidth: _buttonW,
      menuOpen: menuOpen,
      onDismissMenu: () => _dismissTitlebarDropdowns(ref),
      onPrimaryTap: () {
        unawaited(_openPreferred(context, ref, projectId, state));
      },
      onChevronTap: state.enabledApps.isEmpty
          ? () => _openSettings(ref)
          : () => _toggleTitlebarDropdown(ref, _TitlebarDropdown.openIn),
      primaryBuilder: (context) => Row(
        children: [
          SvgPicture.asset(
            preferredIconPath,
            width: 14,
            height: 14,
            colorFilter: const ColorFilter.mode(
              AppTokens.textPrimary,
              BlendMode.srcIn,
            ),
          ),
          const SizedBox(width: 6),
          // Flexible+clip mirrors GPUI's default flex-shrink on
          // a text child — the 114px button width is exact, but
          // Lilex Mono "Open In" at 12px Medium runs ~5px past
          // the inner 67px content width. GPUI clips silently,
          // so we do the same here rather than widen the button.
          const Flexible(
            child: Text(
              'Open In',
              softWrap: false,
              overflow: TextOverflow.clip,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w500,
                color: AppTokens.textSecondary,
              ),
            ),
          ),
        ],
      ),
      chevronBuilder: (context) => SvgPicture.asset(
        'assets/icons/icons__chevron-down.svg',
        width: 11,
        height: 11,
        colorFilter: const ColorFilter.mode(
          AppTokens.textMuted,
          BlendMode.srcIn,
        ),
      ),
      menuBuilder: (context) => Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final app in state.enabledApps)
            _MenuRow(
              app: app,
              height: _menuRowH,
              onTap: () {
                unawaited(_openIn(context, ref, projectId, app));
              },
            ),
        ],
      ),
    );
  }

  // Width comes from desktop/src/titlebar.rs constants.
  static const double _buttonW = 114;
  static const double _menuRowH = 38;

  /// Folder fallback when no preferred app is set yet — matches
  /// GPUI's `preferred_open_in_app().map(icon_path).unwrap_or(folder)`.
  String _resolvePreferredIcon(OpenInState state) {
    final preferredId = state.preferredAppId;
    if (preferredId != null) {
      for (final app in state.enabledApps) {
        if (app.id == preferredId) return app.iconPath;
      }
    }
    return 'assets/icons/open_in__folder_closed.svg';
  }

  Future<void> _openPreferred(
    BuildContext context,
    WidgetRef ref,
    String projectId,
    OpenInState state,
  ) async {
    final preferredId = state.preferredAppId;
    if (preferredId == null) {
      _openSettings(ref);
      return;
    }
    await _openIn(context, ref, projectId, _appById(state, preferredId));
  }

  void _openSettings(WidgetRef ref) {
    _dismissTitlebarDropdowns(ref);
    ref.read(settingsSectionProvider.notifier).state = SettingsSection.openIn;
    ref.read(settingsOpenProvider.notifier).state = true;
  }

  OpenInAppDto _appById(OpenInState state, String id) {
    return state.enabledApps.firstWhere((app) => app.id == id);
  }

  Future<void> _openIn(
    BuildContext context,
    WidgetRef ref,
    String projectId,
    OpenInAppDto app,
  ) async {
    _dismissTitlebarDropdowns(ref);
    final connection = ref.read(localConnectionProvider);
    try {
      await connection.openProjectInApp(projectId: projectId, appId: app.id);
    } catch (e) {
      if (!context.mounted) return;
      showAppToast(context, message: 'Could not open in ${app.label}: $e');
      return;
    }
    // Refetch so the primary-icon flips to the just-picked app.
    ref.invalidate(openInStateProvider);
  }
}

class _MenuRow extends StatefulWidget {
  const _MenuRow({
    required this.app,
    required this.height,
    required this.onTap,
  });

  final OpenInAppDto app;
  final double height;
  final VoidCallback onTap;

  @override
  State<_MenuRow> createState() => _MenuRowState();
}

class _MenuRowState extends State<_MenuRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.app.description,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onTap,
          child: Container(
            height: widget.height,
            color: _hover ? AppTokens.overlayHover : Colors.transparent,
            padding: const EdgeInsets.symmetric(horizontal: 12),
            child: Row(
              children: [
                SvgPicture.asset(
                  widget.app.iconPath,
                  width: 16,
                  height: 16,
                  colorFilter: const ColorFilter.mode(
                    AppTokens.textPrimary,
                    BlendMode.srcIn,
                  ),
                ),
                const SizedBox(width: 10),
                Text(
                  widget.app.label,
                  style: const TextStyle(
                    fontSize: 13,
                    fontWeight: FontWeight.w500,
                    color: AppTokens.textPrimary,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
