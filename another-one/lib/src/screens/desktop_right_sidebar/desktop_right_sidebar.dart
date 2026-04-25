// Right sidebar — visual port of `desktop/src/right_sidebar.rs`.
//
// 320px panel anchored to the right edge of the desktop shell.
// Header is a tab strip with three pills: Changes / Commits /
// Checks. Body is a per-tab pane.
//
// Today bodies are empty-state placeholders ("Working tree
// clean", "No commits", "No checks") — real content lands when
// the matching bridge verbs come online:
//   - Changes: needs `LocalSession.git_status` and a diff list
//     mirroring `git_service::spawn_refresh`'s ChangedFiles output.
//   - Commits: needs `LocalSession.recent_commits` exposing the
//     log of the active branch.
//   - Checks: needs `LocalSession.pull_request_check_runs` reading
//     from the GitHub-association cache the daemon already builds.
//
// This file's purpose is the visual + state shell so those verbs
// have a place to land without re-laying-out the chrome each time.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../state/right_sidebar_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';

const double _rightSidebarWidth = 320;

class DesktopRightSidebar extends ConsumerWidget {
  const DesktopRightSidebar({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tab = ref.watch(rightSidebarTabProvider);
    return Container(
      width: _rightSidebarWidth,
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(
          left: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: Column(
        children: [
          const _RightTabStrip(),
          Expanded(
            child: switch (tab) {
              RightSidebarTab.changes => const _ChangesPane(),
              RightSidebarTab.commits => const _CommitsPane(),
              RightSidebarTab.checks => const _ChecksPane(),
            },
          ),
        ],
      ),
    );
  }
}

class _RightTabStrip extends ConsumerWidget {
  const _RightTabStrip();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final active = ref.watch(rightSidebarTabProvider);
    return Container(
      height: AppTokens.tabStripHeight,
      decoration: const BoxDecoration(
        border: Border(
          bottom: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space2,
        vertical: AppTokens.space1,
      ),
      child: Row(
        children: [
          _TabPill(
            label: 'Changes',
            icon: 'file_icons__changes',
            tooltip: 'View working tree changes',
            active: active == RightSidebarTab.changes,
            onTap: () => ref
                .read(rightSidebarTabProvider.notifier)
                .set(RightSidebarTab.changes),
          ),
          const SizedBox(width: AppTokens.space1),
          _TabPill(
            label: 'Commits',
            icon: 'git-commit',
            tooltip: 'View recent commits on the current branch',
            active: active == RightSidebarTab.commits,
            onTap: () => ref
                .read(rightSidebarTabProvider.notifier)
                .set(RightSidebarTab.commits),
          ),
          const SizedBox(width: AppTokens.space1),
          _TabPill(
            label: 'Checks',
            icon: 'tool-check',
            tooltip: 'View CI checks for the current pull request',
            active: active == RightSidebarTab.checks,
            onTap: () => ref
                .read(rightSidebarTabProvider.notifier)
                .set(RightSidebarTab.checks),
          ),
        ],
      ),
    );
  }
}

class _TabPill extends StatefulWidget {
  const _TabPill({
    required this.label,
    required this.icon,
    required this.tooltip,
    required this.active,
    required this.onTap,
  });

  final String label;
  final String icon;
  final String tooltip;
  final bool active;
  final VoidCallback onTap;

  @override
  State<_TabPill> createState() => _TabPillState();
}

class _TabPillState extends State<_TabPill> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final bg = widget.active
        ? AppTokens.overlayActive
        : (_hovered ? AppTokens.overlayHover : Colors.transparent);
    final color = widget.active
        ? AppTokens.textPrimary
        : AppTokens.textSecondary;
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onTap,
          child: Container(
            height: 26,
            padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.space3,
            ),
            decoration: BoxDecoration(
              color: bg,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                AppIcon(widget.icon, size: 12, color: color),
                const SizedBox(width: AppTokens.space2),
                Text(
                  widget.label,
                  style: TextStyle(
                    fontSize: AppTokens.fontBodyLg,
                    fontWeight: FontWeight.w500,
                    color: color,
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

class _ChangesPane extends StatelessWidget {
  const _ChangesPane();

  @override
  Widget build(BuildContext context) {
    return const _PaneEmptyState(text: 'Working tree clean');
  }
}

class _CommitsPane extends StatelessWidget {
  const _CommitsPane();

  @override
  Widget build(BuildContext context) {
    return const _PaneEmptyState(text: 'No commits to show');
  }
}

class _ChecksPane extends StatelessWidget {
  const _ChecksPane();

  @override
  Widget build(BuildContext context) {
    return const _PaneEmptyState(text: 'No checks to show');
  }
}

/// Single centered line — matches GPUI's "Working tree clean"
/// empty state shown by `right_sidebar.rs` when there are no
/// changed files.
class _PaneEmptyState extends StatelessWidget {
  const _PaneEmptyState({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Text(
        text,
        style: const TextStyle(
          fontSize: AppTokens.fontBodyLg,
          color: AppTokens.textMuted,
        ),
      ),
    );
  }
}
