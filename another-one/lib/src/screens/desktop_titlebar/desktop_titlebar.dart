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
import '../../rust/api/resources.dart'
    show ResourceUsageProjectDto, ResourceUsageSessionDto, ResourceUsageTaskDto;
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
import '../../state/settings_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../window_chrome.dart';
import '../../widgets/app_icon.dart';
import '../../widgets/app_toast.dart';
import '../../widgets/hover_icon_button.dart';
import '../../widgets/toolbar_spinner.dart';
import '../create_branch/create_branch_modal.dart';
import '../custom_action_modal/custom_action_modal.dart';
import '../pair_mobile/pair_mobile_modal.dart';

part 'chrome_button.dart';
part 'custom_actions_button.dart';
part 'git_actions_button.dart';
part 'open_in_button.dart';

enum _TitlebarDropdown { customActions, openIn, gitActions }

final _activeTitlebarDropdownProvider = StateProvider<_TitlebarDropdown?>(
  (ref) => null,
);

void _dismissTitlebarDropdowns(WidgetRef ref) {
  ref.read(_activeTitlebarDropdownProvider.notifier).state = null;
}

void _toggleTitlebarDropdown(WidgetRef ref, _TitlebarDropdown dropdown) {
  final notifier = ref.read(_activeTitlebarDropdownProvider.notifier);
  notifier.state = notifier.state == dropdown ? null : dropdown;
}

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
            onPressed: () {
              _dismissTitlebarDropdowns(ref);
              ref.read(leftSidebarOpenProvider.notifier).toggle();
            },
          ),
          const _TitlebarDragRegion(),
          const _BuildChip(),
          const _CustomActionsButton(),
          const _OpenInButton(),
          const _ActiveProjectGithubButton(),
          const _PullRequestButton(),
          const _GitActionsButton(),
          const _PairMobileButton(),
          // The resource indicator already pads its own right
          // edge with 6px (matching GPUI's `mr(px(6))`); the
          // pair-mobile button does too. No extra spacers
          // needed before/after — explicit `SizedBox(space2)`
          // gaps were what tipped the row into 5.2px overflow.
          const _ResourceIndicator(),
          _TitlebarChromeButton(
            assetPath: 'assets/icons/icons__right-sidebar-toggle.svg',
            tooltip: 'Show or hide the changed files sidebar',
            onPressed: () {
              _dismissTitlebarDropdowns(ref);
              ref.read(rightSidebarOpenProvider.notifier).toggle();
            },
          ),
        ],
      ),
    );
  }
}

