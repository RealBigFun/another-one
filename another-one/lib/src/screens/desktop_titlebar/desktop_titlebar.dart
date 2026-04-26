// Top chrome titlebar for the desktop shell — visual-parity port of
// `desktop/src/titlebar.rs`.
//
// Layout (left → right):
//   - sidebar toggle (hides/shows the left sidebar)
//   - draggable empty region
//   - right cluster: build chip, Open In split-button, GitHub
//     project link, pair-mobile (QR), resource indicator
//     (CPU% / mem MB), right-sidebar toggle
//
// The Open-In split-button + dropdown lives in `open_in_button.dart`
// (a `part of` this file) so the chrome shell stays focused on
// layout. Other not-yet-ported chrome (Push, pull-request, git
// actions, custom actions) lands here when the bridge surface for
// each grows.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_svg/flutter_svg.dart';

import 'package:url_launcher/url_launcher.dart';

import '../../rust/api/local_session.dart'
    show
        OpenInAppDto,
        OpenInState,
        ProjectActionDto,
        ProjectActionIconDto,
        ProjectActionKindDto,
        ProjectActionKindDto_Agent,
        ProjectActionKindDto_Shell,
        ProjectActionScopeDto,
        PullRequestStateDto;
import '../../state/active_git_action_provider.dart';
import '../../state/active_git_state_provider.dart';
import '../../state/active_project_provider.dart';
import '../../state/build_info_provider.dart';
import '../../state/changed_files_provider.dart';
import '../../state/github_url_provider.dart';
import '../../state/left_sidebar_provider.dart';
import '../../state/local_connection_provider.dart';
import '../../state/open_in_provider.dart';
import '../../state/project_actions_provider.dart';
import '../../state/pull_request_status_provider.dart';
import '../../state/repo_default_commit_action_provider.dart';
import '../../state/resource_sample_provider.dart';
import '../../state/right_sidebar_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';
import '../../widgets/hover_icon_button.dart';
import '../../widgets/toolbar_spinner.dart';
import '../create_branch/create_branch_modal.dart';
import '../custom_action_modal/custom_action_modal.dart';
import '../pair_mobile/pair_mobile_modal.dart';

part 'chrome_button.dart';
part 'custom_actions_button.dart';
part 'git_actions_button.dart';
part 'open_in_button.dart';

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
          _TitlebarChromeButton(
            assetPath: 'assets/icons/icons__sidebar-toggle.svg',
            tooltip: 'Show or hide the projects sidebar',
            onPressed: () =>
                ref.read(leftSidebarOpenProvider.notifier).toggle(),
          ),
          // Draggable region — Flutter doesn't expose a native
          // window-drag handle on Linux without `bitsdojo_window`,
          // which lands in Phase 4. Empty Spacer keeps the layout
          // stable until then.
          const Spacer(),
          const _BuildChip(),
          const _CustomActionsButton(),
          const _OpenInButton(),
          const _ActiveProjectGithubButton(),
          const _PullRequestButton(),
          const _GitActionsButton(),
          const _PairMobileButton(),
          const SizedBox(width: AppTokens.space2),
          const _ResourceIndicator(),
          const SizedBox(width: AppTokens.space2),
          _TitlebarChromeButton(
            assetPath: 'assets/icons/icons__right-sidebar-toggle.svg',
            tooltip: 'Show or hide the changed files sidebar',
            onPressed: () =>
                ref.read(rightSidebarOpenProvider.notifier).toggle(),
          ),
        ],
      ),
    );
  }
}

/// Resource-usage indicator — full bordered button mirroring
/// GPUI's `desktop/src/resource_indicator.rs::resource_indicator_button`:
///
///   * 176w × 28h, rounded(11), border white@0.08, idle bg white@0.05,
///     hover white@0.08.
///   * Periwinkle `icons__resource-usage.svg` icon (11px) on the left.
///   * Right cluster: 46w cpu_label, `|` separator (white@0.36), 74w
///     mem_label. Both labels right-aligned, 12px / w500 / white@0.78.
///
/// Em-dashes show until the second sample arrives (CPU% needs a
/// delta). Click target reserved for the future popover toggle —
/// no-op today; the provider polls every 1.5s on its own.
class _ResourceIndicator extends ConsumerStatefulWidget {
  const _ResourceIndicator();

  @override
  ConsumerState<_ResourceIndicator> createState() =>
      _ResourceIndicatorState();
}

