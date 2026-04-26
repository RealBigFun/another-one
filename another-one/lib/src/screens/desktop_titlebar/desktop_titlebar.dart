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
