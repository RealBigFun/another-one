// Pixel-precise port of `desktop/src/titlebar.rs::titlebar_git_actions_button`
// + `titlebar_git_actions_overlay`. Split-button shape: primary
// half fires the dynamic primary action (Commit when there are
// unstaged changes, Push when ahead, Pull when behind, Fetch
// otherwise); chevron half toggles the dropdown.
//
// Lives as `part of desktop_titlebar.dart` so the chrome shell
// stays the single import surface.

part of 'desktop_titlebar.dart';

/// Each menu row's id maps 1:1 to the bridge action_id strings
/// recognised by `LocalSession::run_toolbar_git_action`.
/// `undo-last-commit` is also a valid wire id but it doesn't have
/// a dropdown row — it lives on the first-commit row in the
/// right-sidebar Commits pane.
enum _GitActionId {
  commit('commit'),
  commitAndPush('commit-and-push'),
  fetch('fetch'),
  pull('pull'),
  push('push'),
  forcePush('force-push'),
  createPr('create-pr'),
  createDraftPr('create-draft-pr');

  const _GitActionId(this.wireId);
  final String wireId;
}

/// Mirrors GPUI's `resolve_active_git_action_presentation`: maps
/// each running action to the in-progress label + danger flag.
({String label, bool danger})? _activeActionPresentation(String? wireId) {
  return switch (wireId) {
    'commit' => (label: 'Committing...', danger: false),
    'commit-and-push' => (label: 'Committing & Pushing...', danger: false),
    'undo-last-commit' => (label: 'Undoing Last Commit...', danger: true),
    'fetch' => (label: 'Fetching...', danger: false),
    'pull' => (label: 'Pulling...', danger: false),
    'push' => (label: 'Pushing...', danger: false),
    'force-push' => (label: 'Force Pushing...', danger: true),
    'create-pr' => (label: 'Creating PR...', danger: false),
    'create-draft-pr' => (label: 'Creating Draft PR...', danger: false),
    _ => null,
  };
}

class _PrimaryAction {
  const _PrimaryAction({
    required this.action,
    required this.label,
    required this.icon,
  });

  final _GitActionId action;
  final String label;
  final String icon;
}

/// Mirrors GPUI's `resolve_idle_primary_git_action`: pick whichever
/// action the user is most likely to want next. When changes
/// exist, the user's repo-level commit-action preference picks
/// between Commit and CommitAndPush; otherwise ahead/behind
/// counts pick Push/Pull; Fetch is the always-safe fallback.
///
/// `commitPreference` mirrors `repo_default_commit_action`'s
/// optional return — `null` falls back to plain Commit.
_PrimaryAction _computePrimaryAction({
  required bool hasChanges,
  required int aheadCount,
  required int behindCount,
  required String? commitPreference,
}) {
  if (hasChanges) {
    if (commitPreference == 'commit-and-push') {
      return const _PrimaryAction(
        action: _GitActionId.commitAndPush,
        label: 'Commit & Push',
        icon: 'cloud-upload',
      );
    }
    return const _PrimaryAction(
      action: _GitActionId.commit,
      label: 'Commit',
      icon: 'git-commit',
    );
  }
  if (aheadCount > 0) {
    return _PrimaryAction(
      action: _GitActionId.push,
      label: _countLabel('Push', aheadCount),
      icon: 'cloud-upload',
    );
  }
  if (behindCount > 0) {
    return _PrimaryAction(
      action: _GitActionId.pull,
      label: _countLabel('Pull', behindCount),
      icon: 'git-pull',
    );
  }
  return const _PrimaryAction(
    action: _GitActionId.fetch,
    label: 'Fetch',
    icon: 'tool-download',
  );
}

String _countLabel(String base, int n) => n > 0 ? '$base ($n)' : base;

class _GitActionsButton extends ConsumerWidget {
  const _GitActionsButton();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projectId = ref.watch(activeProjectIdProvider);
    if (projectId == null) return const SizedBox.shrink();
    final files = ref.watch(changedFilesProvider(projectId)).valueOrNull;
    final gitState = ref.watch(activeGitStateProvider(projectId)).valueOrNull;
    final commitPreference = ref
        .watch(repoDefaultCommitActionProvider(projectId))
        .valueOrNull;
    final activeAction = ref.watch(activeGitActionProvider(projectId));
    final pr = ref.watch(pullRequestStatusProvider(projectId));
    final hasChanges = (files?.isNotEmpty) ?? false;
    final aheadCount = gitState?.aheadCount ?? 0;
    final behindCount = gitState?.behindCount ?? 0;
    final primary = _computePrimaryAction(
      hasChanges: hasChanges,
      aheadCount: aheadCount,
      behindCount: behindCount,
      commitPreference: commitPreference,
    );

