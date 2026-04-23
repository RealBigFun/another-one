// Home screen once paired: a drawer-style list of projects that
// expand in-place to reveal their tasks. Visual shape mirrors
// `desktop/src/left_sidebar.rs::project_row` (colour-chip avatar,
// name + metadata, chevron, task-count badge) — transcribed to
// Flutter rather than shared.
//
// Tapping a task row pushes `TaskPage`. Tapping the gear in the app
// bar pushes `SettingsPage`.

import 'package:flutter/material.dart';

import 'rust/api/iroh_client.dart';
import 'task_page.dart';
import 'tokens.dart';
import 'transport_iroh.dart';

class ProjectsDrawerPage extends StatefulWidget {
  const ProjectsDrawerPage({
    super.key,
    required this.transport,
    required this.projects,
    required this.onRefresh,
    required this.onOpenSettings,
  });

  final IrohTransport transport;
  final List<ProjectSummary> projects;

  /// Called when the user pulls-to-refresh. Should trigger
  /// `transport.listProjects()` and complete when the caller has
  /// dispatched the request (it doesn't have to wait for the reply —
  /// the parent updates `projects` as replies arrive).
  final Future<void> Function() onRefresh;

  /// Pushed from the gear icon.
  final VoidCallback onOpenSettings;

  @override
  State<ProjectsDrawerPage> createState() => _ProjectsDrawerPageState();
}

class _ProjectsDrawerPageState extends State<ProjectsDrawerPage> {
  final Set<String> _expanded = <String>{};

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('AnotherOne'),
        actions: [
          IconButton(
            tooltip: 'Settings',
            icon: const Icon(Icons.settings),
            onPressed: widget.onOpenSettings,
          ),
        ],
      ),
      body: SafeArea(
        child: RefreshIndicator(
          onRefresh: widget.onRefresh,
          child: widget.projects.isEmpty
              ? _buildEmpty()
              : _buildList(),
        ),
      ),
    );
  }

  Widget _buildEmpty() {
    // Wrap in a ListView so the RefreshIndicator has something
    // scrollable even when the project list is empty.
    return ListView(
      physics: const AlwaysScrollableScrollPhysics(),
      children: const [
        SizedBox(height: 120),
        Center(
          child: Padding(
            padding: EdgeInsets.all(AppTokens.space7),
            child: Text(
              'No projects.\n'
              'Add one in the desktop app, then pull to refresh.',
              textAlign: TextAlign.center,
              style: TextStyle(
                fontSize: AppTokens.fontBodyLg,
                color: AppTokens.textMuted,
              ),
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildList() {
    return ListView.separated(
      physics: const AlwaysScrollableScrollPhysics(),
      itemCount: widget.projects.length,
      separatorBuilder: (_, _) => const Divider(
        height: 1,
        color: AppTokens.divider,
      ),
      itemBuilder: (_, i) {
        final project = widget.projects[i];
        final expanded = _expanded.contains(project.id);
        return ProjectDrawerRow(
          project: project,
          expanded: expanded,
          onToggle: () {
            setState(() {
              if (expanded) {
                _expanded.remove(project.id);
              } else {
                _expanded.add(project.id);
              }
            });
          },
          onTaskTap: (task) {
            Navigator.of(context).push(
              MaterialPageRoute(
                builder: (_) => TaskPage(
                  transport: widget.transport,
                  project: project,
                  task: task,
                ),
              ),
            );
          },
        );
      },
    );
  }
}

/// One project row + (when expanded) the nested list of tasks.
class ProjectDrawerRow extends StatelessWidget {
  const ProjectDrawerRow({
    super.key,
    required this.project,
    required this.expanded,
    required this.onToggle,
    required this.onTaskTap,
  });

  final ProjectSummary project;
  final bool expanded;
  final VoidCallback onToggle;
  final void Function(TaskSummary task) onTaskTap;

  @override
  Widget build(BuildContext context) {
    final hasTasks = project.tasks.isNotEmpty;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _ProjectHeader(
          project: project,
          expanded: expanded,
          hasChildren: hasTasks,
          onTap: onToggle,
        ),
        AnimatedSize(
          duration: const Duration(milliseconds: 180),
          curve: Curves.easeOutCubic,
          alignment: Alignment.topCenter,
          child: expanded && hasTasks
              ? Column(
                  children: [
                    for (final task in project.tasks)
                      TaskRow(
                        task: task,
                        onTap: () => onTaskTap(task),
                      ),
                  ],
                )
              : const SizedBox(width: double.infinity, height: 0),
        ),
      ],
    );
  }
}

class _ProjectHeader extends StatelessWidget {
  const _ProjectHeader({
    required this.project,
    required this.expanded,
    required this.hasChildren,
    required this.onTap,
  });

  final ProjectSummary project;
  final bool expanded;
  final bool hasChildren;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final chipColor = AppTokens.projectColor(project.id);
    final initial = project.name.isEmpty
        ? '?'
        : project.name.characters.first.toUpperCase();

    final branch = project.currentBranch;
    final worktree = project.kind == ProjectKind.worktree;

    return InkWell(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.space4,
          vertical: AppTokens.space2,
        ),
        child: Row(
          children: [
            // Colour-chip avatar — matches desktop's 24x24 square with
            // the first initial in bold white.
            Container(
              width: 32,
              height: 32,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: chipColor,
                borderRadius: BorderRadius.circular(AppTokens.radiusSm),
              ),
              child: Text(
                initial,
                style: const TextStyle(
                  color: Colors.white,
                  fontSize: AppTokens.fontBodyLg,
                  fontWeight: FontWeight.w700,
                ),
              ),
            ),
            const SizedBox(width: AppTokens.space3),
            // Name + branch stacked.
            Expanded(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    project.name,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      color: AppTokens.textPrimary,
                      fontSize: AppTokens.fontBodyLg,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  if (branch != null && branch.isNotEmpty)
                    Text(
                      '⎇ $branch',
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(
                        color: AppTokens.textMuted,
                        fontSize: AppTokens.fontSmall,
                        fontFamily: AppTokens.fontFamilyMono,
                      ),
                    ),
                ],
              ),
            ),
            const SizedBox(width: AppTokens.space3),
            // Worktree glyph OR task-count badge, right-aligned.
            if (worktree)
              const Padding(
                padding: EdgeInsets.only(right: AppTokens.space2),
                child: Icon(
                  Icons.call_split,
                  size: AppTokens.iconSizeDefault,
                  color: AppTokens.textMuted,
                ),
              ),
            if (project.tasks.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(right: AppTokens.space2),
                child: _TaskCountBadge(count: project.tasks.length),
              ),
            if (hasChildren)
              Icon(
                expanded ? Icons.expand_less : Icons.chevron_right,
                size: AppTokens.iconSizeDefault,
                color: AppTokens.chevron,
              ),
          ],
        ),
      ),
    );
  }
}

