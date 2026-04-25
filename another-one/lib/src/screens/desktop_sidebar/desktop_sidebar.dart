// Left sidebar for the desktop shell — visual-parity port of
// `desktop/src/left_sidebar.rs`.
//
// Layout (top to bottom):
//   - sidebar-toggle row (toggles the sidebar — currently always
//     visible since the shell doesn't yet support hiding it)
//   - "PROJECTS" small-caps header
//   - project list (each row: avatar | name | chevron | ellipsis |
//     github | plus, then expanded task children)
//   - footer with settings gear + add-project button
//
// Functional verbs not yet on the bridge (rename, pin, new-task,
// delete-task) surface as menu items but show a "Not yet
// implemented" snackbar when invoked. This keeps the visual port
// shippable while the verbs are wired in subsequent commits.

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../rust/api/iroh_client.dart';
import '../../state/local_connection_provider.dart';
import '../../state/rename_target_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';

class DesktopSidebar extends ConsumerWidget {
  const DesktopSidebar({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projects = ref.watch(desktopProjectsProvider);
    return Container(
      width: AppTokens.sidebarWidth,
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(
          right: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: Column(
        children: [
          const _SidebarHeader(),
          Expanded(
            child: projects.when(
              data: _ProjectList.new,
              loading: () => const Center(child: CircularProgressIndicator()),
              error: (e, _) =>
                  _SidebarMessage(text: 'Project list error: $e'),
            ),
          ),
          const _SidebarFooter(),
        ],
      ),
    );
  }
}

class _SidebarHeader extends StatelessWidget {
  const _SidebarHeader();

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.fromLTRB(
        AppTokens.space4,
        AppTokens.space3,
        AppTokens.space4,
        AppTokens.space2,
      ),
      alignment: Alignment.centerLeft,
      child: const Text(
        'PROJECTS',
        textAlign: TextAlign.left,
        style: TextStyle(
          fontSize: AppTokens.fontCaption,
          fontWeight: FontWeight.w600,
          color: AppTokens.textPlaceholder,
          letterSpacing: 0.6,
        ),
      ),
    );
  }
}

class _SidebarFooter extends ConsumerWidget {
  const _SidebarFooter();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Container(
      height: 44,
      decoration: const BoxDecoration(
        border: Border(
          top: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space2,
        vertical: AppTokens.space1,
      ),
      child: Row(
        children: [
          _FooterIconButton(
            tooltip: 'Settings',
            icon: 'settings',
            onPressed: () {
              // Settings page port comes in a later phase; the
              // GPUI version opens an in-app sheet.
              ScaffoldMessenger.of(context).showSnackBar(
                const SnackBar(
                  content: Text('Settings page is not yet ported'),
                  duration: Duration(seconds: 2),
                ),
              );
            },
          ),
          const SizedBox(width: AppTokens.space1),
          _FooterIconButton(
            tooltip: 'Add project',
            icon: 'folder-plus',
            onPressed: () => _addProject(context, ref),
          ),
        ],
      ),
    );
  }

  Future<void> _addProject(BuildContext context, WidgetRef ref) async {
    final selectedPath = await FilePicker.platform.getDirectoryPath(
      dialogTitle: 'Add Project Folder',
    );
    if (selectedPath == null || selectedPath.isEmpty) return;
    final transport = ref.read(localConnectionProvider);
    try {
      final inserted = await transport.addProject(selectedPath);
      if (!context.mounted) return;
      if (!inserted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Project already added at that path'),
            duration: Duration(seconds: 3),
          ),
        );
      }
    } catch (e) {
      if (!context.mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Failed to add project: $e'),
          backgroundColor: AppTokens.errorBg,
        ),
      );
    }
  }
}

class _FooterIconButton extends StatefulWidget {
  const _FooterIconButton({
    required this.tooltip,
    required this.icon,
    required this.onPressed,
  });

  final String tooltip;
  final String icon;
  final VoidCallback onPressed;

  @override
  State<_FooterIconButton> createState() => _FooterIconButtonState();
}

