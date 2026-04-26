// Task-row widgets (and the rename-mode editor) — split out of
// `desktop_sidebar.dart` so the shell file stays focused on
// column layout. Same `part of` arrangement as `project_row.dart`.

part of 'desktop_sidebar.dart';

class _TaskRow extends ConsumerWidget {
  const _TaskRow({required this.task, required this.projectId});

  final TaskSummary task;
  final String projectId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selection = ref.watch(selectedTabProvider);
    final isActive = selection != null &&
        selection.sectionId == task.sectionId;
    return Padding(
      padding: const EdgeInsets.only(left: AppTokens.space3),
      child: _TaskRowBody(
        task: task,
        projectId: projectId,
        isActive: isActive,
      ),
    );
  }
}

class _TaskRowBody extends ConsumerStatefulWidget {
  const _TaskRowBody({
    required this.task,
    required this.projectId,
    required this.isActive,
  });

  final TaskSummary task;
  final String projectId;
  final bool isActive;

  @override
  ConsumerState<_TaskRowBody> createState() => _TaskRowBodyState();
}

class _TaskRowBodyState extends ConsumerState<_TaskRowBody> {
  bool _hovered = false;

  void _selectTask() {
    final task = widget.task;
    if (task.activeTabId.isEmpty) return;
    ref.read(selectedTabProvider.notifier).set(
      TabSelection(
        sectionId: task.sectionId,
        tabId: task.activeTabId,
      ),
    );
  }

