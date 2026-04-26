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
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../rust/api/local_session.dart'
    show
        BranchCompareFileDto,
        ChangedFileDto,
        CheckBucket,
        CheckDto,
        CommitDto;
import '../../state/active_project_provider.dart';
import '../../state/changed_files_pending_provider.dart';
import '../../state/changed_files_provider.dart';
import '../../state/changes_section_collapse_provider.dart';
import '../../state/commit_file_changes_provider.dart';
import '../../state/commit_row_expanded_provider.dart';
import '../../state/local_connection_provider.dart';
import '../../state/pr_checks_provider.dart';
import '../../state/recent_commits_provider.dart';
import '../../state/right_sidebar_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';
import '../../widgets/empty_state.dart';
import '../../widgets/pill.dart';
import '../../widgets/run_mutation.dart';
import '../../widgets/toolbar_spinner.dart';

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
                title: 'Staged Changes',
                sectionKey: 'staged',
                files: staged,
                group: _ChangeGroup.staged,
              ),
            if (uncommitted.isNotEmpty)
              _ChangedFilesSection(
                projectId: projectId,
                title: 'Changes',
                sectionKey: 'uncommitted',
                files: uncommitted,
                group: _ChangeGroup.uncommitted,
              ),
          ],
        );
      },
      loading: () => const EmptyState(text: 'Loading changes...'),
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
    required this.sectionKey,
    required this.files,
    required this.group,
  });

  final String projectId;
  final String title;
  final String sectionKey;
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
    final collapseKey = '$projectId:$sectionKey';
    final collapsed =
        ref.watch(changesSectionCollapseProvider).contains(collapseKey);
    final pending = ref.watch(changedFilesPendingProvider(projectId));
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _ChangedFilesSectionHeader(
          title: title,
          fileCount: files.length,
          additions: additions,
          deletions: deletions,
          collapsed: collapsed,
          actionsBusy: pending.actionsBusy,
          stageAllPending:
              pending.isProjectActionPending(ProjectAction.stageAll),
          unstageAllPending:
              pending.isProjectActionPending(ProjectAction.unstageAll),
          discardAllPending:
              pending.isProjectActionPending(ProjectAction.discardAll),
          onToggleCollapse: () => ref
              .read(changesSectionCollapseProvider.notifier)
              .toggle(collapseKey),
          onStageAll: group == _ChangeGroup.uncommitted
              ? () => _stageAll(context, ref)
              : null,
          onUnstageAll: group == _ChangeGroup.staged
              ? () => _unstageAll(context, ref)
              : null,
          onDiscardAll: group == _ChangeGroup.uncommitted
              ? () => _discardAll(context, ref)
              : null,
        ),
        if (!collapsed)
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
    await _withProjectPending(ref, ProjectAction.stageAll, () async {
      final connection = ref.read(localConnectionProvider);
      await runMutation<bool>(
        context,
        () async {
          await connection.stageAllChanges(projectId);
          return true;
        },
        errorPrefix: 'Could not stage all changes',
      );
    });
    ref.invalidate(changedFilesProvider(projectId));
  }

  Future<void> _unstageAll(BuildContext context, WidgetRef ref) async {
    await _withProjectPending(ref, ProjectAction.unstageAll, () async {
      final connection = ref.read(localConnectionProvider);
      await runMutation<bool>(
        context,
        () async {
          await connection.unstageAllChanges(projectId);
          return true;
        },
        errorPrefix: 'Could not unstage all changes',
      );
    });
    ref.invalidate(changedFilesProvider(projectId));
  }

  Future<void> _discardAll(BuildContext context, WidgetRef ref) async {
    final ok = await showDiscardConfirmDialog(context, files: files);
    if (!ok || !context.mounted) return;
    await _withProjectPending(ref, ProjectAction.discardAll, () async {
      final connection = ref.read(localConnectionProvider);
      // Loop the per-file discard verb because core has no
      // discard-all primitive (revert_changed_file is per-path).
      // Partial discard is still useful, so collect failures and
      // surface them at the end rather than aborting on first error.
      final List<String> failures = [];
      for (final f in files) {
        try {
          await connection.discardChangedFile(
            projectId: projectId,
            path: f.path,
            originalPath: f.originalPath,
            untracked: f.untracked,
          );
        } catch (e) {
          failures.add('${f.path}: $e');
        }
      }
      if (failures.isNotEmpty && context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Could not discard ${failures.length} file(s)'),
            backgroundColor: AppTokens.errorBg,
          ),
        );
      }
    });
    ref.invalidate(changedFilesProvider(projectId));
  }

  Future<void> _withProjectPending(
    WidgetRef ref,
    ProjectAction action,
    Future<void> Function() body,
  ) async {
    final notifier = ref.read(changedFilesPendingProvider(projectId).notifier);
    notifier.startProject(action);
    try {
      await body();
    } finally {
      notifier.endProject(action);
    }
  }
}