    final running = activeAction != null;
    final menuOpen =
        ref.watch(_activeTitlebarDropdownProvider) ==
        _TitlebarDropdown.gitActions;
    final presentation = _activeActionPresentation(activeAction);
    final danger = presentation?.danger ?? false;
    final label = presentation?.label ?? primary.label;
    final iconColor = danger ? _dangerText : const Color(0xEBFFFFFF); // 0.92
    final textColor = danger ? _dangerText : const Color(0xDBFFFFFF); // 0.86
    final hasExistingPr = pr.valueOrNull != null;
    final lookupChecked = pr.hasValue;
    final canCreatePr = !running && lookupChecked && !hasExistingPr;

    return _TitlebarSplitButton(
      buttonWidth: _buttonW,
      menuWidth: _menuW,
      menuOpen: menuOpen,
      primaryEnabled: !running,
      chevronEnabled: !running,
      onDismissMenu: () => _dismissTitlebarDropdowns(ref),
      onPrimaryTap: () {
        unawaited(_run(context, ref, projectId, primary.action));
      },
      onChevronTap: () {
        final opening = !menuOpen;
        _toggleTitlebarDropdown(ref, _TitlebarDropdown.gitActions);
        if (opening) {
          // Refresh PR lookup on dropdown open — mirrors
          // GPUI's refresh_active_project_pull_request_lookup.
          ref.invalidate(pullRequestStatusProvider(projectId));
        }
      },
      primaryBuilder: (context) => Row(
        children: [
          if (running)
            ToolbarSpinner(size: 12, color: iconColor)
          else
            AppIcon(primary.icon, size: 14, color: iconColor),
          const SizedBox(width: 6),
          Expanded(
            child: Text(
              label,
              overflow: TextOverflow.ellipsis,
              maxLines: 1,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w500,
                color: textColor,
              ),
            ),
          ),
        ],
      ),
      chevronBuilder: (context) => const AppIcon(
        'chevron-down',
        size: 11,
        color: Color(0xADFFFFFF), // white @ 0.68
      ),
      menuBuilder: (context) => Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _GitActionRow(
            icon: 'git-commit',
            label: 'Commit',
            tooltip: 'Commit changes, staging all files first if needed',
            enabled: hasChanges && !running,
            onTap: () {
              unawaited(_run(context, ref, projectId, _GitActionId.commit));
            },
          ),
          _GitActionRow(
            icon: 'cloud-upload',
            label: 'Commit & Push',
            tooltip:
                'Commit changes and push, staging all files '
                'first if needed',
            enabled: hasChanges && !running,
            onTap: () {
              unawaited(
                _run(context, ref, projectId, _GitActionId.commitAndPush),
              );
            },
          ),
          const _MenuDivider(),
          _GitActionRow(
            icon: 'tool-download',
            label: 'Fetch',
            tooltip: 'Fetch remote updates without changing the local checkout',
            enabled: !running,
            onTap: () {
              unawaited(_run(context, ref, projectId, _GitActionId.fetch));
            },
          ),
          _GitActionRow(
            icon: 'git-pull',
            label: _countLabel('Pull', behindCount),
            tooltip: 'Pull remote updates with fast-forward only',
            enabled: !running,
            onTap: () {
              unawaited(_run(context, ref, projectId, _GitActionId.pull));
            },
          ),
          _GitActionRow(
            icon: 'cloud-upload',
            label: _countLabel('Push', aheadCount),
            tooltip: 'Push the current checked-out branch to its remote',
            enabled: !running,
            onTap: () {
              unawaited(_run(context, ref, projectId, _GitActionId.push));
            },
          ),
          _GitActionRow(
            icon: 'cloud-upload',
            label: 'Force Push',
            tooltip:
                'Force-push with lease to overwrite the remote '
                'branch if needed',
            enabled: !running,
            danger: true,
            onTap: () {
              unawaited(_run(context, ref, projectId, _GitActionId.forcePush));
            },
          ),
          const _MenuDivider(),
          _GitActionRow(
            icon: 'github',
            label: 'Create PR',
            tooltip: 'Create a pull request for the current branch',
            enabled: canCreatePr,
            onTap: () {
              unawaited(_run(context, ref, projectId, _GitActionId.createPr));
            },
          ),
          _GitActionRow(
            icon: 'github',
            label: 'Draft PR',
            tooltip: 'Create a draft pull request for the current branch',
            enabled: canCreatePr,
            onTap: () {
              unawaited(
                _run(context, ref, projectId, _GitActionId.createDraftPr),
              );
            },
          ),
          const _MenuDivider(),
          _GitActionRow(
            icon: 'git-branch',
            label: 'Create Branch',
            tooltip: 'Create a branch in this task or a new worktree',
            enabled: !running,
            onTap: () => _openCreateBranch(context, projectId),
          ),
        ],
      ),
    );
  }

  // Dimensions from desktop/src/titlebar.rs constants.
  static const double _buttonW = 156;
  static const double _menuW = 220;

  static const Color _dangerText = Color(0xFFEB7B7B);
  void _openCreateBranch(BuildContext context, String projectId) {
    showCreateBranchModal(context: context, projectId: projectId);
  }

  Future<void> _run(
    BuildContext context,
    WidgetRef ref,
    String projectId,
    _GitActionId action,
  ) async {
    _dismissTitlebarDropdowns(ref);
    final notifier = ref.read(activeGitActionProvider(projectId).notifier);
    notifier.start(action.wireId);
    final connection = ref.read(localConnectionProvider);
    final messenger = ScaffoldMessenger.maybeOf(context);
    try {
      final outcome = await connection.runToolbarGitAction(
        projectId: projectId,
        actionId: action.wireId,
      );
      if (context.mounted) {
        messenger?.showSnackBar(
          SnackBar(
            content: Text(outcome.toastMessage),
            backgroundColor: outcome.warning
                ? AppTokens.warningBg
                : AppTokens.successBg,
          ),
        );
      }
      if (outcome.refreshGitState) {
        invalidateGitRefreshState(ref, projectId);
      }
    } catch (e) {
      if (context.mounted) {
        messenger?.showSnackBar(
          SnackBar(
            content: Text(e.toString()),
            backgroundColor: AppTokens.errorBg,
          ),
        );
      }
    } finally {
      notifier.clear();
    }
  }
}