class _FooterIconButtonState extends State<_FooterIconButton> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: Container(
            width: 28,
            height: 28,
            decoration: BoxDecoration(
              color: _hovered ? AppTokens.overlayHover : Colors.transparent,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            ),
            alignment: Alignment.center,
            child: AppIcon(
              widget.icon,
              size: 15,
              color: AppTokens.textMuted,
            ),
          ),
        ),
      ),
    );
  }
}

class _ProjectList extends StatelessWidget {
  const _ProjectList(this.projects);

  final List<ProjectSummary> projects;

  @override
  Widget build(BuildContext context) {
    if (projects.isEmpty) {
      return const _SidebarMessage(
        text: 'No projects yet.\nUse the + at the bottom to add one.',
      );
    }
    return ListView.builder(
      padding: const EdgeInsets.fromLTRB(
        AppTokens.space1,
        AppTokens.sidebarListTopPad,
        AppTokens.space1,
        AppTokens.space5,
      ),
      itemCount: projects.length,
      itemBuilder: (_, i) => _ProjectRow(projects[i]),
    );
  }
}

class _SidebarMessage extends StatelessWidget {
  const _SidebarMessage({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(AppTokens.space7),
      child: Text(
        text,
        style: const TextStyle(
          fontSize: AppTokens.fontBody,
          color: AppTokens.textMuted,
        ),
      ),
    );
  }
}

class _ProjectRow extends ConsumerStatefulWidget {
  const _ProjectRow(this.project);

  final ProjectSummary project;

  @override
  ConsumerState<_ProjectRow> createState() => _ProjectRowState();
}

class _ProjectRowState extends ConsumerState<_ProjectRow> {
  bool _expanded = true;
  bool _rowHovered = false;