class _ChangedFilesSectionHeader extends StatefulWidget {
  const _ChangedFilesSectionHeader({
    required this.title,
    required this.fileCount,
    required this.additions,
    required this.deletions,
    required this.collapsed,
    required this.actionsBusy,
    required this.stageAllPending,
    required this.unstageAllPending,
    required this.discardAllPending,
    required this.onToggleCollapse,
    required this.onStageAll,
    required this.onUnstageAll,
    required this.onDiscardAll,
  });

  final String title;
  final int fileCount;
  final int additions;
  final int deletions;
  final bool collapsed;
  final bool actionsBusy;
  final bool stageAllPending;
  final bool unstageAllPending;
  final bool discardAllPending;
  final VoidCallback onToggleCollapse;
  final VoidCallback? onStageAll;
  final VoidCallback? onUnstageAll;
  final VoidCallback? onDiscardAll;

  @override
  State<_ChangedFilesSectionHeader> createState() =>
      _ChangedFilesSectionHeaderState();
}

class _ChangedFilesSectionHeaderState
    extends State<_ChangedFilesSectionHeader> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: 'Expand or collapse this section',
      waitDuration: const Duration(milliseconds: 500),
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onToggleCollapse,
          child: Container(
            constraints: const BoxConstraints(minHeight: 44),
            padding: const EdgeInsets.symmetric(horizontal: 14),
            decoration: BoxDecoration(
              color: _hover
                  ? const Color(0x08FFFFFF) // white @ 0.03
                  : Colors.transparent,
              border: const Border(
                bottom: BorderSide(color: AppTokens.divider, width: 0.5),
              ),
            ),
            child: Row(
              children: [
                Expanded(
                  child: Row(
                    children: [
                      AppIcon(
                        widget.collapsed ? 'chevron-right' : 'chevron-down',
                        size: 10,
                        color: const Color(0xBDFFFFFF), // 0.74 white
                      ),
                      const SizedBox(width: 6),
                      Flexible(
                        child: Text(
                          '${widget.title} (${widget.fileCount})',
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(
                            fontSize: 11,
                            fontWeight: FontWeight.w600,
                            color: Color(0xEBFFFFFF),
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
                _DiffBadge(value: widget.additions, positive: true),
                const SizedBox(width: 8),
                _DiffBadge(value: widget.deletions, positive: false),
                if (widget.onStageAll case final cb?) ...[
                  const SizedBox(width: 8),
                  _IconActionButton(
                    icon: 'plus',
                    tooltip: 'Stage All Changes',
                    onPressed: cb,
                    enabled: !widget.actionsBusy,
                    pending: widget.stageAllPending,
                  ),
                ],
                if (widget.onUnstageAll case final cb?) ...[
                  const SizedBox(width: 8),
                  _IconActionButton(
                    icon: 'minus',
                    tooltip: 'Unstage all files in this section',
                    onPressed: cb,
                    enabled: !widget.actionsBusy,
                    pending: widget.unstageAllPending,
                  ),
                ],
                if (widget.onDiscardAll case final cb?) ...[
                  const SizedBox(width: 8),
                  _IconActionButton(
                    icon: 'discard',
                    tooltip: 'Discard All Changes',
                    onPressed: cb,
                    enabled: !widget.actionsBusy,
                    pending: widget.discardAllPending,
                  ),
                ],
              ],
            ),
          ),
        ),
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
    final pending = ref.watch(changedFilesPendingProvider(projectId));
    final fileName = _basename(file.path);
    final parentDir = _parentDir(file.path);
    final status = _rowStatus(file, group);
    final additions = group == _ChangeGroup.staged
        ? file.stagedAdditions
        : file.unstagedAdditions;
    final deletions = group == _ChangeGroup.staged
        ? file.stagedDeletions
        : file.unstagedDeletions;
    final filePending = pending.isFilePending(file.path);
    final actionsBusy = pending.actionsBusy;
    final projectMutationsPending = pending.projectMutationsPending;
    // Mirror desktop/src/right_sidebar.rs::changed_file_row exactly:
    //   pl(22) pr(14) mx(4), rounded_md, hover white@0.04
    //   gap 12 between (status+name+parent) cluster and stats+actions
    //   status: min_w 18, bold 12px, status_color
    //   name + parent share gap(6); name truncated, parent dir to the
    //   right (truncated, smaller, muted)
    return _RowHover(
      child: Padding(
        padding: const EdgeInsets.only(left: 22, right: 14),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            Expanded(
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.center,
                children: [
                  SizedBox(
                    width: 18,
                    child: Text(
                      status,
                      style: TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w700,
                        color: _statusColor(status),
                      ),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Flexible(
                    child: Text(
                      fileName,
                      overflow: TextOverflow.ellipsis,
                      maxLines: 1,
                      style: const TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w500,
                        color: Color(0xF0FFFFFF),
                      ),
                    ),
                  ),
                  if (parentDir != null) ...[
                    const SizedBox(width: 6),
                    Flexible(
                      child: Text(
                        parentDir,
                        overflow: TextOverflow.ellipsis,
                        maxLines: 1,
                        style: const TextStyle(
                          fontSize: 11,
                          color: Color(0x94FFFFFF),
                        ),
                      ),
                    ),
                  ],
                ],
              ),
            ),
            const SizedBox(width: 12),
            if (additions > 0)
              _DiffBadge(value: additions, positive: true),
            if (deletions > 0) ...[
              const SizedBox(width: 8),
              _DiffBadge(value: deletions, positive: false),
            ],
            const SizedBox(width: 8),
            if (group == _ChangeGroup.uncommitted) ...[
              _IconActionButton(
                icon: 'plus',
                tooltip: 'Stage File',
                onPressed: () => _stage(context, ref),
                enabled: !actionsBusy,
                pending: filePending,
              ),
              const SizedBox(width: 4),
              _IconActionButton(
                icon: 'discard',
                tooltip: 'Discard File Changes',
                onPressed: () => _confirmDiscard(context, ref),
                enabled: !actionsBusy && !projectMutationsPending,
              ),
            ] else
              _IconActionButton(
                icon: 'minus',
                tooltip: 'Unstage file',
                onPressed: () => _unstage(context, ref),
                enabled: !actionsBusy,
                pending: filePending,
              ),
          ],
        ),
      ),
    );
  }

  Future<void> _confirmDiscard(BuildContext context, WidgetRef ref) async {
    final ok = await showDiscardConfirmDialog(context, files: [file]);
    if (!ok || !context.mounted) return;
    await _withFilePending(ref, () async {
      final connection = ref.read(localConnectionProvider);
      await runMutation<bool>(
        context,
        () async {
          await connection.discardChangedFile(
            projectId: projectId,
            path: file.path,
            originalPath: file.originalPath,
            untracked: file.untracked,
          );
          return true;
        },
        errorPrefix: 'Could not discard ${file.path}',
      );
    });
    ref.invalidate(changedFilesProvider(projectId));
  }

  Future<void> _stage(BuildContext context, WidgetRef ref) async {
    await _withFilePending(ref, () async {
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
    });
    ref.invalidate(changedFilesProvider(projectId));
  }

  Future<void> _unstage(BuildContext context, WidgetRef ref) async {
    await _withFilePending(ref, () async {
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
    });
    ref.invalidate(changedFilesProvider(projectId));
  }

  Future<void> _withFilePending(
    WidgetRef ref,
    Future<void> Function() body,
  ) async {
    final notifier = ref.read(changedFilesPendingProvider(projectId).notifier);
    notifier.startFile(file.path);
    try {
      await body();
    } finally {
      notifier.endFile(file.path);
    }
  }
}