/// Single dropdown row — h(34) px(12) gap(8). Disabled rows render
/// at 0.55 opacity, no hover bg, no click. Danger rows hover with
/// the danger tint instead of plain white@0.06.
class _GitActionRow extends StatefulWidget {
  const _GitActionRow({
    required this.icon,
    required this.label,
    required this.tooltip,
    required this.enabled,
    required this.onTap,
    this.danger = false,
  });

  final String icon;
  final String label;
  final String tooltip;
  final bool enabled;
  final VoidCallback onTap;
  final bool danger;

  @override
  State<_GitActionRow> createState() => _GitActionRowState();
}

class _GitActionRowState extends State<_GitActionRow> {
  bool _hover = false;

  // Danger col + danger hover from desktop/src/titlebar.rs:
  //   hsla(0, 0.78, 0.72) ≈ #EB7B7B text
  //   hsla(0, 0.45, 0.34, 0.26) ≈ rgba(126, 48, 48, 0.26) hover bg
  static const Color _dangerText = Color(0xFFEB7B7B);
  static const Color _dangerHover = Color(0x427E3030);

  @override
  Widget build(BuildContext context) {
    final hoverBg = widget.danger ? _dangerHover : AppTokens.overlayHover;
    final textColor = widget.danger
        ? _dangerText
        : const Color(0xEBFFFFFF); // 0.92
    final iconColor = textColor;
    final row = Opacity(
      opacity: widget.enabled ? 1.0 : 0.55,
      child: Container(
        height: 34,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        color: widget.enabled && _hover ? hoverBg : Colors.transparent,
        child: Row(
          children: [
            AppIcon(widget.icon, size: 14, color: iconColor),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                widget.label,
                overflow: TextOverflow.ellipsis,
                maxLines: 1,
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w500,
                  color: textColor,
                ),
              ),
            ),
          ],
        ),
      ),
    );
    if (!widget.enabled) return row;
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onTap,
          child: row,
        ),
      ),
    );
  }
}

class _MenuDivider extends StatelessWidget {
  const _MenuDivider();

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 1,
      margin: const EdgeInsets.symmetric(horizontal: 8),
      color: AppTokens.border,
    );
  }
}
