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
import '../../widgets/empty_state.dart';
import '../../widgets/pill.dart';

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
          Pill(
            label: 'Changes',
            icon: 'file_icons__changes',
            tooltip: 'View working tree changes',
            active: active == RightSidebarTab.changes,
            onTap: () => ref
                .read(rightSidebarTabProvider.notifier)
                .set(RightSidebarTab.changes),
          ),
          const SizedBox(width: AppTokens.space1),
          Pill(
            label: 'Commits',
            icon: 'git-commit',
            tooltip: 'View recent commits on the current branch',
            active: active == RightSidebarTab.commits,
            onTap: () => ref
                .read(rightSidebarTabProvider.notifier)
                .set(RightSidebarTab.commits),
          ),
          const SizedBox(width: AppTokens.space1),
          Pill(
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

class _ChangesPane extends StatelessWidget {
  const _ChangesPane();

  @override
  Widget build(BuildContext context) {
    return const EmptyState(text:'Working tree clean');
  }
}

class _CommitsPane extends StatelessWidget {
  const _CommitsPane();

  @override
  Widget build(BuildContext context) {
    return const EmptyState(text:'No commits to show');
  }
}

class _ChecksPane extends StatelessWidget {
  const _ChecksPane();

  @override
  Widget build(BuildContext context) {
    return const EmptyState(text:'No checks to show');
  }
}