/// Mirrors `desktop/src/right_sidebar.rs::changed_file_status_color`.
/// These differ from the diff-badge colors (which are paler) — the
/// status chars sit alone in the gutter and need higher saturation
/// to read at a glance. HSL values from the GPUI source, computed
/// once into sRGB constants here so we don't re-compute every
/// build.
///
/// A → hsla(135/360, 0.70, 0.68) = #74E691
/// D → hsla(0, 0.72, 0.68)       = #E87373
/// R/C → hsla(210/360, 0.72, 0.72) = #7BB6E0
/// other → hsla(50/360, 0.90, 0.60) = #F5C229
Color _statusColor(String status) {
  switch (status) {
    case 'A':
      return const Color(0xFF74E691);
    case 'D':
      return const Color(0xFFE87373);
    case 'R':
    case 'C':
      return const Color(0xFF7BB6E0);
    default:
      return const Color(0xFFF5C229);
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

/// In-row icon button matching GPUI's `changed_file_action_button`:
/// 28×28, rounded_md, 16px svg, icon at white@0.72, hover bg
/// white@0.08, opacity 0.35 when disabled.
///
/// `pending: true` swaps the icon for a [ToolbarSpinner] (GPUI's
/// `changed_file_action_pending` substitution).
/// `enabled: false` greys the button to opacity 0.35, hides hover
/// bg, and disables the click handler — mirrors GPUI's
/// `enabled: !actions_busy` gate.
class _IconActionButton extends StatefulWidget {
  const _IconActionButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    this.enabled = true,
    this.pending = false,
  });

  final String icon;
  final String tooltip;
  final VoidCallback onPressed;
  final bool enabled;
  final bool pending;

  @override
  State<_IconActionButton> createState() => _IconActionButtonState();
}

class _IconActionButtonState extends State<_IconActionButton> {
  bool _hover = false;

  static const Color _iconColor = Color(0xB8FFFFFF); // white @ 0.72

  @override
  Widget build(BuildContext context) {
    if (widget.pending) {
      return const SizedBox(
        width: 28,
        height: 28,
        child: Center(
          child: ToolbarSpinner(size: 14, color: _iconColor),
        ),
      );
    }
    final container = Opacity(
      opacity: widget.enabled ? 1.0 : 0.35,
      child: Container(
        width: 28,
        height: 28,
        alignment: Alignment.center,
        decoration: BoxDecoration(
          color: widget.enabled && _hover
              ? AppTokens.overlayHoverStrong
              : Colors.transparent,
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        ),
        child: AppIcon(
          widget.icon,
          size: 16,
          color: _iconColor,
        ),
      ),
    );
    if (!widget.enabled) return container;
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: container,
        ),
      ),
    );
  }
}