class _TitlebarDragRegion extends ConsumerWidget {
  const _TitlebarDragRegion();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Expanded(
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () => _dismissTitlebarDropdowns(ref),
        onDoubleTap: () {
          _dismissTitlebarDropdowns(ref);
          unawaited(toggleNativeWindowMaximize());
        },
        onPanStart: (details) {
          _dismissTitlebarDropdowns(ref);
          unawaited(startNativeWindowDrag(details.globalPosition));
        },
        child: const SizedBox.expand(),
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
/// Em-dashes show until the first shared resource snapshot arrives.
/// Clicking toggles the popover and bumps the sampling cadence from
/// the idle 5s loop to the open-state 1s loop.
class _ResourceIndicator extends ConsumerStatefulWidget {
  const _ResourceIndicator();

  @override
  ConsumerState<_ResourceIndicator> createState() => _ResourceIndicatorState();
}

/// Build-time flag — `--dart-define=ANOTHER_ONE_AUTO_OPEN=resource-popover`
/// auto-opens the resource indicator popover after the first frame.
/// Used by screenshot tooling to capture the popover without driving
/// the cursor through synthetic input.
const String _kAutoOpen = String.fromEnvironment(
  'ANOTHER_ONE_AUTO_OPEN',
  defaultValue: '',
);

class _ResourceIndicatorState extends ConsumerState<_ResourceIndicator> {
  bool _hover = false;
  final OverlayPortalController _popover = OverlayPortalController();
  final LayerLink _link = LayerLink();
  // Stable across repolls — collapsing a project/task in GPUI is
  // tracked the same way (`AnotherOneApp::resource_collapsed_nodes`).
  final Set<String> _collapsedNodes = <String>{};

  @override
  void initState() {
    super.initState();
    if (_kAutoOpen == 'resource-popover') {
      // 800ms gives the embedded daemon time to publish at least one
      // tracked-process sample so the popover renders the tree on
      // open instead of the empty-state pill.
      Future.delayed(const Duration(milliseconds: 800), () {
        if (mounted && !_popover.isShowing) {
          _setPopoverVisible(true);
        }
      });
    }
  }

  void _setPopoverVisible(bool visible) {
    setState(() {
      if (visible) {
        _popover.show();
      } else {
        _popover.hide();
      }
    });
    ref.read(resourceUsagePopoverOpenProvider.notifier).state = visible;
  }

  void _togglePopover() {
    _setPopoverVisible(!_popover.isShowing);
  }

  @override
  Widget build(BuildContext context) {
    final usage = ref.watch(resourceUsageProvider);
    final cpuLabel = usage?.cpuPercent != null
        ? '${usage!.cpuPercent!.toStringAsFixed(1)}%'
        : '— %';
    final memLabel = usage?.snapshot != null
        ? _formatMemory(usage!.snapshot!.appMemoryBytes)
        : usage != null && usage.memoryMib > 0
        ? '${usage.memoryMib.toStringAsFixed(1)} MB'
        : '— MB';
    final open = _popover.isShowing;
    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: CompositedTransformTarget(
        link: _link,
        child: OverlayPortal(
          controller: _popover,
          overlayChildBuilder: (context) =>
              _buildPopover(context, usage, cpuLabel, memLabel),
          child: MouseRegion(
            cursor: SystemMouseCursors.click,
            onEnter: (_) => setState(() => _hover = true),
            onExit: (_) => setState(() => _hover = false),
            child: Tooltip(
              message: 'Show resource usage',
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: _togglePopover,
                child: Container(
                  width: 176,
                  height: 28,
                  padding: const EdgeInsets.symmetric(horizontal: 8),
                  decoration: BoxDecoration(
                    color: open
                        ? AppTokens.overlayActive
                        : (_hover
                              ? AppTokens.overlayHoverStrong
                              : AppTokens.overlayRest),
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
          ),
        ),
      ),
    );
  }

  /// Popover panel — port of GPUI's `resource_indicator_panel`.
  /// 360w, rounded(14), bg #2b2d31, border white@0.08. Header
  /// (`RESOURCE USAGE` muted + refresh icon), APP SHELL section
  /// (3 stat cards: CPU / MEM / SESSIONS), TERMINAL SESSIONS
  /// section (project → task → session tree, sorted by memory
  /// then CPU then label, matching GPUI's `ResourceIndicator`).
  Widget _buildPopover(
    BuildContext context,
    ResourceUsage? usage,
    String cpuLabel,
    String memLabel,
  ) {
    final snapshot = usage?.snapshot;
    final sessionCount = snapshot != null
        ? snapshot.sessionCount.toString()
        : '—';
    final projects = snapshot?.projects ?? const <ResourceUsageProjectDto>[];
    return Stack(
      children: [
        // Outside-tap dismiss.
        Positioned.fill(
          child: GestureDetector(
            behavior: HitTestBehavior.translucent,
            onTap: () => _setPopoverVisible(false),
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
              width: 420,
              decoration: BoxDecoration(
                color: AppTokens.cardBg,
                borderRadius: BorderRadius.circular(14),
                border: Border.all(color: AppTokens.border),
                boxShadow: const [
                  BoxShadow(
                    color: Color(0x66000000),
                    blurRadius: 16,
                    offset: Offset(0, 6),
                  ),
                ],
              ),
              clipBehavior: Clip.antiAlias,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  // Header
                  Padding(
                    padding: const EdgeInsets.fromLTRB(20, 18, 20, 12),
                    child: Row(
                      children: [
                        const Expanded(
                          child: Text(
                            'RESOURCE USAGE',
                            style: TextStyle(
                              fontSize: 11,
                              fontWeight: FontWeight.w600,
                              color: Color(0x7AFFFFFF),
                              letterSpacing: 0.6,
                            ),
                          ),
                        ),
                        _PopoverIconButton(
                          asset: 'assets/icons/icons__refresh.svg',
                          tooltip: 'Refresh resource usage',
                          onTap: () => ref
                              .read(resourceUsageProvider.notifier)
                              .refreshNow(),
                        ),
                      ],
                    ),
                  ),
                  // APP SHELL heading
                  const Padding(
                    padding: EdgeInsets.fromLTRB(20, 0, 20, 10),
                    child: Text(
                      'APP SHELL',
                      style: TextStyle(
                        fontSize: 14,
                        fontWeight: FontWeight.w600,
                        color: Color(0xE5FFFFFF),
                      ),
                    ),
                  ),
                  // Stat cards
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 20),
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Expanded(
                          child: _StatCard(title: 'APP CPU', value: cpuLabel),
                        ),
                        const SizedBox(width: 12),
                        Expanded(
                          child: _StatCard(title: 'APP MEM', value: memLabel),
                        ),
                        const SizedBox(width: 12),
                        Expanded(
                          child: _StatCard(
                            title: 'SESSIONS',
                            value: sessionCount,
                          ),
                        ),
                      ],
                    ),
                  ),
                  // TERMINAL SESSIONS section
                  const Padding(
                    padding: EdgeInsets.fromLTRB(20, 16, 20, 10),
                    child: Text(
                      'TERMINAL SESSIONS',
                      style: TextStyle(
                        fontSize: 14,
                        fontWeight: FontWeight.w600,
                        color: Color(0xE5FFFFFF),
                      ),
                    ),
                  ),
                  Padding(
                    padding: const EdgeInsets.fromLTRB(20, 0, 20, 20),
                    child: snapshot == null
                        ? Container(
                            width: double.infinity,
                            padding: const EdgeInsets.symmetric(
                              horizontal: 14,
                              vertical: 10,
                            ),
                            decoration: BoxDecoration(
                              color: const Color(0x14000000),
                              borderRadius: BorderRadius.circular(10),
                            ),
                            child: const Text(
                              'Loading resource usage...',
                              style: TextStyle(
                                fontSize: 12,
                                color: Color(0x94FFFFFF),
                              ),
                            ),
                          )
                        : projects.isEmpty
                        ? Container(
                            width: double.infinity,
                            padding: const EdgeInsets.symmetric(
                              horizontal: 14,
                              vertical: 10,
                            ),
                            decoration: BoxDecoration(
                              color: const Color(0x14000000),
                              borderRadius: BorderRadius.circular(10),
                            ),
                            child: const Text(
                              'No active terminal sessions',
                              style: TextStyle(
                                fontSize: 12,
                                color: Color(0x94FFFFFF),
                              ),
                            ),
                          )
                        : Column(
                            mainAxisSize: MainAxisSize.min,
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              for (final project in projects)
                                _ProjectGroup(
                                  project: project,
                                  collapsed: _collapsedNodes.contains(
                                    project.key,
                                  ),
                                  collapsedNodes: _collapsedNodes,
                                  onToggle: (key) => setState(() {
                                    if (!_collapsedNodes.add(key)) {
                                      _collapsedNodes.remove(key);
                                    }
                                  }),
                                ),
                            ],
                          ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _PopoverIconButton extends StatefulWidget {
  const _PopoverIconButton({
    required this.asset,
    required this.tooltip,
    required this.onTap,
  });
  final String asset;
  final String tooltip;
  final VoidCallback onTap;

  @override
  State<_PopoverIconButton> createState() => _PopoverIconButtonState();
}

class _PopoverIconButtonState extends State<_PopoverIconButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    // Bare MouseRegion + GestureDetector — no Tooltip wrapper.
    // Tooltip uses an internal LayoutBuilder, and this button lives
    // inside the resource-indicator's OverlayPortal child. The
    // resource provider rebuilds this overlay on every refresh tick;
    // when LayoutBuilder is updated mid-frame from an OverlayPortal
    // rebuild it trips
    // `RenderObjectWithLayoutCallbackMixin.scheduleLayoutCallback`'s
    // `debugNeedsLayout` assertion. The icon is recognizable on its
    // own; if a hover hint becomes critical, swap in a non-
    // LayoutBuilder-based hover tip.
    return Semantics(
      label: widget.tooltip,
      button: true,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onTap,
          child: Container(
            width: 24,
            height: 24,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: _hover ? AppTokens.overlayHoverStrong : Colors.transparent,
              borderRadius: BorderRadius.circular(6),
            ),
            child: SvgPicture.asset(
              widget.asset,
              width: 14,
              height: 14,
              colorFilter: const ColorFilter.mode(
                Color(0xEBFFFFFF),
                BlendMode.srcIn,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _StatCard extends StatelessWidget {
  const _StatCard({required this.title, required this.value});
  final String title;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 14),
      decoration: BoxDecoration(
        color: const Color(0xFF363941),
        borderRadius: BorderRadius.circular(12),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            title,
            style: const TextStyle(
              fontSize: 11,
              fontWeight: FontWeight.w600,
              color: Color(0x7AFFFFFF),
              letterSpacing: 0.6,
            ),
          ),
          const SizedBox(height: 6),
          Text(
            value,
            style: const TextStyle(
              fontSize: 20,
              fontWeight: FontWeight.w600,
              color: Color(0xDBFFFFFF),
            ),
          ),
        ],
      ),
    );
  }
}

String _formatMemory(BigInt bytes) {
  const kib = 1024.0;
  const mib = kib * 1024.0;
  const gib = mib * 1024.0;
  final b = bytes.toDouble();
  if (b >= gib) return '${(b / gib).toStringAsFixed(1)} GB';
  if (b >= mib) return '${(b / mib).toStringAsFixed(1)} MB';
  if (b >= kib) return '${(b / kib).toStringAsFixed(0)} KB';
  return '${b.toStringAsFixed(0)} B';
}

class _ProjectGroup extends StatelessWidget {
  const _ProjectGroup({
    required this.project,
    required this.collapsed,
    required this.collapsedNodes,
    required this.onToggle,
  });

  final ResourceUsageProjectDto project;
  final bool collapsed;
  final Set<String> collapsedNodes;
  final void Function(String key) onToggle;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _GroupRow(
          label: project.label,
          cpuPercent: project.cpuPercent,
          memoryBytes: project.memoryBytes,
          indent: 0,
          collapsed: collapsed,
          textColor: const Color(0xD1FFFFFF),
          onTap: () => onToggle(project.key),
        ),
        if (!collapsed)
          for (final task in project.tasks)
            _TaskGroup(
              task: task,
              collapsed: collapsedNodes.contains(task.key),
              onToggle: onToggle,
            ),
      ],
    );
  }
}

class _TaskGroup extends StatelessWidget {
  const _TaskGroup({
    required this.task,
    required this.collapsed,
    required this.onToggle,
  });

  final ResourceUsageTaskDto task;
  final bool collapsed;
  final void Function(String key) onToggle;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _GroupRow(
          label: task.label,
          cpuPercent: task.cpuPercent,
          memoryBytes: task.memoryBytes,
          indent: 20,
          collapsed: collapsed,
          textColor: const Color(0xBDFFFFFF),
          onTap: () => onToggle(task.key),
        ),
        if (!collapsed)
          for (final session in task.sessions) _SessionRow(session: session),
      ],
    );
  }
}

