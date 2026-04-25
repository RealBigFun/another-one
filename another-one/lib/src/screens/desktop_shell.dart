// Desktop shell — the top-level layout for tablet/desktop/wideDesktop
// breakpoints. Hosts the chrome (titlebar, future sidebar) and a
// content slot for whatever screen the user is on.
//
// Pre-Phase 6: this is a thin scaffold so the pair-mobile titlebar
// button is reachable end-to-end. The real sidebar + project page
// land in Phase 3 #2; the placeholder "Welcome" body keeps the
// shell shippable in the meantime.

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/iroh_client.dart';
import '../state/local_connection_provider.dart';
import '../state/tab_selection_provider.dart';
import '../tokens.dart';
import 'desktop_terminal/desktop_terminal_pane.dart';
import 'pair_mobile/pair_mobile_modal.dart';

const double _titlebarHeight = 32;
const double _sidebarWidth = 280;

class DesktopShell extends ConsumerWidget {
  const DesktopShell({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // Eagerly read the local connection so the daemon-backed
    // transport spins up before anything tries to render projects.
    ref.watch(localConnectionProvider);
    return Scaffold(
      backgroundColor: AppTokens.terminalBg,
      body: Column(
        children: [
          const _Titlebar(),
          Expanded(
            child: Row(
              children: [
                const _Sidebar(),
                Expanded(child: _MainArea()),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _Sidebar extends ConsumerWidget {
  const _Sidebar();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projects = ref.watch(desktopProjectsProvider);
    return Container(
      width: _sidebarWidth,
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
        ],
      ),
    );
  }
}

class _SidebarHeader extends ConsumerWidget {
  const _SidebarHeader();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(
        AppTokens.space5,
        AppTokens.space3,
        AppTokens.space2,
        AppTokens.space2,
      ),
      child: Row(
        children: [
          const Expanded(
            child: Text(
              'Projects',
              style: TextStyle(
                fontSize: AppTokens.fontCaption,
                fontWeight: FontWeight.w600,
                color: AppTokens.textPlaceholder,
                letterSpacing: 0.6,
              ),
            ),
          ),
          _SidebarRowButton(
            onTap: () => _addProject(context, ref),
            padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.space2,
              vertical: 2,
            ),
            child: const Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.add, size: 14, color: AppTokens.textSecondary),
                SizedBox(width: 2),
                Text(
                  'Add',
                  style: TextStyle(
                    fontSize: AppTokens.fontCaption,
                    color: AppTokens.textSecondary,
                  ),
                ),
              ],
            ),
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

class _ProjectList extends StatelessWidget {
  const _ProjectList(this.projects);

  final List<ProjectSummary> projects;

  @override
  Widget build(BuildContext context) {
    if (projects.isEmpty) {
      return const _SidebarMessage(
        text: 'No projects yet.\nAdd one from the desktop app to see it here.',
      );
    }
    return ListView.builder(
      padding: const EdgeInsets.symmetric(vertical: AppTokens.space3),
      itemCount: projects.length,
      itemBuilder: (_, i) => _ProjectRow(projects[i]),
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
              backgroundColor: const Color(0xFF8A2A2A),
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

  @override
  Widget build(BuildContext context) {
    final project = widget.project;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space3,
        vertical: AppTokens.space1,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _SidebarRowButton(
            onTap: () => setState(() => _expanded = !_expanded),
            onSecondaryTapDown: (details) =>
                _showProjectContextMenu(context, details.globalPosition),
            child: Row(
              children: [
                Icon(
                  _expanded ? Icons.expand_more : Icons.chevron_right,
                  size: 16,
                  color: AppTokens.chevron,
                ),
                const SizedBox(width: 2),
                Container(
                  width: 14,
                  height: 14,
                  decoration: BoxDecoration(
                    color: AppTokens.projectColor(project.id),
                    borderRadius: BorderRadius.circular(AppTokens.radiusXs),
                  ),
                ),
                const SizedBox(width: AppTokens.space3),
                Expanded(
                  child: Text(
                    project.name,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: AppTokens.fontBodyLg,
                      color: AppTokens.textPrimary,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                ),
                if (project.currentBranch != null &&
                    project.currentBranch!.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(left: AppTokens.space2),
                    child: Text(
                      project.currentBranch!,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(
                        fontFamily: AppTokens.fontFamilyMono,
                        fontSize: AppTokens.fontCaption,
                        color: AppTokens.textPlaceholder,
                      ),
                    ),
                  ),
              ],
            ),
          ),
          if (_expanded)
            Padding(
              padding: const EdgeInsets.only(
                left: 18,
                top: AppTokens.space1,
                bottom: AppTokens.space1,
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (final task in project.tasks) _TaskRow(task: task),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

class _TaskRow extends ConsumerWidget {
  const _TaskRow({required this.task});

  final TaskSummary task;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selection = ref.watch(selectedTabProvider);
    final taskHasActive = selection != null &&
        selection.sectionId == task.sectionId &&
        task.tabs.any((t) => t.id == selection.tabId);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 1),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _SidebarRowButton(
            onTap: () {
              if (task.activeTabId.isEmpty) return;
              ref.read(selectedTabProvider.notifier).state = TabSelection(
                sectionId: task.sectionId,
                tabId: task.activeTabId,
              );
            },
            highlighted: taskHasActive,
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    task.name,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: AppTokens.fontBody,
                      color: AppTokens.textSecondary,
                    ),
                  ),
                ),
                if (task.pinned)
                  const Padding(
                    padding: EdgeInsets.only(left: AppTokens.space1),
                    child: Icon(Icons.push_pin,
                        size: 11, color: AppTokens.accent),
                  ),
              ],
            ),
          ),
          if (task.tabs.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(
                left: AppTokens.space3,
                top: 1,
                bottom: 1,
              ),
              child: Wrap(
                spacing: AppTokens.space1,
                runSpacing: AppTokens.space1,
                children: [
                  for (final tab in task.tabs)
                    _TabChip(
                      sectionId: task.sectionId,
                      tab: tab,
                      selected: selection?.sectionId == task.sectionId &&
                          selection?.tabId == tab.id,
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

class _TabChip extends ConsumerWidget {
  const _TabChip({
    required this.sectionId,
    required this.tab,
    required this.selected,
  });

  final String sectionId;
  final TabSummary tab;
  final bool selected;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final label = tab.fixedTitle ?? tab.title;
    return _SidebarRowButton(
      onTap: () {
        ref.read(selectedTabProvider.notifier).state = TabSelection(
          sectionId: sectionId,
          tabId: tab.id,
        );
      },
      highlighted: selected,
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space2,
        vertical: 2,
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (tab.running)
            Container(
              width: 6,
              height: 6,
              margin: const EdgeInsets.only(right: AppTokens.space1),
              decoration: BoxDecoration(
                color: AppTokens.successIcon,
                borderRadius: BorderRadius.circular(AppTokens.radiusPill),
              ),
            ),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 140),
            child: Text(
              label,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                fontSize: AppTokens.fontCaption,
                color:
                    selected ? AppTokens.textPrimary : AppTokens.textMuted,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// Reusable click target with hover/selection backgrounds. Centralised
/// so every sidebar row treats interaction identically — no rogue
/// hover effects, no clicks-without-cursor.
class _SidebarRowButton extends StatefulWidget {
  const _SidebarRowButton({
    required this.onTap,
    required this.child,
    this.highlighted = false,
    this.padding = const EdgeInsets.symmetric(
      horizontal: AppTokens.space2,
      vertical: 3,
    ),
    this.onSecondaryTapDown,
  });

  final VoidCallback onTap;
  final Widget child;
  final bool highlighted;
  final EdgeInsets padding;
  final GestureTapDownCallback? onSecondaryTapDown;

  @override
  State<_SidebarRowButton> createState() => _SidebarRowButtonState();
}

class _SidebarRowButtonState extends State<_SidebarRowButton> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final bg = widget.highlighted
        ? AppTokens.overlayActive
        : (_hovered ? AppTokens.overlayHover : Colors.transparent);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        onSecondaryTapDown: widget.onSecondaryTapDown,
        child: Container(
          padding: widget.padding,
          decoration: BoxDecoration(
            color: bg,
            borderRadius: BorderRadius.circular(AppTokens.radiusSm),
          ),
          child: widget.child,
        ),
      ),
    );
  }
}

extension _ProjectRowMenu on _ProjectRowState {
  Future<void> _showProjectContextMenu(
    BuildContext context,
    Offset globalPosition,
  ) async {
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final value = await showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(
        globalPosition.dx,
        globalPosition.dy,
        overlay.size.width - globalPosition.dx,
        overlay.size.height - globalPosition.dy,
      ),
      color: AppTokens.cardBg,
      items: const [
        PopupMenuItem<String>(
          value: 'remove',
          child: Text(
            'Remove project',
            style: TextStyle(color: AppTokens.textPrimary),
          ),
        ),
      ],
    );
    if (value == 'remove') {
      await _confirmRemove();
    }
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

class _MainArea extends ConsumerWidget {
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selection = ref.watch(selectedTabProvider);
    return Container(
      color: AppTokens.terminalBg,
      alignment: Alignment.center,
      child: selection == null
          ? const _WelcomePlaceholder()
          : DesktopTerminalPane(selection: selection),
    );
  }
}

class _Titlebar extends StatelessWidget {
  const _Titlebar();

  @override
  Widget build(BuildContext context) {
    return Container(
      height: _titlebarHeight,
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(
          bottom: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: Row(
        children: [
          const SizedBox(width: AppTokens.space5),
          const Text(
            'AnotherOne',
            style: TextStyle(
              fontSize: AppTokens.fontBody,
              fontWeight: FontWeight.w500,
              color: AppTokens.textSecondary,
            ),
          ),
          const Spacer(),
          const _PairMobileButton(),
          const SizedBox(width: AppTokens.space2),
        ],
      ),
    );
  }
}

class _PairMobileButton extends StatelessWidget {
  const _PairMobileButton();

  @override
  Widget build(BuildContext context) {
    return _TitlebarIconButton(
      tooltip: 'Pair a mobile device with the embedded daemon',
      icon: Icons.qr_code_2,
      onPressed: () => showPairMobileModal(context),
    );
  }
}

class _TitlebarIconButton extends StatefulWidget {
  const _TitlebarIconButton({
    required this.tooltip,
    required this.icon,
    required this.onPressed,
  });

  final String tooltip;
  final IconData icon;
  final VoidCallback onPressed;

  @override
  State<_TitlebarIconButton> createState() => _TitlebarIconButtonState();
}

class _TitlebarIconButtonState extends State<_TitlebarIconButton> {
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
            width: 40,
            height: 24,
            decoration: BoxDecoration(
              color: _hovered ? AppTokens.overlayHoverStrong : AppTokens.overlayRest,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              border: Border.all(color: AppTokens.border),
            ),
            alignment: Alignment.center,
            child: Icon(
              widget.icon,
              size: AppTokens.iconSizeDefault,
              color: AppTokens.textPrimary,
            ),
          ),
        ),
      ),
    );
  }
}

class _WelcomePlaceholder extends StatelessWidget {
  const _WelcomePlaceholder();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(AppTokens.space10),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(
            Icons.terminal,
            size: 64,
            color: AppTokens.textMuted,
          ),
          const SizedBox(height: AppTokens.space5),
          const Text(
            'AnotherOne',
            style: TextStyle(
              fontSize: AppTokens.fontHeadingLg,
              fontWeight: FontWeight.w600,
              color: AppTokens.textPrimary,
            ),
          ),
          const SizedBox(height: AppTokens.space2),
          const Text(
            'Desktop UI under construction. The sidebar, project view,\n'
            'and task panes land in subsequent phases.',
            textAlign: TextAlign.center,
            style: TextStyle(
              fontSize: AppTokens.fontBodyLg,
              color: AppTokens.textMuted,
            ),
          ),
          const SizedBox(height: AppTokens.space7),
          Text(
            'Use the QR button above to pair a mobile device.',
            style: TextStyle(
              fontSize: AppTokens.fontBody,
              color: AppTokens.textPlaceholder,
            ),
          ),
        ],
      ),
    );
  }
}