/// Wraps a Changes-pane row so the whole row gets a subtle
/// white@0.04 hover background — mirrors `changed_file_row`'s
/// `hover(row_hover)` rule. The row content itself stays
/// non-interactive (only the per-row buttons are clickable).
class _RowHover extends StatefulWidget {
  const _RowHover({required this.child});

  final Widget child;

  @override
  State<_RowHover> createState() => _RowHoverState();
}

class _RowHoverState extends State<_RowHover> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: Container(
        margin: const EdgeInsets.symmetric(horizontal: 4),
        decoration: BoxDecoration(
          color: _hover ? const Color(0x0AFFFFFF) : Colors.transparent,
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        ),
        child: widget.child,
      ),
    );
  }
}

/// `+N` / `-N` diff badge matching `git_diff_badge`: semibold,
/// font 11px (font_px capped at 11), green/red HSL constants from
/// `desktop/src/right_sidebar.rs`.
class _DiffBadge extends StatelessWidget {
  const _DiffBadge({required this.value, required this.positive});

  final int value;
  final bool positive;

  @override
  Widget build(BuildContext context) {
    return Text(
      positive ? '+$value' : '-$value',
      style: TextStyle(
        fontSize: 11,
        fontWeight: FontWeight.w600,
        color: positive ? AppTokens.diffAdded : AppTokens.diffRemoved,
      ),
    );
  }
}

/// Confirmation modal mirroring
/// `desktop/src/right_sidebar.rs::discard_confirm_modal`: 320px
/// card, "Confirm Discard" title, "This action cannot be undone."
/// caption, Cancel + Discard (danger-bg) buttons. Returns true when
/// the user confirmed.
///
/// Keyboard parity: Esc cancels, Enter confirms — same bindings GPUI
/// installs via `on_key_down` on the overlay.
Future<bool> showDiscardConfirmDialog(
  BuildContext context, {
  required List<ChangedFileDto> files,
}) async {
  final fileCount = files.length;
  final message = fileCount == 1
      ? 'Discard changes to "${_basename(files[0].path)}"?'
      : 'Discard changes to $fileCount files?';
  final result = await showDialog<bool>(
    context: context,
    barrierColor: const Color(0x80000000),
    builder: (ctx) => Dialog(
      backgroundColor: Colors.transparent,
      elevation: 0,
      insetPadding: EdgeInsets.zero,
      child: Shortcuts(
        shortcuts: const {
          SingleActivator(LogicalKeyboardKey.escape):
              _DiscardDismissIntent(),
          SingleActivator(LogicalKeyboardKey.enter):
              _DiscardConfirmIntent(),
          SingleActivator(LogicalKeyboardKey.numpadEnter):
              _DiscardConfirmIntent(),
        },
        child: Actions(
          actions: {
            _DiscardDismissIntent: CallbackAction<_DiscardDismissIntent>(
              onInvoke: (_) {
                Navigator.of(ctx).pop(false);
                return null;
              },
            ),
            _DiscardConfirmIntent: CallbackAction<_DiscardConfirmIntent>(
              onInvoke: (_) {
                Navigator.of(ctx).pop(true);
                return null;
              },
            ),
          },
          child: Focus(
            autofocus: true,
            child: _DiscardCard(message: message),
          ),
        ),
      ),
    ),
  );
  return result ?? false;
}

class _DiscardDismissIntent extends Intent {
  const _DiscardDismissIntent();
}

class _DiscardConfirmIntent extends Intent {
  const _DiscardConfirmIntent();
}

