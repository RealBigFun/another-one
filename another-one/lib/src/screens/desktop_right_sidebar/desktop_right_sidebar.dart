// Right sidebar — visual port of `desktop/src/right_sidebar.rs`.
//
// 320px panel anchored to the right edge of the desktop shell.
// Header is a tab strip with three pills: Changes / Commits /
// Checks. Body is a per-tab pane.
//
// Status:
//   - Changes: flat list of files with status glyph + path + per-
//     file +N/-N badges via `read_changed_files`. Stage/unstage
//     actions and the Staged-vs-Uncommitted section grouping
//     haven't landed — that needs `ChangedFilesGitMutation`.
//   - Commits: flat list of recent commits (short SHA + subject +
//     author + relative time) via `read_recent_commits`. The
//     expandable per-commit file list GPUI paints needs a
//     `read_commit_file_changes` bridge call.
//   - Checks: list of `gh pr checks` rows via
//     `read_pull_request_checks`. Three-state UI: PR not found,
//     no checks configured, or an error from gh (CLI missing,
//     auth failure).
//
// This file's purpose is the visual + state shell so those verbs
// have a place to land without re-laying-out the chrome each time.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../rust/api/local_session.dart'
    show ChangedFileDto, CheckBucket, CheckDto, CommitDto;
import '../../state/active_project_provider.dart';
import '../../state/changed_files_provider.dart';
import '../../state/pr_checks_provider.dart';
import '../../state/recent_commits_provider.dart';
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

class _ChangesPane extends ConsumerWidget {
  const _ChangesPane();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projectId = ref.watch(activeProjectIdProvider);
    if (projectId == null) {
      return const EmptyState(text: 'No project selected');
    }
    final files = ref.watch(changedFilesProvider(projectId));
    return files.when(
      data: (data) {
        if (data.isEmpty) {
          return const EmptyState(text: 'Working tree clean');
        }
        return ListView.builder(
          padding: const EdgeInsets.symmetric(
            vertical: AppTokens.space2,
          ),
          itemCount: data.length,
          itemBuilder: (_, i) => _ChangedFileRow(file: data[i]),
        );
      },
      loading: () => const EmptyState(text: 'Reading working tree…'),
      error: (e, _) => EmptyState(text: 'Could not read changes: $e'),
    );
  }
}

/// Single-line file row: status glyph + path (with parent-dir
/// muted) + per-file +N/-N counts. Mirrors the grid GPUI paints in
/// `desktop/src/right_sidebar.rs::changed_file_row`, but flattened
/// — the Staged/Uncommitted section headers + per-file
/// stage/unstage chevrons land when the mutation bridge does.
class _ChangedFileRow extends StatelessWidget {
  const _ChangedFileRow({required this.file});

  final ChangedFileDto file;

  @override
  Widget build(BuildContext context) {
    final fileName = _basename(file.path);
    final parentDir = _parentDir(file.path);
    // Worktree status takes precedence — that's what an unstaged
    // user sees as their pending work. Untracked files are always
    // 'A' regardless of the raw char ('?' otherwise).
    final status = file.untracked
        ? 'A'
        : (file.worktreeStatus.trim().isEmpty
            ? (file.indexStatus.trim().isEmpty ? 'M' : file.indexStatus)
            : file.worktreeStatus);
    final additions = file.unstagedAdditions + file.stagedAdditions;
    final deletions = file.unstagedDeletions + file.stagedDeletions;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space3,
        vertical: AppTokens.space1,
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          SizedBox(
            width: 14,
            child: Text(
              status,
              textAlign: TextAlign.center,
              style: TextStyle(
                fontSize: AppTokens.fontSmall,
                fontWeight: FontWeight.w600,
                fontFamily: AppTokens.fontFamilyMono,
                color: _statusColor(status),
              ),
            ),
          ),
          const SizedBox(width: AppTokens.space2),
          Expanded(
            child: RichText(
              overflow: TextOverflow.ellipsis,
              text: TextSpan(
                style: const TextStyle(
                  fontSize: AppTokens.fontBody,
                  color: AppTokens.textPrimary,
                ),
                children: [
                  if (parentDir != null) ...[
                    TextSpan(
                      text: '$parentDir/',
                      style: const TextStyle(color: AppTokens.textMuted),
                    ),
                  ],
                  TextSpan(text: fileName),
                ],
              ),
            ),
          ),
          if (additions > 0 || deletions > 0) ...[
            const SizedBox(width: AppTokens.space2),
            if (additions > 0)
              Text(
                '+$additions',
                style: const TextStyle(
                  fontSize: AppTokens.fontCaption,
                  fontWeight: FontWeight.w600,
                  color: AppTokens.diffAdded,
                ),
              ),
            if (deletions > 0) ...[
              const SizedBox(width: 3),
              Text(
                '-$deletions',
                style: const TextStyle(
                  fontSize: AppTokens.fontCaption,
                  fontWeight: FontWeight.w600,
                  color: AppTokens.diffRemoved,
                ),
              ),
            ],
          ],
        ],
      ),
    );
  }

  /// Mirrors `desktop/src/right_sidebar.rs::changed_file_status_color`
  /// — A=green, D=red, R/C=blue, anything else (M/T) amber.
  Color _statusColor(String status) {
    switch (status) {
      case 'A':
        return AppTokens.diffAdded;
      case 'D':
        return AppTokens.diffRemoved;
      case 'R':
      case 'C':
        return AppTokens.accent;
      default:
        return AppTokens.warningIcon;
    }
  }

  String _basename(String path) {
    final i = path.lastIndexOf('/');
    return i < 0 ? path : path.substring(i + 1);
  }

  String? _parentDir(String path) {
    final i = path.lastIndexOf('/');
    if (i <= 0) return null;
    return path.substring(0, i);
  }
}