class _GroupRow extends StatefulWidget {
  const _GroupRow({
    required this.label,
    required this.cpuPercent,
    required this.memoryBytes,
    required this.indent,
    required this.collapsed,
    required this.textColor,
    required this.onTap,
  });

  final String label;
  final double cpuPercent;
  final BigInt memoryBytes;
  final double indent;
  final bool collapsed;
  final Color textColor;
  final VoidCallback onTap;

  @override
  State<_GroupRow> createState() => _GroupRowState();
}

class _GroupRowState extends State<_GroupRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          height: 32,
          padding: EdgeInsets.only(left: widget.indent, right: 4),
          decoration: BoxDecoration(
            color: _hover ? const Color(0x0DFFFFFF) : Colors.transparent,
            borderRadius: BorderRadius.circular(8),
          ),
          child: Row(
            children: [
              SizedBox(
                width: 14,
                child: AppIcon(
                  widget.collapsed ? 'chevron-right' : 'chevron-down',
                  size: 10,
                  color: const Color(0x94FFFFFF),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  widget.label,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                    color: widget.textColor,
                  ),
                ),
              ),
              _Metrics(
                cpuPercent: widget.cpuPercent,
                memoryBytes: widget.memoryBytes,
                color: const Color(0xADFFFFFF),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _SessionRow extends StatelessWidget {
  const _SessionRow({required this.session});
  final ResourceUsageSessionDto session;

  @override
  Widget build(BuildContext context) {
    const titleCol = Color(0xA3FFFFFF);
    return Container(
      height: 32,
      padding: const EdgeInsets.only(left: 42, right: 4),
      child: Row(
        children: [
          _SessionGlyph(iconPath: session.iconPath, color: titleCol),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              session.label,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(fontSize: 12.5, color: titleCol),
            ),
          ),
          _Metrics(
            cpuPercent: session.cpuPercent,
            memoryBytes: session.memoryBytes,
            color: titleCol,
          ),
        ],
      ),
    );
  }
}

