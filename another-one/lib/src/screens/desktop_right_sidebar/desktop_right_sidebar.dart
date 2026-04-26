// Right sidebar — visual port of `desktop/src/right_sidebar.rs`.
//
// 320px panel anchored to the right edge of the desktop shell.
// Header is a tab strip with three pills: Changes / Commits /
// Checks. Body is a per-tab pane.
//
// Status:
//   - Changes: Staged / Uncommitted sections with per-section
//     totals + "stage/unstage all" action and per-row +/-
//     stage/unstage buttons. Reads through `read_changed_files`,
//     mutates through `stage_changed_file` / `stage_all_changes`
//     and the unstage equivalents. Discard-all (the destructive
//     companion GPUI ships) is deferred.
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
import '../../state/local_connection_provider.dart';
import '../../state/pr_checks_provider.dart';
import '../../state/recent_commits_provider.dart';
import '../../state/right_sidebar_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';
import '../../widgets/empty_state.dart';
import '../../widgets/pill.dart';
import '../../widgets/run_mutation.dart';

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
        final staged = data.where(_hasStagedChanges).toList();
        final uncommitted = data.where(_hasUnstagedChanges).toList();
        return ListView(
          padding: const EdgeInsets.symmetric(vertical: AppTokens.space2),
          children: [
            if (staged.isNotEmpty)
              _ChangedFilesSection(
                projectId: projectId,
                title: 'Staged',
                files: staged,
                group: _ChangeGroup.staged,
              ),
            if (uncommitted.isNotEmpty)
              _ChangedFilesSection(
                projectId: projectId,
                title: 'Uncommitted',
                files: uncommitted,
                group: _ChangeGroup.uncommitted,
              ),
          ],
        );
      },
      loading: () => const EmptyState(text: 'Reading working tree…'),
      error: (e, _) => EmptyState(text: 'Could not read changes: $e'),
    );
  }
}

/// Which side of the staged/working-tree split a row is rendered
/// for. A file with both staged and unstaged changes appears in
/// both sections; the row's stage/unstage button + diff counts
/// switch on this enum.
enum _ChangeGroup { staged, uncommitted }

bool _hasStagedChanges(ChangedFileDto f) {
  final c = f.indexStatus;
  return c.isNotEmpty && c != ' ' && c != '?';
}

bool _hasUnstagedChanges(ChangedFileDto f) {
  if (f.untracked) return true;
  final c = f.worktreeStatus;
  return c.isNotEmpty && c != ' ' && c != '?';
}

/// Status char to render in the gutter for a given group. Mirrors
/// GPUI's `changed_file_status_char`: untracked → 'A' on the
/// uncommitted side; ' ' → 'M' (modified-but-no-status-char shows
/// up on rename/copy partners).
String _rowStatus(ChangedFileDto f, _ChangeGroup group) {
  final raw = group == _ChangeGroup.staged
      ? f.indexStatus
      : (f.untracked ? 'A' : f.worktreeStatus);
  if (raw == '?') return 'A';
  if (raw.trim().isEmpty) return 'M';
  return raw;
}

class _ChangedFilesSection extends ConsumerWidget {
  const _ChangedFilesSection({
    required this.projectId,
    required this.title,
    required this.files,
    required this.group,
  });

  final String projectId;
  final String title;
  final List<ChangedFileDto> files;
  final _ChangeGroup group;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final additions = files.fold<int>(
      0,
      (a, f) => a +
          (group == _ChangeGroup.staged ? f.stagedAdditions : f.unstagedAdditions),
    );
    final deletions = files.fold<int>(
      0,
      (a, f) => a +
          (group == _ChangeGroup.staged ? f.stagedDeletions : f.unstagedDeletions),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _ChangedFilesSectionHeader(
          title: title,
          fileCount: files.length,
          additions: additions,
          deletions: deletions,
          group: group,
          onStageAll: group == _ChangeGroup.uncommitted
              ? () => _stageAll(context, ref)
              : null,
          onUnstageAll: group == _ChangeGroup.staged
              ? () => _unstageAll(context, ref)
              : null,
        ),
        for (final file in files)
          _ChangedFileRow(
            projectId: projectId,
            file: file,
            group: group,
          ),
      ],
    );
  }

  Future<void> _stageAll(BuildContext context, WidgetRef ref) async {
    final connection = ref.read(localConnectionProvider);
    await runMutation<bool>(
      context,
      () async {
        await connection.stageAllChanges(projectId);
        return true;
      },
      errorPrefix: 'Could not stage all changes',
    );
    ref.invalidate(changedFilesProvider(projectId));
  }

  Future<void> _unstageAll(BuildContext context, WidgetRef ref) async {
    final connection = ref.read(localConnectionProvider);
    await runMutation<bool>(
      context,
      () async {
        await connection.unstageAllChanges(projectId);
        return true;
      },
      errorPrefix: 'Could not unstage all changes',
    );
    ref.invalidate(changedFilesProvider(projectId));
  }
}