class _DiscardCard extends StatelessWidget {
  const _DiscardCard({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 320,
      decoration: BoxDecoration(
        color: AppTokens.cardBg,
        borderRadius: BorderRadius.circular(AppTokens.radiusLg),
        border: Border.all(color: AppTokens.border),
        boxShadow: const [
          BoxShadow(
            color: Color(0x66000000),
            blurRadius: 24,
            offset: Offset(0, 8),
          ),
        ],
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 20, 20, 12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  'Confirm Discard',
                  style: TextStyle(
                    fontSize: 14,
                    fontWeight: FontWeight.w600,
                    color: AppTokens.textPrimary,
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  message,
                  style: const TextStyle(
                    fontSize: 12,
                    color: AppTokens.textSecondary,
                  ),
                ),
                const SizedBox(height: 4),
                const Text(
                  'This action cannot be undone.',
                  style: TextStyle(
                    fontSize: 11,
                    color: AppTokens.textMuted,
                  ),
                ),
              ],
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 8, 20, 16),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                _DialogButton(
                  label: 'Cancel',
                  tooltip: 'Close without discarding changes',
                  fontWeight: FontWeight.w500,
                  background: AppTokens.overlayHoverStrong,
                  hoverBg: const Color(0x24FFFFFF), // white @ 0.14
                  onPressed: () => Navigator.of(context).pop(false),
                ),
                const SizedBox(width: 8),
                _DialogButton(
                  label: 'Discard',
                  tooltip: 'Permanently discard the selected changes',
                  fontWeight: FontWeight.w600,
                  // hsla(0, 0.62, 0.50) ≈ #cf3030 ; hover 0.58 lightness ≈ #db4f4f
                  background: const Color(0xFFCF3030),
                  hoverBg: const Color(0xFFDB4F4F),
                  onPressed: () => Navigator.of(context).pop(true),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _DialogButton extends StatefulWidget {
  const _DialogButton({
    required this.label,
    required this.tooltip,
    required this.fontWeight,
    required this.background,
    required this.hoverBg,
    required this.onPressed,
  });

  final String label;
  final String tooltip;
  final FontWeight fontWeight;
  final Color background;
  final Color hoverBg;
  final VoidCallback onPressed;

  @override
  State<_DialogButton> createState() => _DialogButtonState();
}

class _DialogButtonState extends State<_DialogButton> {
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
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 6),
            decoration: BoxDecoration(
              color: _hover ? widget.hoverBg : widget.background,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            ),
            child: Text(
              widget.label,
              style: TextStyle(
                fontSize: 12,
                fontWeight: widget.fontWeight,
                color: AppTokens.textPrimary,
              ),
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
    final fallbackBranch = ref.watch(activeBranchNameProvider);
    return commits.when(
      data: (view) => _CommitsBody(
        projectId: projectId,
        view: view,
        fallbackBranch: fallbackBranch,
      ),
      loading: () => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _CommitsHeader(
            branchName: fallbackBranch,
            summary: 'Recent commits from HEAD.',
          ),
          const Expanded(child: EmptyState(text: 'Loading commits...')),
        ],
      ),
      error: (e, _) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _CommitsHeader(
            branchName: fallbackBranch,
            summary: 'Recent commits from HEAD.',
          ),
          Expanded(child: EmptyState(text: 'Could not read commits: $e')),
        ],
      ),
    );
  }
}

class _CommitsBody extends ConsumerWidget {
  const _CommitsBody({
    required this.projectId,
    required this.view,
    required this.fallbackBranch,
  });

  final String projectId;
  final dynamic view; // RecentCommitsView? — keep loose to avoid extra import.
  final String? fallbackBranch;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final v = view;
    if (v == null) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _CommitsHeader(
            branchName: fallbackBranch,
            summary: 'Recent commits from HEAD.',
          ),
          const Expanded(
            child: EmptyState(text: 'No commits yet on this branch.'),
          ),
        ],
      );
    }
    final List<CommitDto> commits = (v.commits as List).cast<CommitDto>();
    final bool hasMore = v.hasMore as bool;
    final String? branchName = (v.currentBranch as String?) ?? fallbackBranch;
    final summary = hasMore
        ? '${commits.length} shown'
        : '${commits.length} recent commits';

    if (commits.isEmpty) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _CommitsHeader(branchName: branchName, summary: summary),
          const Expanded(
            child: EmptyState(text: 'No commits yet on this branch.'),
          ),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _CommitsHeader(branchName: branchName, summary: summary),
        Expanded(
          child: ListView.builder(
            padding: const EdgeInsets.symmetric(vertical: 8),
            itemCount: commits.length + (hasMore ? 1 : 0),
            itemBuilder: (_, i) {
              if (i == commits.length) {
                return _LoadMoreCommitsRow(projectId: projectId);
              }
              return _CommitRow(
                projectId: projectId,
                commit: commits[i],
                isFirst: i == 0,
              );
            },
          ),
        ),
      ],
    );
  }
}

class _CommitsHeader extends StatelessWidget {
  const _CommitsHeader({required this.branchName, required this.summary});

  final String? branchName;
  final String summary;

  @override
  Widget build(BuildContext context) {
    final title = branchName != null
        ? 'Recent commits on $branchName'
        : 'Recent commits';
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      decoration: const BoxDecoration(
        border: Border(
          bottom: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            title,
            style: const TextStyle(
              fontSize: 11,
              fontWeight: FontWeight.w600,
              color: Color(0xE0FFFFFF), // white @ 0.88
            ),
          ),
          Text(
            summary,
            style: const TextStyle(
              fontSize: 11,
              color: Color(0x94FFFFFF), // muted, ~0.58
            ),
          ),
        ],
      ),
    );
  }
}

class _LoadMoreCommitsRow extends ConsumerWidget {
  const _LoadMoreCommitsRow({required this.projectId});

  final String projectId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Padding(
      padding: const EdgeInsets.only(top: 10, bottom: 6),
      child: Center(
        child: _LoadMorePill(
          onPressed: () {
            ref.read(commitPageSizeProvider(projectId).notifier).update(
                  (state) => state + kRecentCommitsPageSize,
                );
            ref.invalidate(recentCommitsProvider(projectId));
          },
        ),
      ),
    );
  }
}

class _LoadMorePill extends StatefulWidget {
  const _LoadMorePill({required this.onPressed});

  final VoidCallback onPressed;

  @override
  State<_LoadMorePill> createState() => _LoadMorePillState();
}

