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

import 'package:url_launcher/url_launcher.dart';

import '../../state/active_project_provider.dart';
import '../../state/build_info_provider.dart';
import '../../state/github_url_provider.dart';
import '../../state/left_sidebar_provider.dart';
import '../../state/resource_sample_provider.dart';
import '../../state/right_sidebar_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';
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
          _TitlebarIconButton(
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
          const _ActiveProjectGithubButton(),
          const _PairMobileButton(),
          const SizedBox(width: AppTokens.space2),
          const _ResourceIndicator(),
          const SizedBox(width: AppTokens.space2),
          _TitlebarIconButton(
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
class _ActiveProjectGithubButton extends ConsumerStatefulWidget {
  const _ActiveProjectGithubButton();

  @override
  ConsumerState<_ActiveProjectGithubButton> createState() =>
      _ActiveProjectGithubButtonState();
}

class _ActiveProjectGithubButtonState
    extends ConsumerState<_ActiveProjectGithubButton> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final projectId = ref.watch(activeProjectIdProvider);
    if (projectId == null) return const SizedBox.shrink();
    final url =
        ref.watch(projectGithubUrlProvider(projectId)).valueOrNull;
    if (url == null || url.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: Tooltip(
        message: 'View this project on GitHub',
        child: MouseRegion(
          cursor: SystemMouseCursors.click,
          onEnter: (_) => setState(() => _hovered = true),
          onExit: (_) => setState(() => _hovered = false),
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: () async {
              final uri = Uri.tryParse(url);
              if (uri == null) return;
              await launchUrl(uri, mode: LaunchMode.externalApplication);
            },
            child: Container(
              width: 28,
              height: 28,
              decoration: BoxDecoration(
                color: _hovered
                    ? AppTokens.overlayHoverStrong
                    : AppTokens.overlayRest,
                borderRadius: BorderRadius.circular(AppTokens.radiusMd),
                border: Border.all(color: AppTokens.border),
              ),
              alignment: Alignment.center,
              child: const AppIcon(
                'github',
                size: 14,
                color: AppTokens.textSecondary,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _PairMobileButton extends StatelessWidget {
  const _PairMobileButton();

  @override
  Widget build(BuildContext context) {
    return _TitlebarIconButton(
      icon: 'qr-code',
      tooltip: 'Pair a mobile device with the embedded daemon',
      onPressed: () => showPairMobileModal(context),
    );
  }
}

class _TitlebarIconButton extends StatefulWidget {
  const _TitlebarIconButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
  });

  final String icon;
  final String tooltip;
  final VoidCallback onPressed;

  @override
  State<_TitlebarIconButton> createState() => _TitlebarIconButtonState();
}

class _TitlebarIconButtonState extends State<_TitlebarIconButton> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: Container(
            width: 28,
            height: 28,
            decoration: BoxDecoration(
              color: _hovered
                  ? AppTokens.overlayHoverStrong
                  : AppTokens.overlayRest,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              border: Border.all(color: AppTokens.border),
            ),
            alignment: Alignment.center,
            child: AppIcon(
              widget.icon,
              size: 15,
              color: AppTokens.textPrimary,
            ),
          ),
        ),
      ),
    );
  }
}
