// Top chrome titlebar for the desktop shell — visual-parity port of
// `desktop/src/titlebar.rs`.
//
// Layout (left → right):
//   - sidebar toggle (hides/shows the left sidebar — toggle wire-up
//     comes when the shell supports collapsible chrome)
//   - draggable empty region
//   - right cluster: pair-mobile (QR), resource indicator stub
//     (CPU% / mem MB), right-sidebar toggle
//
// Buttons that need verbs not yet on the bridge (Open In, Push,
// custom actions, build chip) are intentionally absent — they
// surface as placeholder buttons in subsequent commits when the
// underlying verbs land.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_svg/flutter_svg.dart';

import 'package:url_launcher/url_launcher.dart';

import '../../rust/api/local_session.dart' show OpenInAppDto, OpenInState;
import '../../state/active_project_provider.dart';
import '../../state/build_info_provider.dart';
import '../../state/github_url_provider.dart';
import '../../state/left_sidebar_provider.dart';
import '../../state/local_connection_provider.dart';
import '../../state/open_in_provider.dart';
import '../../state/resource_sample_provider.dart';
import '../../state/right_sidebar_provider.dart';
import '../../tokens.dart';
import '../../widgets/hover_icon_button.dart';
import '../pair_mobile/pair_mobile_modal.dart';

class DesktopTitlebar extends ConsumerWidget {
  const DesktopTitlebar({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Container(
      height: AppTokens.titlebarHeight,
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(
          bottom: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.space2),
      child: Row(
        children: [
          HoverIconButton(
            size: 28,
            iconSize: 15,
            iconColor: AppTokens.textPrimary,
            restBg: AppTokens.overlayRest,
            hoverBg: AppTokens.overlayHoverStrong,
            showBorder: true,
            icon: 'layout-split',
            tooltip: 'Toggle sidebar',
            onPressed: () =>
                ref.read(leftSidebarOpenProvider.notifier).toggle(),
          ),
          const SizedBox(width: AppTokens.space2),
          // Draggable region — Flutter doesn't expose a native
          // window-drag handle on Linux without `bitsdojo_window`,
          // which lands in Phase 4. Empty Spacer keeps the layout
          // stable until then.
          const Spacer(),
          const _BuildChip(),
          const _OpenInButton(),
          const _ActiveProjectGithubButton(),
          const _PairMobileButton(),
          const SizedBox(width: AppTokens.space2),
          const _ResourceIndicator(),
          const SizedBox(width: AppTokens.space2),
          HoverIconButton(
            size: 28,
            iconSize: 15,
            iconColor: AppTokens.textPrimary,
            restBg: AppTokens.overlayRest,
            hoverBg: AppTokens.overlayHoverStrong,
            showBorder: true,
            icon: 'layout-split',
            tooltip: 'Toggle right sidebar',
            onPressed: () =>
                ref.read(rightSidebarOpenProvider.notifier).toggle(),
          ),
        ],
      ),
    );
  }
}

/// Resource-usage indicator: CPU% + memory MB. Reads the
/// `resourceUsageProvider`, which polls
/// `core::platform::HeadlessPlatform::read_process_samples` every
/// 1.5s through the FRB bridge and derives CPU% from cumulative-
/// time deltas. Em-dashes show until the second sample arrives
/// (CPU% needs a delta).
class _ResourceIndicator extends ConsumerWidget {
  const _ResourceIndicator();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final usage = ref.watch(resourceUsageProvider);
    final cpuLabel = usage?.cpuPercent != null
        ? '${usage!.cpuPercent!.toStringAsFixed(1)}%'
        : '— %';
    final memLabel = usage != null && usage.memoryMib > 0
        ? '${usage.memoryMib.toStringAsFixed(1)} MB'
        : '— MB';
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.space2),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(
            Icons.public,
            size: 12,
            color: AppTokens.textPlaceholder,
          ),
          const SizedBox(width: AppTokens.space1),
          Text(
            '$cpuLabel  |  $memLabel',
            style: const TextStyle(
              fontFamily: AppTokens.fontFamilyMono,
              fontSize: AppTokens.fontCaption,
              color: AppTokens.textPlaceholder,
            ),
          ),
        ],
      ),
    );
  }
}

/// Build-identity chip — small pill between drag region and the
/// pair-mobile button. Mirrors the GPUI titlebar's
/// `titlebar_build_chip`: dev+dirty=red, dev+clean=amber,
/// release=subtle. Tooltip surfaces profile, branch, sha, dirty
/// flag, and build time.
class _BuildChip extends ConsumerWidget {
  const _BuildChip();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final info = ref.watch(buildInfoProvider).valueOrNull;
    if (info == null) {
      return const SizedBox.shrink();
    }
    final (Color bg, Color border, Color text) = switch ((info.isDev, info.isDirty)) {
      (true, true) => (
          const Color(0x8CB23232),
          const Color(0xD9F25656),
          const Color(0xFFF7F7F7),
        ),
      (true, false) => (
          const Color(0x73E68A1F),
          const Color(0xBFFFB347),
          const Color(0xFFFAF0E6),
        ),
      _ => (
          const Color(0x14FFFFFF),
          const Color(0x29FFFFFF),
          const Color(0x8CFFFFFF),
        ),
    };
    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: Tooltip(
        message: info.tooltip,
        child: Container(
          height: 20,
          padding: const EdgeInsets.symmetric(horizontal: 8),
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: bg,
            borderRadius: BorderRadius.circular(10),
            border: Border.all(color: border),
          ),
          child: Text(
            info.chipLabel,
            style: TextStyle(
              fontSize: 11,
              fontWeight: FontWeight.w500,
              color: text,
            ),
          ),
        ),
      ),
    );
  }
}