class _LoadMorePillState extends State<_LoadMorePill> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: 'Show 20 more recent commits',
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: Container(
            height: 30,
            padding: const EdgeInsets.symmetric(horizontal: 7),
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: _hover ? AppTokens.overlayHover : Colors.transparent,
              borderRadius: BorderRadius.circular(7),
            ),
            child: const Text(
              'Load more',
              style: TextStyle(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: Color(0xF0FFFFFF), // white @ 0.94
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Per-commit row matching `desktop/src/right_sidebar.rs::branch_commit_row`:
/// chevron + subject (truncated medium) collapsed, plus an
/// expanded panel below with author/time meta and the file list.
/// `isFirst` toggles the optional "Undo last commit" button — left
/// off here pending the titlebar git_actions toolbar bridge.
class _CommitRow extends ConsumerWidget {
  const _CommitRow({
    required this.projectId,
    required this.commit,
    required this.isFirst,
  });

  final String projectId;
  final CommitDto commit;
  final bool isFirst;

  String get _expandKey => '$projectId:${commit.id}';

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final expanded = ref.watch(commitRowExpandedProvider).contains(_expandKey);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _CommitRowHeader(
          subject: commit.subject,
          expanded: expanded,
          onToggle: () =>
              ref.read(commitRowExpandedProvider.notifier).toggle(_expandKey),
        ),
        if (expanded)
          _CommitExpandedPanel(
            projectId: projectId,
            commit: commit,
          ),
      ],
    );
  }
}

class _CommitRowHeader extends StatefulWidget {
  const _CommitRowHeader({
    required this.subject,
    required this.expanded,
    required this.onToggle,
  });

  final String subject;
  final bool expanded;
  final VoidCallback onToggle;

  @override
  State<_CommitRowHeader> createState() => _CommitRowHeaderState();
}