class _TaskCountBadge extends StatelessWidget {
  const _TaskCountBadge({required this.count});

  final int count;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space3,
        vertical: 2,
      ),
      decoration: BoxDecoration(
        color: AppTokens.overlayHover,
        borderRadius: BorderRadius.circular(AppTokens.radiusLg),
      ),
      child: Text(
        '$count',
        style: const TextStyle(
          color: AppTokens.textSecondary,
          fontSize: AppTokens.fontSmall,
          fontFamily: AppTokens.fontFamilyMono,
        ),
      ),
    );
  }
}

class TaskRow extends StatelessWidget {
  const TaskRow({
    super.key,
    required this.task,
    required this.onTap,
  });

  final TaskSummary task;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.only(
          left: AppTokens.space10, // inset under the project avatar
          right: AppTokens.space4,
          top: AppTokens.space2,
          bottom: AppTokens.space2,
        ),
        child: Row(
          children: [
            const Icon(
              Icons.subdirectory_arrow_right,
              size: AppTokens.iconSizeSm,
              color: AppTokens.textMuted,
            ),
            const SizedBox(width: AppTokens.space3),
            Expanded(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    task.name,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      color: AppTokens.textPrimary,
                      fontSize: AppTokens.fontBody,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  Text(
                    '⎇ ${task.branchName}',
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      color: AppTokens.textMuted,
                      fontSize: AppTokens.fontCaption,
                      fontFamily: AppTokens.fontFamilyMono,
                    ),
                  ),
                ],
              ),
            ),
            if (task.tabs.isNotEmpty) ...[
              const SizedBox(width: AppTokens.space2),
              _TaskCountBadge(count: task.tabs.length),
            ],
            const SizedBox(width: AppTokens.space2),
            const Icon(
              Icons.chevron_right,
              size: AppTokens.iconSizeDefault,
              color: AppTokens.chevron,
            ),
          ],
        ),
      ),
    );
  }
}