  Future<void> _showTaskMenu(Offset globalPosition) async {
    final overlay =
        Overlay.of(context).context.findRenderObject() as RenderBox;
    final value = await showMenu<String>(
      context: context,
      color: AppTokens.cardBg,
      position: RelativeRect.fromLTRB(
        globalPosition.dx,
        globalPosition.dy,
        overlay.size.width - globalPosition.dx,
        overlay.size.height - globalPosition.dy,
      ),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        side: const BorderSide(color: AppTokens.border),
      ),
      constraints: const BoxConstraints(
        minWidth: AppTokens.taskMenuWidth,
        maxWidth: AppTokens.taskMenuWidth,
      ),
      items: [
        _menuItem(
          value: 'pin',
          label: widget.task.pinned ? 'Unpin' : 'Pin',
          icon: 'pin-off',
        ),
        _menuItem(
          value: 'new-task',
          label: 'New task from current branch',
          icon: 'git-worktree',
        ),
        _menuItem(value: 'rename', label: 'Rename', icon: 'edit'),
        const PopupMenuDivider(height: 1),
        _menuItem(
          value: 'delete',
          label: 'Delete',
          icon: 'trash',
          danger: true,
        ),
      ],
    );
    if (!mounted || value == null) return;
    final transport = ref.read(localConnectionProvider);
    switch (value) {
      case 'pin':
        await runMutation(
          context,
          () => transport.setTaskPinned(
            widget.task.id,
            !widget.task.pinned,
          ),
          errorPrefix: 'Failed to toggle pin',
        );
      case 'delete':
        await _confirmDelete();
      case 'rename':
        ref.read(renameTargetTaskIdProvider.notifier).state = widget.task.id;
      case 'new-task':
        if (!mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('New-task modal is not yet ported'),
            duration: Duration(seconds: 2),
          ),
        );
    }
  }

  Future<void> _commitRename(String newName) async {
    ref.read(renameTargetTaskIdProvider.notifier).state = null;
    final trimmed = newName.trim();
    if (trimmed.isEmpty || trimmed == widget.task.name) return;
    final transport = ref.read(localConnectionProvider);
    await runMutation(
      context,
      () => transport.renameTask(widget.task.id, trimmed),
      errorPrefix: 'Failed to rename task',
    );
  }

  void _cancelRename() {
    ref.read(renameTargetTaskIdProvider.notifier).state = null;
  }

  Future<void> _confirmDelete() async {
    final task = widget.task;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: AppTokens.cardBg,
        title: const Text(
          'Delete task?',
          style: TextStyle(color: AppTokens.textPrimary),
        ),
        content: Text(
          'Delete "${task.name}" from AnotherOne. The on-disk worktree '
          'branch is left untouched, but the task and its terminal '
          'history disappear from the sidebar.',
          style: const TextStyle(color: AppTokens.textSecondary),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            style: FilledButton.styleFrom(
              backgroundColor: AppTokens.dangerBg,
              foregroundColor: AppTokens.textPrimary,
            ),
            onPressed: () => Navigator.of(ctx).pop(true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    final transport = ref.read(localConnectionProvider);
    final removed = await runMutation(
      context,
      () => transport.removeTask(widget.projectId, task.id),
      errorPrefix: 'Failed to delete task',
    );
    if (removed != null) {
      // If the deleted task was the currently-selected one, clear
      // selection so the main pane drops back to the welcome state.
      final selection = ref.read(selectedTabProvider);
      if (selection?.sectionId == task.sectionId) {
        ref.read(selectedTabProvider.notifier).clear();
      }
    }
  }

  PopupMenuItem<String> _menuItem({
    required String value,
    required String label,
    required String icon,
    bool danger = false,
  }) {
    final color = danger
        ? const Color(0xFFE74C5E)
        : AppTokens.textPrimary;
    return PopupMenuItem<String>(
      value: value,
      height: 38,
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.space6),
      child: Row(
        children: [
          AppIcon(icon, size: 15, color: color),
          const SizedBox(width: AppTokens.space4),
          Text(
            label,
            style: TextStyle(
              fontSize: AppTokens.fontBodyLg,
              fontWeight: FontWeight.w500,
              color: color,
            ),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final task = widget.task;
    final isActive = widget.isActive;
    final taskName =
        task.tabs.firstWhere(
          (t) => t.id == task.activeTabId,
          orElse: () => task.tabs.isNotEmpty
              ? task.tabs.first
              : const TabSummary(
                  id: '',
                  title: '',
                  running: false,
                  pinned: false,
                ),
        ).fixedTitle ?? task.name;
    final subtitle = _buildSubtitle(task);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: _selectTask,
        onSecondaryTapDown: (details) => _showTaskMenu(details.globalPosition),
        child: Container(
          // GPUI's `BRANCH_ROW_H` is 44px, but Flutter's text
          // line-heights for the title (13px) + subtitle (10px) +
          // 2px gap come in just over the 32px content budget
          // when both lines are present. Use a min-constraint
          // instead of a hard height so a row with the diff stats
          // and a long subtitle can grow by the missing few pixels
          // — keeps GPUI's normal-case visual at 44px while
          // avoiding the runtime overflow assertion.
          constraints: const BoxConstraints(
            minHeight: AppTokens.taskRowHeight,
          ),
          margin: const EdgeInsets.symmetric(
            vertical: AppTokens.sidebarListGap / 2,
          ),
          padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.space4,
            vertical: 6,
          ),
          decoration: BoxDecoration(
            color: isActive
                ? AppTokens.rowActiveBg
                : (_hovered
                    ? AppTokens.overlayHover
                    : Colors.transparent),
            borderRadius: BorderRadius.circular(AppTokens.radiusSm),
            border: Border.all(
              color:
                  isActive ? AppTokens.rowActiveBorder : Colors.transparent,
              width: 1,
            ),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Row(
                children: [
                  Expanded(
                    child: ref.watch(renameTargetTaskIdProvider) == task.id
                        ? _TaskRenameField(
                            task: task,
                            onCommit: _commitRename,
                            onCancel: _cancelRename,
                          )
                        : GestureDetector(
                            // Double-click to enter rename mode —
                            // matches GPUI's left_sidebar.rs:
                            // "Double-click: enter rename mode".
                            onDoubleTap: () {
                              ref
                                  .read(renameTargetTaskIdProvider.notifier)
                                  .state = task.id;
                            },
                            child: Text(
                              taskName,
                              overflow: TextOverflow.ellipsis,
                              style: const TextStyle(
                                fontSize: AppTokens.fontBodyLg,
                                fontWeight: FontWeight.w500,
                                color: AppTokens.textSecondary,
                              ),
                            ),
                          ),
                  ),
                  const SizedBox(width: 4),
                  // Worktree marker — every task in AnotherOne is a
                  // worktree today, so this glyph is always shown
                  // (mirrors GPUI's git-worktree icon).
                  const AppIcon(
                    'git-worktree',
                    size: 11,
                    color: AppTokens.textPlaceholder,
                  ),
                  if (task.pinned) ...[
                    const SizedBox(width: 4),
                    const AppIcon(
                      'pin-off',
                      size: 11,
                      color: AppTokens.accent,
                    ),
                  ],
                ],
              ),
              if (subtitle != null || _hasDiff(task))
                Padding(
                  padding: const EdgeInsets.only(top: 2),
                  child: Row(
                    children: [
                      if (subtitle != null)
                        Flexible(
                          child: Text(
                            subtitle,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(
                              fontSize: AppTokens.fontCaption,
                              color: AppTokens.textPlaceholder,
                            ),
                          ),
                        ),
                      if (subtitle != null && _hasDiff(task))
                        const Padding(
                          padding: EdgeInsets.symmetric(horizontal: 4),
                          child: Text(
                            '•',
                            style: TextStyle(
                              fontSize: AppTokens.fontCaption,
                              color: AppTokens.textPlaceholder,
                            ),
                          ),
                        ),
                      if (_hasDiff(task)) ...[
                        Text(
                          '+${task.linesAdded}',
                          style: const TextStyle(
                            fontSize: AppTokens.fontCaption,
                            fontWeight: FontWeight.w600,
                            color: AppTokens.diffAdded,
                          ),
                        ),
                        const SizedBox(width: 4),
                        Text(
                          '-${task.linesRemoved}',
                          style: const TextStyle(
                            fontSize: AppTokens.fontCaption,
                            fontWeight: FontWeight.w600,
                            color: AppTokens.diffRemoved,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  /// Compose the row subtitle from raw bridge fields. GPUI's
  /// `branch_row` does the same join (`branch.name (when !=
  /// task.name) • last_commit_relative`, drop empty segments) —
  /// keeping it here means the bridge ships raw data and the UI
  /// formats per-render, no interop tax for sub-second display
  /// updates.
  String? _buildSubtitle(TaskSummary task) {
    final parts = <String>[];
    if (task.branchName.isNotEmpty && task.branchName != task.name) {
      parts.add(task.branchName);
    }
    if (task.lastCommitRelative.isNotEmpty) {
      parts.add(task.lastCommitRelative);
    }
    if (parts.isEmpty) return null;
    return parts.join(' • ');
  }

  /// True when at least one of the diff-stat counters is non-zero —
  /// matches GPUI's `has_diff` gate. Branch refresh hasn't run, or
  /// the working tree is clean against the merge base, when both
  /// are zero; in that case we omit the +/- pair entirely so the
  /// subtitle row doesn't carry empty noise.
  bool _hasDiff(TaskSummary task) =>
      task.linesAdded > 0 || task.linesRemoved > 0;
}

/// Inline editor swapped in for the task name when the row is the
/// current `renameTargetTaskIdProvider` target. Auto-focuses,
/// pre-selects the existing name (so typing replaces it), commits
/// on Enter or blur, cancels on Esc — matches GPUI's task rename
/// editor in `left_sidebar.rs`.
class _TaskRenameField extends StatefulWidget {
  const _TaskRenameField({
    required this.task,
    required this.onCommit,
    required this.onCancel,
  });

  final TaskSummary task;
  final ValueChanged<String> onCommit;
  final VoidCallback onCancel;

  @override
  State<_TaskRenameField> createState() => _TaskRenameFieldState();
}

class _TaskRenameFieldState extends State<_TaskRenameField> {
  late final TextEditingController _controller;
  late final FocusNode _focus;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.task.name);
    _focus = FocusNode()..addListener(_onFocusChanged);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      _focus.requestFocus();
      // Select all so typing replaces the existing name in one go,
      // matching GPUI's rename-on-double-click behaviour.
      _controller.selection = TextSelection(
        baseOffset: 0,
        extentOffset: _controller.text.length,
      );
    });
  }

  @override
  void dispose() {
    _focus.removeListener(_onFocusChanged);
    _focus.dispose();
    _controller.dispose();
    super.dispose();
  }

  void _onFocusChanged() {
    if (!_focus.hasFocus && mounted) {
      // Blur commits — matches the GPUI editor.
      widget.onCommit(_controller.text);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Shortcuts(
      shortcuts: const {
        SingleActivator(LogicalKeyboardKey.escape): _CancelIntent(),
      },
      child: Actions(
        actions: <Type, Action<Intent>>{
          _CancelIntent: CallbackAction<_CancelIntent>(
            onInvoke: (_) {
              widget.onCancel();
              return null;
            },
          ),
        },
        child: TextField(
          controller: _controller,
          focusNode: _focus,
          autocorrect: false,
          enableSuggestions: false,
          smartDashesType: SmartDashesType.disabled,
          smartQuotesType: SmartQuotesType.disabled,
          style: const TextStyle(
            fontSize: AppTokens.fontBodyLg,
            fontWeight: FontWeight.w500,
            color: AppTokens.textPrimary,
          ),
          decoration: InputDecoration(
            isDense: true,
            contentPadding: const EdgeInsets.symmetric(
              horizontal: AppTokens.space2,
              vertical: 2,
            ),
            filled: true,
            fillColor: const Color(0x24000000),
            border: OutlineInputBorder(
              borderRadius: BorderRadius.circular(AppTokens.radiusSm),
              borderSide: BorderSide(color: AppTokens.focusRing),
            ),
            enabledBorder: OutlineInputBorder(
              borderRadius: BorderRadius.circular(AppTokens.radiusSm),
              borderSide: BorderSide(color: AppTokens.focusRing),
            ),
            focusedBorder: OutlineInputBorder(
              borderRadius: BorderRadius.circular(AppTokens.radiusSm),
              borderSide: BorderSide(color: AppTokens.focusRing, width: 1.5),
            ),
          ),
          onSubmitted: widget.onCommit,
        ),
      ),
    );
  }
}

class _CancelIntent extends Intent {
  const _CancelIntent();
}
