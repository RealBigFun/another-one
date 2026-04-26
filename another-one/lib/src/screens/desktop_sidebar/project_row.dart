// Project-row widgets — split out of `desktop_sidebar.dart` so the
// shell file stays focused on the column layout rather than per-row
// behaviour. `part of` keeps the underscore-prefixed classes
// library-private without forcing them public for cross-file
// imports.

part of 'desktop_sidebar.dart';

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
    final activeProjectPage = ref.watch(activeProjectPageProvider);
    // GPUI marks the row active in two cases: the user is viewing
    // this project's overview page, OR a task under this project is
    // the focused terminal. Either way the periwinkle outline shows.
    final isActive = activeProjectPage == project.id ||
        (selection != null &&
            project.tasks.any((t) => t.sectionId == selection.sectionId));
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
              onTap: () => ref
                  .read(activeProjectPageProvider.notifier)
                  .state = project.id,
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
                          // GPUI's `project_row` text_col =
                          // hsla(0,0,0.90,1) → opaque gray
                          // 0xFFE5E5E5; 14px / w500.
                          fontSize: AppTokens.fontHeadingSm,
                          fontWeight: FontWeight.w500,
                          color: Color(0xFFE5E5E5),
                        ),
                      ),
                    ),
                    const SizedBox(width: AppTokens.space2),
                    // Chevron is its own gesture region so the tap
                    // toggles expansion without bubbling up to the
                    // row's "activate project page" handler. Mirrors
                    // GPUI's `cx.stop_propagation()` on the chevron.
                    GestureDetector(
                      behavior: HitTestBehavior.opaque,
                      onTap: () =>
                          setState(() => _expanded = !_expanded),
                      child: Padding(
                        padding: const EdgeInsets.symmetric(
                            horizontal: AppTokens.space1),
                        child: AppIcon(
                          _expanded ? 'chevron-down' : 'chevron-right',
                          size: 12,
                          color: AppTokens.chevron,
                        ),
                      ),
                    ),
                    const SizedBox(width: AppTokens.space2),
                    HoverIconButton(
                      icon: 'ellipsis',
                      tooltip: 'More',
                      onPressed: () =>
                          _showProjectMenu(_globalCenterOf(context)),
                    ),
                    // GitHub link slot — GPUI keeps the slot in the
                    // row layout always (so widths stay stable) and
                    // toggles visibility/clickability based on
                    // whether the project's `origin` remote
                    // resolves to a github.com URL. We mirror that
                    // by reading the cached `projectGithubUrlProvider`
                    // for this project id.
                    _ProjectGithubButton(projectId: project.id),
                    HoverIconButton(
                      icon: 'plus',
                      tooltip: 'New task',
                      onPressed: () =>
                          showNewTaskModal(context, project: project),
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
  /// Sort lives display-side so the bridge data stays raw — the
  /// daemon doesn't need to know the UI's preferred render order.
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
    await runMutation(
      context,
      () => transport.removeProject(project.id),
      errorPrefix: 'Failed to remove project',
    );
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
          // GPUI avatar letter: rems(12./16.) = 12px,
          // FontWeight::BOLD = w700, color = pure
          // opaque white (gpui::white(), not a translucent
          // text token).
          fontSize: AppTokens.fontBody,
          fontWeight: FontWeight.w700,
          color: Colors.white,
        ),
      ),
    );
  }
}

/// GitHub-link slot for project rows. GPUI keeps the slot present
/// in the row layout regardless of whether a URL resolves — when
/// it doesn't, the icon is rendered transparently so widths stay
/// stable as the cache populates. We mirror that with
/// `HoverIconButton`'s `iconOpacity` and `onPressed: null` modes:
/// the slot occupies the same 24×24 either way, and `onPressed`
/// gates the hover bg + click handler.
class _ProjectGithubButton extends ConsumerWidget {
  const _ProjectGithubButton({required this.projectId});

  final String projectId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final url = ref.watch(projectGithubUrlProvider(projectId)).valueOrNull;
    final hasUrl = url != null && url.isNotEmpty;
    return HoverIconButton(
      icon: 'github',
      tooltip: "Open this project's GitHub link",
      iconOpacity: hasUrl ? 1.0 : 0.0,
      onPressed: hasUrl
          ? () async {
              final uri = Uri.tryParse(url);
              if (uri == null) return;
              await launchUrl(uri, mode: LaunchMode.externalApplication);
            }
          : null,
    );
  }
}