class _CommitRowHeaderState extends State<_CommitRowHeader> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onToggle,
        child: Container(
          margin: const EdgeInsets.symmetric(horizontal: 4),
          constraints: const BoxConstraints(minHeight: 30),
          padding: EdgeInsets.fromLTRB(
            14,
            widget.expanded ? 7 : 5,
            14,
            widget.expanded ? 7 : 5,
          ),
          decoration: BoxDecoration(
            color: _hover ? const Color(0x0AFFFFFF) : Colors.transparent,
            borderRadius: BorderRadius.circular(AppTokens.radiusMd),
          ),
          child: Row(
            children: [
              AppIcon(
                widget.expanded ? 'chevron-down' : 'chevron-right',
                size: 8,
                color: const Color(0x94FFFFFF), // 0.58 white
              ),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  widget.subject,
                  overflow: TextOverflow.ellipsis,
                  maxLines: 1,
                  style: const TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w500,
                    color: Color(0xF0FFFFFF), // 0.94 white
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _CommitExpandedPanel extends ConsumerWidget {
  const _CommitExpandedPanel({
    required this.projectId,
    required this.commit,
  });

  final String projectId;
  final CommitDto commit;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final files = ref.watch(
      commitFileChangesProvider(
        CommitFileChangesKey(projectId: projectId, commitId: commit.id),
      ),
    );
    return Container(
      margin: const EdgeInsets.fromLTRB(12, 0, 12, 8),
      decoration: BoxDecoration(
        color: const Color(0x08FFFFFF), // white @ 0.03
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        border: Border.all(color: AppTokens.divider),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Author + time meta line
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 10, 12, 6),
            child: Row(
              children: [
                Flexible(
                  child: Text(
                    commit.authorName,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: 11,
                      color: Color(0x94FFFFFF), // 0.58
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                const Text(
                  '·',
                  style: TextStyle(
                    fontSize: 11,
                    color: Color(0x94FFFFFF),
                  ),
                ),
                const SizedBox(width: 8),
                Text(
                  commit.authoredRelative,
                  style: const TextStyle(
                    fontSize: 11,
                    color: Color(0x94FFFFFF),
                  ),
                ),
              ],
            ),
          ),
          files.when(
            data: (list) => _CommitFileList(
              projectId: projectId,
              commitId: commit.id,
              files: list,
            ),
            loading: () => const Padding(
              padding: EdgeInsets.fromLTRB(12, 0, 12, 10),
              child: Row(
                children: [
                  SizedBox(
                    width: 12,
                    height: 12,
                    child: ToolbarSpinner(
                      size: 12,
                      color: Color(0x94FFFFFF),
                    ),
                  ),
                  SizedBox(width: 8),
                  Text(
                    'Loading file changes...',
                    style: TextStyle(
                      fontSize: 11,
                      color: Color(0x94FFFFFF),
                    ),
                  ),
                ],
              ),
            ),
            error: (e, _) => const Padding(
              padding: EdgeInsets.fromLTRB(12, 0, 12, 10),
              child: Text(
                "Couldn't load file changes.",
                style: TextStyle(
                  fontSize: 11,
                  color: Color(0x94FFFFFF),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _CommitFileList extends StatelessWidget {
  const _CommitFileList({
    required this.projectId,
    required this.commitId,
    required this.files,
  });

  final String projectId;
  final String commitId;
  final List<BranchCompareFileDto>? files;

  @override
  Widget build(BuildContext context) {
    final list = files;
    if (list == null) {
      return const Padding(
        padding: EdgeInsets.fromLTRB(12, 0, 12, 10),
        child: Text(
          "Couldn't load file changes.",
          style: TextStyle(
            fontSize: 11,
            color: Color(0x94FFFFFF),
          ),
        ),
      );
    }
    if (list.isEmpty) {
      return const Padding(
        padding: EdgeInsets.fromLTRB(12, 0, 12, 10),
        child: Text(
          'No file changes in this commit.',
          style: TextStyle(
            fontSize: 11,
            color: Color(0x94FFFFFF),
          ),
        ),
      );
    }
    final caption = list.length == 1
        ? '1 file changed'
        : '${list.length} files changed';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 0, 12, 2),
          child: Text(
            caption,
            style: const TextStyle(
              fontSize: 10,
              fontWeight: FontWeight.w600,
              color: Color(0x94FFFFFF),
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.only(bottom: 10),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (final file in list)
                _BranchCompareFileRow(file: file),
            ],
          ),
        ),
      ],
    );
  }
}

class _BranchCompareFileRow extends StatelessWidget {
  const _BranchCompareFileRow({required this.file});

  final BranchCompareFileDto file;

  @override
  Widget build(BuildContext context) {
    final fileName = _basename(file.path);
    final parentDir = _parentDir(file.path);
    final status = file.status.isEmpty ? 'M' : file.status;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Row(
                  children: [
                    SizedBox(
                      width: 18,
                      child: Text(
                        status,
                        style: TextStyle(
                          fontSize: 12,
                          fontWeight: FontWeight.w700,
                          color: _statusColor(status),
                        ),
                      ),
                    ),
                    const SizedBox(width: 12),
                    Flexible(
                      child: Text(
                        fileName,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 12,
                          fontWeight: FontWeight.w500,
                          color: Color(0xEBFFFFFF),
                        ),
                      ),
                    ),
                    if (parentDir != null) ...[
                      const SizedBox(width: 6),
                      Flexible(
                        child: Text(
                          parentDir,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(
                            fontSize: 11,
                            color: Color(0x8FFFFFFF), // 0.56
                          ),
                        ),
                      ),
                    ],
                  ],
                ),
              ),
              const SizedBox(width: 12),
              if (file.additions > 0)
                _DiffBadge(value: file.additions, positive: true),
              if (file.deletions > 0) ...[
                const SizedBox(width: 8),
                _DiffBadge(value: file.deletions, positive: false),
              ],
            ],
          ),
          if (file.originalPath != null)
            Padding(
              padding: const EdgeInsets.only(left: 30, top: 2),
              child: Text(
                'Renamed from ${file.originalPath}',
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(
                  fontSize: 11,
                  color: Color(0x8FFFFFFF),
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
      data: (list) => _ChecksBody(list: list),
      loading: () => const EmptyState(text: 'Loading checks...'),
      // GPUI surfaces the error message verbatim — same here so a
      // missing gh CLI / auth issue is actionable from the panel.
      error: (e, _) => EmptyState(text: e.toString()),
    );
  }
}

class _ChecksBody extends StatelessWidget {
  const _ChecksBody({required this.list});

  final List<CheckDto>? list;

  @override
  Widget build(BuildContext context) {
    final l = list;
    if (l == null) {
      return const EmptyState(
        text: 'No pull request exists for this branch.',
      );
    }
    if (l.isEmpty) {
      return const EmptyState(
        text: 'No CI checks found for this pull request.',
      );
    }
    // Sort by bucket priority (Fail, Pending, Pass, Skipping/Cancel)
    // then case-insensitive name. Mirrors GPUI's sort.
    final sorted = [...l]..sort((a, b) {
        final pa = _bucketPriority(a.bucket);
        final pb = _bucketPriority(b.bucket);
        if (pa != pb) return pa.compareTo(pb);
        return a.name.toLowerCase().compareTo(b.name.toLowerCase());
      });
    final passed = sorted.where((c) => c.bucket == CheckBucket.pass).length;
    final failed = sorted.where((c) => c.bucket == CheckBucket.fail).length;
    final pending =
        sorted.where((c) => c.bucket == CheckBucket.pending).length;
    final skipped = sorted
        .where((c) =>
            c.bucket == CheckBucket.skipping || c.bucket == CheckBucket.cancel)
        .length;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _ChecksSummaryBar(
          passed: passed,
          failed: failed,
          pending: pending,
          skipped: skipped,
        ),
        Expanded(
          child: ListView.builder(
            padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 6),
            itemCount: sorted.length,
            itemBuilder: (_, i) => _CheckRow(check: sorted[i]),
          ),
        ),
      ],
    );
  }
}

/// Mirrors GPUI's `check_run_sort_priority`: Fail first, Pending,
/// then Pass, then Skipping/Cancel. Lower number = higher
/// priority.
int _bucketPriority(CheckBucket bucket) => switch (bucket) {
      CheckBucket.fail => 0,
      CheckBucket.pending => 1,
      CheckBucket.pass => 2,
      CheckBucket.skipping || CheckBucket.cancel => 3,
    };

/// HSL-derived sRGB constants, computed once. Matches GPUI's
/// `check_run_visual` colors so the badge glyphs and the summary
/// pills agree.
const Color _bucketColorPass = Color(0xFFA0D9B4); // hsla(138/360, 0.50, 0.74)
const Color _bucketColorFail = Color(0xFFEBA8B0); // hsla(352/360, 0.52, 0.76)
const Color _bucketColorPending = Color(0xFFF5C53D); // hsla(42/360, 0.90, 0.66)
const Color _bucketColorMuted = Color(0xFF9E9E9E); // hsla(0, 0, 0.62) — summary
const Color _bucketIconMuted = Color(0xFF8F8F8F); // hsla(0, 0, 0.56) — row icon

Color _bucketColor(CheckBucket bucket) => switch (bucket) {
      CheckBucket.pass => _bucketColorPass,
      CheckBucket.fail => _bucketColorFail,
      CheckBucket.pending => _bucketColorPending,
      CheckBucket.skipping || CheckBucket.cancel => _bucketIconMuted,
    };

String _bucketIconName(CheckBucket bucket) => switch (bucket) {
      CheckBucket.pass => 'badge-check',
      CheckBucket.fail => 'badge-x',
      CheckBucket.pending => 'badge-clock',
      CheckBucket.skipping || CheckBucket.cancel => 'minus',
    };

class _ChecksSummaryBar extends StatelessWidget {
  const _ChecksSummaryBar({
    required this.passed,
    required this.failed,
    required this.pending,
    required this.skipped,
  });

  final int passed;
  final int failed;
  final int pending;
  final int skipped;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
      decoration: const BoxDecoration(
        border: Border(
          bottom: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: Wrap(
        spacing: 6,
        runSpacing: 6,
        children: [
          if (passed > 0)
            _CheckSummaryBadge(label: '$passed passed', color: _bucketColorPass),
          if (failed > 0)
            _CheckSummaryBadge(label: '$failed failed', color: _bucketColorFail),
          if (pending > 0)
            _CheckSummaryBadge(
              label: '$pending pending',
              color: _bucketColorPending,
            ),
          if (skipped > 0)
            _CheckSummaryBadge(
              label: '$skipped skipped',
              color: _bucketColorMuted,
            ),
        ],
      ),
    );
  }
}

/// `check_runs_summary_badge`: pill, white@0.06 bg, 11px semibold,
/// bucket-coloured text.
class _CheckSummaryBadge extends StatelessWidget {
  const _CheckSummaryBadge({required this.label, required this.color});

  final String label;
  final Color color;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(
        color: const Color(0x0FFFFFFF), // white @ 0.06
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(
        label,
        style: TextStyle(
          fontSize: 11,
          fontWeight: FontWeight.w600,
          color: color,
        ),
      ),
    );
  }
}