  @override
  Widget build(BuildContext context) {
    final project = widget.project;
    final selection = ref.watch(selectedTabProvider);
    final isActive = selection != null &&
        project.tasks.any((t) => t.sectionId == selection.sectionId);
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space1,
        vertical: AppTokens.sidebarListGap / 2,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          MouseRegion(
            cursor: SystemMouseCursors.click,
            onEnter: (_) => setState(() => _rowHovered = true),
            onExit: (_) => setState(() => _rowHovered = false),
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: () => setState(() => _expanded = !_expanded),
              onSecondaryTapDown: (details) =>
                  _showProjectMenu(details.globalPosition),
              child: Container(
                height: AppTokens.projectRowHeight,
                padding: const EdgeInsets.symmetric(
                  horizontal: AppTokens.space4,
                  vertical: AppTokens.space1 + 1,
                ),
                decoration: BoxDecoration(
                  color: isActive
                      ? AppTokens.rowActiveBg
                      : (_rowHovered
                          ? AppTokens.overlayHover
                          : Colors.transparent),
                  borderRadius: BorderRadius.circular(AppTokens.radiusSm),
                  border: Border.all(
                    color: isActive
                        ? AppTokens.rowActiveBorder
                        : Colors.transparent,
                    width: 1,
                  ),
                ),
                child: Row(
                  children: [
                    _ProjectAvatar(project: project),
                    const SizedBox(width: AppTokens.space3),
                    Expanded(
                      child: Text(
                        project.name,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: AppTokens.fontBodyLg,
                          fontWeight: FontWeight.w500,
                          color: AppTokens.textPrimary,
                        ),
                      ),
                    ),
                    const SizedBox(width: AppTokens.space2),
                    AppIcon(
                      _expanded ? 'chevron-down' : 'chevron-right',
                      size: 12,
                      color: AppTokens.chevron,
                    ),
                    const SizedBox(width: AppTokens.space2),
                    _RowIconButton(
                      icon: 'ellipsis',
                      tooltip: 'More',
                      onPressed: () =>
                          _showProjectMenu(_globalCenterOf(context)),
                    ),
                    // GitHub link button (the GPUI sidebar shows a
                    // GitHub glyph for projects with a remote on
                    // github.com). The bridge doesn't yet expose
                    // GitHub-association on `ProjectSummary` so this
                    // renders as a placeholder, hidden by default
                    // and revealed on row hover, matching GPUI's
                    // "invisible until hover unless link exists".
                    if (_rowHovered)
                      _RowIconButton(
                        icon: 'github',
                        tooltip: 'Open on GitHub',
                        onPressed: () {
                          ScaffoldMessenger.of(context).showSnackBar(
                            const SnackBar(
                              content:
                                  Text('GitHub link is not yet wired'),
                              duration: Duration(seconds: 2),
                            ),
                          );
                        },
                      ),
                    _RowIconButton(
                      icon: 'plus',
                      tooltip: 'New task',
                      onPressed: () {
                        ScaffoldMessenger.of(context).showSnackBar(
                          const SnackBar(
                            content:
                                Text('New-task modal is not yet ported'),
                            duration: Duration(seconds: 2),
                          ),
                        );
                      },
                    ),
                  ],
                ),
              ),
            ),
          ),
          if (_expanded && project.tasks.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: AppTokens.sidebarListGap),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (final task in _orderedTasks(project.tasks))
                    _TaskRow(task: task, projectId: project.id),
                ],
              ),
            ),
        ],
      ),
    );
  }

  /// Pinned tasks float to the top, mirroring the GPUI sidebar's
  /// `child_entries.sort_by_key(|e| !e.is_pinned)` behaviour.
  List<TaskSummary> _orderedTasks(List<TaskSummary> tasks) {
    final pinned = <TaskSummary>[];
    final rest = <TaskSummary>[];
    for (final task in tasks) {
      (task.pinned ? pinned : rest).add(task);
    }
    return [...pinned, ...rest];
  }

  Future<void> _showProjectMenu(Offset globalPosition) async {
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
        minWidth: AppTokens.projectMenuWidth,
        maxWidth: AppTokens.projectMenuWidth,
      ),
      items: [
        const PopupMenuItem<String>(
          enabled: false,
          height: 28,
          padding: EdgeInsets.symmetric(horizontal: AppTokens.space5),
          child: Text(
            'Sort tasks by',
            style: TextStyle(
              fontSize: AppTokens.fontCaption,
              fontWeight: FontWeight.w600,
              color: AppTokens.textPlaceholder,
              letterSpacing: 0.6,
            ),
          ),
        ),
        _menuRadioItem(value: 'sort-recent', label: 'Recent activity', selected: true),
        _menuRadioItem(value: 'sort-most', label: 'Most activity', selected: false),
        _menuRadioItem(value: 'sort-manual', label: 'Manual', selected: false),
        const PopupMenuDivider(height: 1),
        _menuActionItem(
          value: 'remove',
          label: 'Remove project',
          icon: 'trash',
          danger: true,
        ),
      ],
    );
    if (!mounted || value == null) return;
    if (value == 'remove') {
      await _confirmRemove();
    } else {
      // Sort options are placeholders until the daemon tracks per-
      // project sort preference. Surface a no-op feedback for now.
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text('Sort preference not yet wired through the daemon'),
          duration: Duration(seconds: 2),
        ),
      );
    }
  }

  PopupMenuItem<String> _menuRadioItem({
    required String value,
    required String label,
    required bool selected,
  }) {
    return PopupMenuItem<String>(
      value: value,
      height: 38,
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.space5),
      child: Row(
        children: [
          Icon(
            selected ? Icons.radio_button_checked : Icons.radio_button_off,
            size: 16,
            color:
                selected ? AppTokens.accent : AppTokens.textPlaceholder,
          ),
          const SizedBox(width: AppTokens.space4),
          Text(
            label,
            style: const TextStyle(
              fontSize: AppTokens.fontBodyLg,
              color: AppTokens.textPrimary,
            ),
          ),
        ],
      ),
    );
  }

  PopupMenuItem<String> _menuActionItem({
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
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.space5),
      child: Row(
        children: [
          AppIcon(icon, size: 15, color: color),
          const SizedBox(width: AppTokens.space4),
          Text(
            label,
            style: TextStyle(
              fontSize: AppTokens.fontBodyLg,
              color: color,
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _confirmRemove() async {
    final project = widget.project;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: AppTokens.cardBg,
        title: const Text(
          'Remove project?',
          style: TextStyle(color: AppTokens.textPrimary),
        ),
        content: Text(
          'Remove "${project.name}" from AnotherOne. The folder on disk '
          'is left untouched, but its tasks and terminal history disappear '
          'from the sidebar.',
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
            child: const Text('Remove'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    final transport = ref.read(localConnectionProvider);
    try {
      await transport.removeProject(project.id);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to remove project: $e')),
      );
    }
  }
}

class _ProjectAvatar extends StatelessWidget {
  const _ProjectAvatar({required this.project});

  final ProjectSummary project;

  @override
  Widget build(BuildContext context) {
    final letter = project.name.isEmpty
        ? '?'
        : project.name.characters.first.toUpperCase();
    return Container(
      width: AppTokens.avatarSize,
      height: AppTokens.avatarSize,
      decoration: BoxDecoration(
        color: AppTokens.projectColor(project.id),
        borderRadius: BorderRadius.circular(AppTokens.avatarRadius),
      ),
      alignment: Alignment.center,
      child: Text(
        letter,
        style: const TextStyle(
          fontSize: AppTokens.fontBodyLg,
          fontWeight: FontWeight.w700,
          color: AppTokens.textPrimary,
        ),
      ),
    );
  }
}

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
        try {
          await transport.setTaskPinned(
            widget.task.id,
            !widget.task.pinned,
          );
        } catch (e) {
          if (!mounted) return;
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text('Failed to toggle pin: $e')),
          );
        }
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
    try {
      await transport.renameTask(widget.task.id, trimmed);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to rename task: $e')),
      );
    }
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
    try {
      await transport.removeTask(widget.projectId, task.id);
      // If the deleted task was the currently-selected one, clear
      // selection so the main pane drops back to the welcome state.
      final selection = ref.read(selectedTabProvider);
      if (selection?.sectionId == task.sectionId) {
        ref.read(selectedTabProvider.notifier).clear();
      }
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to delete task: $e')),
      );
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
          height: AppTokens.taskRowHeight,
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
              if (subtitle != null)
                Padding(
                  padding: const EdgeInsets.only(top: 2),
                  child: Text(
                    subtitle,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: AppTokens.fontCaption,
                      color: AppTokens.textPlaceholder,
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  String? _buildSubtitle(TaskSummary task) {
    final parts = <String>[];
    if (task.branchName.isNotEmpty && task.branchName != task.name) {
      parts.add(task.branchName);
    }
    if (parts.isEmpty) return null;
    return parts.join(' • ');
  }
}

/// Compact icon button used for the inline ellipsis / plus / github
/// controls on a project row. Mirrors GPUI's 24px square buttons.
class _RowIconButton extends StatefulWidget {
  const _RowIconButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
  });

  final String icon;
  final String tooltip;
  final VoidCallback onPressed;

  @override
  State<_RowIconButton> createState() => _RowIconButtonState();
}

class _RowIconButtonState extends State<_RowIconButton> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: Container(
            width: 24,
            height: 24,
            decoration: BoxDecoration(
              color: _hovered ? AppTokens.overlayHover : Colors.transparent,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            ),
            alignment: Alignment.center,
            child: AppIcon(
              widget.icon,
              size: 13,
              color: AppTokens.textMuted,
            ),
          ),
        ),
      ),
    );
  }
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

/// Approximates the global-coords centre of `context`'s render box —
/// used as the spawn point for project context menus when triggered
/// via the ellipsis button instead of right-click.
Offset _globalCenterOf(BuildContext context) {
  final box = context.findRenderObject() as RenderBox?;
  if (box == null) return Offset.zero;
  final size = box.size;
  return box.localToGlobal(Offset(size.width, size.height / 2));
}