class _ChangedFilesSectionHeader extends StatelessWidget {
  const _ChangedFilesSectionHeader({
    required this.title,
    required this.fileCount,
    required this.additions,
    required this.deletions,
    required this.group,
    required this.onStageAll,
    required this.onUnstageAll,
  });

  final String title;
  final int fileCount;
  final int additions;
  final int deletions;
  final _ChangeGroup group;
  final VoidCallback? onStageAll;
  final VoidCallback? onUnstageAll;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(
        AppTokens.space3,
        AppTokens.space2,
        AppTokens.space2,
        AppTokens.space1,
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          Text(
            title,
            style: const TextStyle(
              fontSize: AppTokens.fontSmall,
              fontWeight: FontWeight.w600,
              color: AppTokens.textPrimary,
              letterSpacing: 0.4,
            ),
          ),
          const SizedBox(width: AppTokens.space2),
          Text(
            '$fileCount',
            style: const TextStyle(
              fontSize: AppTokens.fontCaption,
              color: AppTokens.textMuted,
            ),
          ),
          const Spacer(),
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
            const SizedBox(width: 4),
            Text(
              '-$deletions',
              style: const TextStyle(
                fontSize: AppTokens.fontCaption,
                fontWeight: FontWeight.w600,
                color: AppTokens.diffRemoved,
              ),
            ),
          ],
          if (onStageAll case final cb?) ...[
            const SizedBox(width: AppTokens.space2),
            _IconActionButton(
              icon: 'plus',
              tooltip: 'Stage all changes',
              onPressed: cb,
            ),
          ],
          if (onUnstageAll case final cb?) ...[
            const SizedBox(width: AppTokens.space2),
            _IconActionButton(
              icon: 'pin-off',
              tooltip: 'Unstage all changes',
              onPressed: cb,
            ),
          ],
        ],
      ),
    );
  }
}

class _ChangedFileRow extends ConsumerWidget {
  const _ChangedFileRow({
    required this.projectId,
    required this.file,
    required this.group,
  });

  final String projectId;
  final ChangedFileDto file;
  final _ChangeGroup group;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final fileName = _basename(file.path);
    final parentDir = _parentDir(file.path);
    final status = _rowStatus(file, group);
    final additions = group == _ChangeGroup.staged
        ? file.stagedAdditions
        : file.unstagedAdditions;
    final deletions = group == _ChangeGroup.staged
        ? file.stagedDeletions
        : file.unstagedDeletions;
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
          const SizedBox(width: AppTokens.space2),
          if (group == _ChangeGroup.uncommitted)
            _IconActionButton(
              icon: 'plus',
              tooltip: 'Stage this file',
              onPressed: () => _stage(context, ref),
            )
          else
            _IconActionButton(
              icon: 'pin-off',
              tooltip: 'Unstage this file',
              onPressed: () => _unstage(context, ref),
            ),
        ],
      ),
    );
  }

  Future<void> _stage(BuildContext context, WidgetRef ref) async {
    final connection = ref.read(localConnectionProvider);
    await runMutation<bool>(
      context,
      () async {
        await connection.stageChangedFile(
          projectId: projectId,
          path: file.path,
          originalPath: file.originalPath,
        );
        return true;
      },
      errorPrefix: 'Could not stage ${file.path}',
    );
    ref.invalidate(changedFilesProvider(projectId));
  }

  Future<void> _unstage(BuildContext context, WidgetRef ref) async {
    final connection = ref.read(localConnectionProvider);
    await runMutation<bool>(
      context,
      () async {
        await connection.unstageChangedFile(
          projectId: projectId,
          path: file.path,
          originalPath: file.originalPath,
        );
        return true;
      },
      errorPrefix: 'Could not unstage ${file.path}',
    );
    ref.invalidate(changedFilesProvider(projectId));
  }
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

/// Small in-row icon button used for per-file and per-section
/// actions. Sized like GPUI's `changed_file_action_button`.
class _IconActionButton extends StatefulWidget {
  const _IconActionButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
  });

  final String icon;
  final String tooltip;
  final VoidCallback onPressed;

  @override
  State<_IconActionButton> createState() => _IconActionButtonState();
}

class _IconActionButtonState extends State<_IconActionButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: Container(
            width: 18,
            height: 18,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: _hover ? AppTokens.overlayHover : Colors.transparent,
              borderRadius: BorderRadius.circular(AppTokens.radiusXs),
            ),
            child: AppIcon(
              widget.icon,
              size: 11,
              color: AppTokens.textSecondary,
            ),
          ),
        ),
      ),
    );
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