/// Per-check row matching `desktop/src/right_sidebar.rs::check_run_row`:
///   px(14) py(0.5) gap(10) rounded_md, hover white@0.04, items_start
///   16px bucket icon — name (12px medium 0.94, truncated)
///                       description (11px 0.58, truncated, optional)
///   right cluster: duration text (11px 0.58, optional) + open-link
///   action button (icons__external-link.svg) when `link` is set.
class _CheckRow extends StatefulWidget {
  const _CheckRow({required this.check});

  final CheckDto check;

  @override
  State<_CheckRow> createState() => _CheckRowState();
}

class _CheckRowState extends State<_CheckRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final check = widget.check;
    final bucketColor = _bucketColor(check.bucket);
    final iconName = _bucketIconName(check.bucket);
    final description = check.description;
    final duration = check.durationText;
    final link = check.link;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 0.5),
        decoration: BoxDecoration(
          color: _hover ? const Color(0x0AFFFFFF) : Colors.transparent,
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: AppIcon(iconName, size: 16, color: bucketColor),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Padding(
                padding: const EdgeInsets.symmetric(vertical: 4),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      check.name,
                      overflow: TextOverflow.ellipsis,
                      maxLines: 1,
                      style: const TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w500,
                        color: Color(0xF0FFFFFF),
                      ),
                    ),
                    if (description != null) ...[
                      const SizedBox(height: 2),
                      Text(
                        description,
                        overflow: TextOverflow.ellipsis,
                        maxLines: 1,
                        style: const TextStyle(
                          fontSize: 11,
                          color: Color(0x94FFFFFF),
                        ),
                      ),
                    ],
                  ],
                ),
              ),
            ),
            if (duration != null || link != null) ...[
              const SizedBox(width: 8),
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 2),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.center,
                  children: [
                    if (duration != null) ...[
                      Text(
                        duration,
                        style: const TextStyle(
                          fontSize: 11,
                          color: Color(0x94FFFFFF),
                        ),
                      ),
                      const SizedBox(width: 8),
                    ],
                    if (link != null)
                      _IconActionButton(
                        icon: 'external-link',
                        tooltip: 'Open this check in GitHub',
                        onPressed: () async {
                          final uri = Uri.tryParse(link);
                          if (uri == null) return;
                          await launchUrl(
                            uri,
                            mode: LaunchMode.externalApplication,
                          );
                        },
                      ),
                  ],
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