class _ResourceIndicatorState extends ConsumerState<_ResourceIndicator> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final usage = ref.watch(resourceUsageProvider);
    final cpuLabel = usage?.cpuPercent != null
        ? '${usage!.cpuPercent!.toStringAsFixed(1)}%'
        : '— %';
    final memLabel = usage != null && usage.memoryMib > 0
        ? '${usage.memoryMib.toStringAsFixed(1)} MB'
        : '— MB';
    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: Tooltip(
          message: 'Show resource usage',
          child: Container(
            width: 176,
            height: 28,
            padding: const EdgeInsets.symmetric(horizontal: 8),
            decoration: BoxDecoration(
              color: _hover
                  ? AppTokens.overlayHoverStrong
                  : AppTokens.overlayRest,
              borderRadius: BorderRadius.circular(11),
              border: Border.all(color: AppTokens.border),
            ),
            child: Row(
              children: [
                SvgPicture.asset(
                  'assets/icons/icons__resource-usage.svg',
                  width: 11,
                  height: 11,
                  colorFilter: const ColorFilter.mode(
                    AppTokens.toggleIconColor,
                    BlendMode.srcIn,
                  ),
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: [
                      SizedBox(
                        width: 46,
                        child: Text(
                          cpuLabel,
                          textAlign: TextAlign.right,
                          style: const TextStyle(
                            fontSize: 12,
                            fontWeight: FontWeight.w500,
                            color: Color(0xC7FFFFFF),
                          ),
                        ),
                      ),
                      const Padding(
                        padding: EdgeInsets.symmetric(horizontal: 4),
                        child: Text(
                          '|',
                          style: TextStyle(
                            fontSize: 12,
                            fontWeight: FontWeight.w500,
                            color: Color(0x5CFFFFFF),
                          ),
                        ),
                      ),
                      SizedBox(
                        width: 74,
                        child: Text(
                          memLabel,
                          textAlign: TextAlign.right,
                          style: const TextStyle(
                            fontSize: 12,
                            fontWeight: FontWeight.w500,
                            color: Color(0xC7FFFFFF),
                          ),
                        ),
                      ),
                    ],
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

/// State-tinted pill linking to the active project's PR. Mirrors
/// `desktop/src/titlebar.rs::titlebar_pull_request_button`. Hidden
/// when no PR exists for the current branch; otherwise the bg /
/// border / glyph all share the same state hue (open green,
/// closed grey, merged purple) at three opacity rungs (.13 bg,
/// .46 border, .20 hover bg).
class _PullRequestButton extends ConsumerStatefulWidget {
  const _PullRequestButton();

  @override
  ConsumerState<_PullRequestButton> createState() =>
      _PullRequestButtonState();
}

class _PullRequestButtonState extends ConsumerState<_PullRequestButton> {
  // HSL → sRGB constants from desktop/src/titlebar.rs.
  // hsla(160/360, 0.84, 0.35) ≈ #0FA170
  static const Color _openColor = Color(0xFF0FA170);
  // hsla(240/360, 0.04, 0.46) ≈ #71727A
  static const Color _closedColor = Color(0xFF71727A);
  // hsla(262/360, 0.83, 0.58) ≈ #8C44E0
  static const Color _mergedColor = Color(0xFF8C44E0);

  bool _hover = false;

  Color _stateColor(PullRequestStateDto state) => switch (state) {
        PullRequestStateDto.open => _openColor,
        PullRequestStateDto.closed => _closedColor,
        PullRequestStateDto.merged => _mergedColor,
      };

  String _tooltip(PullRequestStateDto state) => switch (state) {
        PullRequestStateDto.open => 'Open pull request in GitHub',
        PullRequestStateDto.closed => 'Open closed pull request in GitHub',
        PullRequestStateDto.merged => 'Open merged pull request in GitHub',
      };

  @override
  Widget build(BuildContext context) {
    final projectId = ref.watch(activeProjectIdProvider);
    if (projectId == null) return const SizedBox.shrink();
    final pr = ref.watch(pullRequestStatusProvider(projectId)).valueOrNull;
    if (pr == null) return const SizedBox.shrink();
    final color = _stateColor(pr.state);
    final bg = _hover
        ? color.withValues(alpha: 0.20)
        : color.withValues(alpha: 0.13);
    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: Tooltip(
        message: _tooltip(pr.state),
        child: MouseRegion(
          cursor: SystemMouseCursors.click,
          onEnter: (_) => setState(() => _hover = true),
          onExit: (_) => setState(() => _hover = false),
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: () async {
              final uri = Uri.tryParse(pr.url);
              if (uri == null) return;
              await launchUrl(uri, mode: LaunchMode.externalApplication);
            },
            child: Container(
              width: 32,
              height: 28,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: bg,
                borderRadius: BorderRadius.circular(11),
                border: Border.all(color: color.withValues(alpha: 0.46)),
              ),
              child: AppIcon(
                'pull-request',
                size: 13,
                color: color,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