/// GitHub button for the active project. Hidden when no project is
/// active or the active project's `origin` remote isn't a github.com
/// URL — same gating GPUI applies (`titlebar.rs` only renders the
/// github button when `active_open_in_project_id().is_some()` and
/// `project_github_links.contains(project_id)`).
class _ActiveProjectGithubButton extends ConsumerWidget {
  const _ActiveProjectGithubButton();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projectId = ref.watch(activeProjectIdProvider);
    if (projectId == null) return const SizedBox.shrink();
    final url =
        ref.watch(projectGithubUrlProvider(projectId)).valueOrNull;
    if (url == null || url.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: HoverIconButton(
        size: 28,
        iconSize: 14,
        iconColor: AppTokens.textSecondary,
        restBg: AppTokens.overlayRest,
        hoverBg: AppTokens.overlayHoverStrong,
        showBorder: true,
        icon: 'github',
        tooltip: 'View this project on GitHub',
        onPressed: () async {
          final uri = Uri.tryParse(url);
          if (uri == null) return;
          await launchUrl(uri, mode: LaunchMode.externalApplication);
        },
      ),
    );
  }
}

class _PairMobileButton extends StatelessWidget {
  const _PairMobileButton();

  @override
  Widget build(BuildContext context) {
    return HoverIconButton(
      size: 28,
      iconSize: 15,
      iconColor: AppTokens.textPrimary,
      restBg: AppTokens.overlayRest,
      hoverBg: AppTokens.overlayHoverStrong,
      showBorder: true,
      icon: 'qr-code',
      tooltip: 'Pair a mobile device with the embedded daemon',
      onPressed: () => showPairMobileModal(context),
    );
  }
}

/// Split-button "Open In" — primary half launches the active
/// project in the preferred app, chevron half toggles a dropdown of
/// every enabled app. Mirrors `desktop/src/titlebar.rs`'s
/// `titlebar_open_in_button` + `titlebar_open_in_overlay` (GPUI's
/// version paints them as two top-level chrome elements; here they
/// fuse into one widget via `OverlayPortal`).
///
/// Hidden when there's no active project, or when no Open-In apps
/// are enabled — same gate GPUI applies.
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

  @override
  Widget build(BuildContext context) {
    final projectId = ref.watch(activeProjectIdProvider);
    final state = ref.watch(openInStateProvider).valueOrNull;
    if (projectId == null || state == null || state.enabledApps.isEmpty) {
      return const SizedBox.shrink();
    }

    final preferredIconPath = _resolvePreferredIcon(state);
    final menuOpen = _menu.isShowing;
    final containerBg = menuOpen ? AppTokens.overlayActive : AppTokens.overlayRest;

    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: CompositedTransformTarget(
        link: _link,
        child: OverlayPortal(
          controller: _menu,
          overlayChildBuilder: (context) => _buildMenu(context, projectId, state),
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
              color: _bodyHover ? AppTokens.overlayHoverStrong : Colors.transparent,
              border: const Border(
                right: BorderSide(color: AppTokens.divider),
              ),
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
                const Text(
                  'Open In',
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w500,
                    color: AppTokens.textSecondary,
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
        onTap: () => setState(_menu.toggle),
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

  Widget _buildMenu(
    BuildContext context,
    String projectId,
    OpenInState state,
  ) {
    return Stack(
      children: [
        // Outside-tap dismisser. Sits behind the menu so taps on the
        // menu itself are absorbed by its own GestureDetector.
        Positioned.fill(
          child: GestureDetector(
            behavior: HitTestBehavior.translucent,
            onTap: () => setState(_menu.hide),
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
      // GPUI navigates to the Open-In settings section here. The
      // settings page hasn't been ported yet (#3 in the plan), so
      // for now no-op — the chevron dropdown still gets the user
      // there in one click.
      return;
    }
    await _openIn(projectId, _appById(state, preferredId));
  }

  OpenInAppDto _appById(OpenInState state, String id) {
    return state.enabledApps.firstWhere((app) => app.id == id);
  }

  Future<void> _openIn(String projectId, OpenInAppDto app) async {
    setState(_menu.hide);
    final connection = ref.read(localConnectionProvider);
    try {
      await connection.openProjectInApp(projectId: projectId, appId: app.id);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Could not open in ${app.label}: $e'),
          backgroundColor: AppTokens.errorBg,
        ),
      );
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