class _CommitsPane extends ConsumerWidget {
  const _CommitsPane();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projectId = ref.watch(activeProjectIdProvider);
    if (projectId == null) {
      return const EmptyState(text: 'No project selected');
    }
    final commits = ref.watch(recentCommitsProvider(projectId));
    return commits.when(
      data: (view) {
        if (view == null || view.commits.isEmpty) {
          return const EmptyState(text: 'No commits on this branch yet');
        }
        return ListView.builder(
          padding: const EdgeInsets.symmetric(vertical: AppTokens.space2),
          itemCount: view.commits.length,
          itemBuilder: (_, i) => _CommitRow(commit: view.commits[i]),
        );
      },
      loading: () => const EmptyState(text: 'Reading recent commits…'),
      error: (e, _) => EmptyState(text: 'Could not read commits: $e'),
    );
  }
}

/// Single-line commit row: short SHA + subject + author + time.
/// GPUI's `branch_commit_row` paints a richer expandable two-line
/// layout with file lists; this is the flat baseline so the pane
/// renders something useful while the diff bridge is built out.
class _CommitRow extends StatelessWidget {
  const _CommitRow({required this.commit});

  final CommitDto commit;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space3,
        vertical: AppTokens.space2,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              Text(
                commit.shortId,
                style: const TextStyle(
                  fontFamily: AppTokens.fontFamilyMono,
                  fontSize: AppTokens.fontSmall,
                  color: AppTokens.textMuted,
                ),
              ),
              const SizedBox(width: AppTokens.space2),
              Expanded(
                child: Text(
                  commit.subject,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: AppTokens.fontBody,
                    color: AppTokens.textPrimary,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 2),
          Padding(
            padding: const EdgeInsets.only(left: 56),
            child: Text(
              '${commit.authorName} • ${commit.authoredRelative}',
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(
                fontSize: AppTokens.fontCaption,
                color: AppTokens.textMuted,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _ChecksPane extends ConsumerWidget {
  const _ChecksPane();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projectId = ref.watch(activeProjectIdProvider);
    if (projectId == null) {
      return const EmptyState(text: 'No project selected');
    }
    final checks = ref.watch(prChecksProvider(projectId));
    return checks.when(
      data: (list) {
        if (list == null) {
          return const EmptyState(text: 'No pull request for this branch');
        }
        if (list.isEmpty) {
          return const EmptyState(text: 'No checks configured for this PR');
        }
        return ListView.builder(
          padding: const EdgeInsets.symmetric(vertical: AppTokens.space2),
          itemCount: list.length,
          itemBuilder: (_, i) => _CheckRow(check: list[i]),
        );
      },
      loading: () => const EmptyState(text: 'Loading PR checks…'),
      error: (e, _) => EmptyState(text: 'Could not load checks: $e'),
    );
  }
}

/// Single check row — bucket glyph + name + state + duration.
/// Clickable when `link` is set: opens the check's GitHub page in
/// the system browser, mirroring GPUI's
/// `right_sidebar.rs::open_external_url` chevron.
class _CheckRow extends StatelessWidget {
  const _CheckRow({required this.check});

  final CheckDto check;

  @override
  Widget build(BuildContext context) {
    final bucket = check.bucket;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space3,
        vertical: AppTokens.space2,
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          Icon(
            _bucketIcon(bucket),
            size: 16,
            color: _bucketColor(bucket),
          ),
          const SizedBox(width: AppTokens.space2),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  check.name,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: AppTokens.fontBody,
                    color: AppTokens.textPrimary,
                  ),
                ),
                Text(
                  check.state,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: AppTokens.fontCaption,
                    color: AppTokens.textMuted,
                  ),
                ),
              ],
            ),
          ),
          if (check.durationText != null) ...[
            const SizedBox(width: AppTokens.space2),
            Text(
              check.durationText!,
              style: const TextStyle(
                fontSize: AppTokens.fontCaption,
                color: AppTokens.textMuted,
                fontFamily: AppTokens.fontFamilyMono,
              ),
            ),
          ],
        ],
      ),
    );
  }

  IconData _bucketIcon(CheckBucket bucket) {
    return switch (bucket) {
      CheckBucket.pass => Icons.check_circle,
      CheckBucket.fail => Icons.error,
      CheckBucket.pending => Icons.pending,
      CheckBucket.skipping => Icons.remove_circle_outline,
      CheckBucket.cancel => Icons.cancel,
    };
  }

  Color _bucketColor(CheckBucket bucket) {
    return switch (bucket) {
      CheckBucket.pass => AppTokens.successIcon,
      CheckBucket.fail => AppTokens.errorIcon,
      CheckBucket.pending => AppTokens.warningIcon,
      CheckBucket.skipping => AppTokens.textMuted,
      CheckBucket.cancel => AppTokens.textMuted,
    };
  }
}

