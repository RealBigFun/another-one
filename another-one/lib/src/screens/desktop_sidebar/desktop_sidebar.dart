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
// Per-row widgets live in sibling part files so this file stays
// focused on the column layout. The privates stay underscore-
// prefixed; `part of` keeps them library-private without forcing
// them public for cross-file imports.

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:url_launcher/url_launcher.dart';

import '../../rust/api/iroh_client.dart';
import '../../state/active_project_page_provider.dart';
import '../../state/github_url_provider.dart';
import '../../state/local_connection_provider.dart';
import '../../state/rename_target_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';
import '../../widgets/empty_state.dart';
import '../../widgets/hover_icon_button.dart';
import '../../widgets/run_mutation.dart';
import '../new_task/new_task_modal.dart';

part 'project_row.dart';
part 'task_row.dart';

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
              error: (e, _) => EmptyState(
                text: 'Project list error: $e',
                padding: const EdgeInsets.all(AppTokens.space7),
                fontSize: AppTokens.fontBody,
              ),
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
          HoverIconButton(
            size: 28,
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
          HoverIconButton(
            size: 28,
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
    if (!context.mounted) return;
    final transport = ref.read(localConnectionProvider);
    final inserted = await runMutation(
      context,
      () => transport.addProject(selectedPath),
      errorPrefix: 'Failed to add project',
    );
    if (inserted == false && context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text('Project already added at that path'),
          duration: Duration(seconds: 3),
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
      return const EmptyState(
        text: 'No projects yet.\nUse the + at the bottom to add one.',
        padding: EdgeInsets.all(AppTokens.space7),
        fontSize: AppTokens.fontBody,
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

/// Approximates the global-coords centre of `context`'s render box —
/// used as the spawn point for project context menus when triggered
/// via the ellipsis button instead of right-click.
Offset _globalCenterOf(BuildContext context) {
  final box = context.findRenderObject() as RenderBox?;
  if (box == null) return Offset.zero;
  final size = box.size;
  return box.localToGlobal(Offset(size.width, size.height / 2));
}
