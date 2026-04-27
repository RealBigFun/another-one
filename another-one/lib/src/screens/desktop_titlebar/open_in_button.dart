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
class _OpenInButton extends ConsumerStatefulWidget {
  const _OpenInButton();

  @override
  ConsumerState<_OpenInButton> createState() => _OpenInButtonState();
}

class _OpenInButtonState extends ConsumerState<_OpenInButton> {
  // Width / height come from desktop/src/titlebar.rs constants.
  static const double _buttonW = 114;
  static const double _buttonH = 28;
  static const double _chevronW = 26;
  static const double _menuRowH = 38;

  final OverlayPortalController _menu = OverlayPortalController();
  final LayerLink _link = LayerLink();
  bool _bodyHover = false;
  bool _chevronHover = false;

  void _syncMenuVisibility(bool visible) {
    if (visible == _menu.isShowing) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || visible == _menu.isShowing) return;
      setState(visible ? _menu.show : _menu.hide);
    });
  }

  @override
  Widget build(BuildContext context) {
    final projectId = ref.watch(activeProjectIdProvider);
    final state = ref.watch(openInStateProvider).valueOrNull;
    if (projectId == null || state == null) {
      return const SizedBox.shrink();
    }

    final preferredIconPath = _resolvePreferredIcon(state);
    final menuOpen =
        ref.watch(_activeTitlebarDropdownProvider) == _TitlebarDropdown.openIn;
    _syncMenuVisibility(menuOpen);
    final containerBg = menuOpen
        ? AppTokens.overlayActive
        : AppTokens.overlayRest;

    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: CompositedTransformTarget(
        link: _link,
        child: OverlayPortal(
          controller: _menu,
          overlayChildBuilder: (context) =>
              _buildMenu(context, projectId, state),
          child: Container(
            width: _buttonW,
            height: _buttonH,
            decoration: BoxDecoration(
              color: containerBg,
              borderRadius: BorderRadius.circular(11),
              border: Border.all(color: AppTokens.border),
            ),
            child: Row(
              children: [
                _buildPrimaryHalf(projectId, preferredIconPath, state),
                _buildChevronHalf(projectId, state),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildPrimaryHalf(
    String projectId,
    String preferredIconPath,
    OpenInState state,
  ) {
    return Expanded(
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _bodyHover = true),
        onExit: (_) => setState(() => _bodyHover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () => _openPreferred(projectId, state),
          child: Container(
            decoration: BoxDecoration(
              color: _bodyHover
                  ? AppTokens.overlayHoverStrong
                  : Colors.transparent,
              border: const Border(right: BorderSide(color: AppTokens.divider)),
            ),
            padding: const EdgeInsets.symmetric(horizontal: 9),
            alignment: Alignment.centerLeft,
            child: Row(
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
          ),
        ),
      ),
    );
  }

  Widget _buildChevronHalf(String projectId, OpenInState state) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _chevronHover = true),
      onExit: (_) => setState(() => _chevronHover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: state.enabledApps.isEmpty
            ? _openSettings
            : () => _toggleTitlebarDropdown(ref, _TitlebarDropdown.openIn),
        child: Container(
          width: _chevronW,
          height: _buttonH,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: _chevronHover
                ? AppTokens.overlayHoverStrong
                : Colors.transparent,
            borderRadius: const BorderRadius.only(
              topRight: Radius.circular(11),
              bottomRight: Radius.circular(11),
            ),
          ),
          child: SvgPicture.asset(
            'assets/icons/icons__chevron-down.svg',
            width: 11,
            height: 11,
            colorFilter: const ColorFilter.mode(
              AppTokens.textMuted,
              BlendMode.srcIn,
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildMenu(BuildContext context, String projectId, OpenInState state) {
    return Stack(
      children: [
        // Outside-tap dismisser. Sits behind the menu so taps on the
        // menu itself are absorbed by its own GestureDetector.
        Positioned.fill(
          child: GestureDetector(
            behavior: HitTestBehavior.translucent,
            onTap: () => _dismissTitlebarDropdowns(ref),
          ),
        ),
        CompositedTransformFollower(
          link: _link,
          targetAnchor: Alignment.bottomRight,
          followerAnchor: Alignment.topRight,
          offset: const Offset(0, 6),
          child: Material(
            color: Colors.transparent,
            child: Container(
              width: _buttonW,
              decoration: BoxDecoration(
                color: AppTokens.cardBg,
                borderRadius: BorderRadius.circular(12),
                border: Border.all(color: AppTokens.border),
                boxShadow: const [
                  BoxShadow(
                    color: Color(0x66000000),
                    blurRadius: 12,
                    offset: Offset(0, 4),
                  ),
                ],
              ),
              clipBehavior: Clip.antiAlias,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  for (final app in state.enabledApps)
                    _MenuRow(
                      app: app,
                      height: _menuRowH,
                      onTap: () => _openIn(projectId, app),
                    ),
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }

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

  Future<void> _openPreferred(String projectId, OpenInState state) async {
    final preferredId = state.preferredAppId;
    if (preferredId == null) {
      _openSettings();
      return;
    }
    await _openIn(projectId, _appById(state, preferredId));
  }

  void _openSettings() {
    _dismissTitlebarDropdowns(ref);
    ref.read(settingsSectionProvider.notifier).state = SettingsSection.openIn;
    ref.read(settingsOpenProvider.notifier).state = true;
  }

  OpenInAppDto _appById(OpenInState state, String id) {
    return state.enabledApps.firstWhere((app) => app.id == id);
  }

  Future<void> _openIn(String projectId, OpenInAppDto app) async {
    _dismissTitlebarDropdowns(ref);
    final connection = ref.read(localConnectionProvider);
    try {
      await connection.openProjectInApp(projectId: projectId, appId: app.id);
    } catch (e) {
      if (!mounted) return;
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