class _SessionGlyph extends StatelessWidget {
  const _SessionGlyph({required this.iconPath, required this.color});
  final String iconPath;
  final Color color;

  @override
  Widget build(BuildContext context) {
    if (iconPath.endsWith('.svg')) {
      return SvgPicture.asset(
        iconPath,
        width: 16,
        height: 16,
        colorFilter: ColorFilter.mode(color, BlendMode.srcIn),
      );
    }
    return Image.asset(iconPath, width: 16, height: 16);
  }
}

class _Metrics extends StatelessWidget {
  const _Metrics({
    required this.cpuPercent,
    required this.memoryBytes,
    required this.color,
  });
  final double cpuPercent;
  final BigInt memoryBytes;
  final Color color;

  @override
  Widget build(BuildContext context) {
    final cpu = '${cpuPercent.toStringAsFixed(1)}%';
    final mem = _formatMemory(memoryBytes);
    final style = TextStyle(fontSize: 14, color: color);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        SizedBox(width: 58, child: Text(cpu, style: style)),
        const SizedBox(width: 18),
        SizedBox(width: 84, child: Text(mem, style: style)),
      ],
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
    final (Color bg, Color border, Color text) = switch ((
      info.isDev,
      info.isDirty,
    )) {
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
    final url = ref.watch(projectGithubUrlProvider(projectId)).valueOrNull;
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
  ConsumerState<_PullRequestButton> createState() => _PullRequestButtonState();
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
              child: AppIcon('pull-request', size: 13, color: color),
            ),
          ),
        ),
      ),
    );
  }
}
