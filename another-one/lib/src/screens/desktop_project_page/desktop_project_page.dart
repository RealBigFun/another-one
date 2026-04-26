// Project overview page rendered in the desktop's main pane when a
// project row (not a task row) is the active selection. Mirrors
// `desktop/src/project_page.rs`.
//
// Today's port covers the header bar only — name + "New Task" +
// "View on GitHub" + "Remove project". The body sections (Open PRs
// list with filter tabs + search; Branch settings configuration
// panel) are deferred until the bridge surface for PR fetching and
// `ResolvedProjectBranchSettings` lands. Until then, the body
// renders an explicit pending-feature message so reviewers can
// distinguish "intentionally not yet implemented" from "broken".

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../rust/api/iroh_client.dart' show ProjectSummary;
import '../../state/active_project_page_provider.dart';
import '../../state/github_url_provider.dart';
import '../../state/local_connection_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';
import '../../widgets/run_mutation.dart';
import '../new_task/new_task_modal.dart';
import 'configuration_section.dart';

class DesktopProjectPage extends ConsumerWidget {
  const DesktopProjectPage({super.key, required this.project});

  final ProjectSummary project;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Container(
      color: const Color(0xFF1E1F22),
      child: Column(
        children: [
          _ProjectPageHeader(project: project),
          Expanded(child: _ProjectPageBody(projectId: project.id)),
        ],
      ),
    );
  }
}

class _ProjectPageHeader extends ConsumerWidget {
  const _ProjectPageHeader({required this.project});

  final ProjectSummary project;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final githubUrl = ref.watch(projectGithubUrlProvider(project.id)).valueOrNull;
    final hasGithub = githubUrl != null && githubUrl.isNotEmpty;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
      decoration: const BoxDecoration(
        border: Border(bottom: BorderSide(color: AppTokens.divider)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Text(
              project.name,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(
                fontSize: 16,
                fontWeight: FontWeight.w600,
                color: AppTokens.textPrimary,
              ),
            ),
          ),
          const SizedBox(width: 12),
          _NewTaskPill(project: project),
          if (hasGithub) ...[
            const SizedBox(width: 12),
            _SquareIconButton(
              icon: 'github',
              tooltip: "Open this project's GitHub repository",
              iconSize: 14,
              iconColor: AppTokens.textSecondary,
              onPressed: () async {
                final uri = Uri.tryParse(githubUrl);
                if (uri == null) return;
                await launchUrl(uri, mode: LaunchMode.externalApplication);
              },
            ),
          ],
          const SizedBox(width: 12),
          _SquareIconButton(
            icon: 'trash',
            tooltip: 'Remove this project group from the sidebar',
            iconSize: 14,
            iconColor: AppTokens.textSecondary,
            onPressed: () => _confirmRemove(context, ref),
          ),
        ],
      ),
    );
  }

  Future<void> _confirmRemove(BuildContext context, WidgetRef ref) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: AppTokens.cardBg,
        title: Text(
          'Remove project?',
          style: const TextStyle(color: AppTokens.textPrimary),
        ),
        content: Text(
          'Stop tracking "${project.name}" in this workspace? Tasks and '
          'terminals associated with the project will also be removed. '
          "The project's files on disk are not touched.",
          style: const TextStyle(color: AppTokens.textSecondary),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: const Text('Cancel'),
          ),
          TextButton(
            style: TextButton.styleFrom(
              foregroundColor: AppTokens.errorText,
              backgroundColor: AppTokens.dangerBg,
            ),
            onPressed: () => Navigator.of(ctx).pop(true),
            child: const Text('Remove'),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    if (!context.mounted) return;
    final connection = ref.read(localConnectionProvider);
    final result = await runMutation<bool>(
      context,
      () async {
        await connection.removeProject(project.id);
        return true;
      },
      errorPrefix: 'Could not remove project',
    );
    if (result != true) return;
    // Clear the active page since the project just went away.
    ref.read(activeProjectPageProvider.notifier).state = null;
  }
}

/// "New Task" pill — primary CTA in the header. Mirrors GPUI's
/// rounded-rectangle button with the plus glyph + bold label.
class _NewTaskPill extends StatefulWidget {
  const _NewTaskPill({required this.project});

  final ProjectSummary project;

  @override
  State<_NewTaskPill> createState() => _NewTaskPillState();
}

class _NewTaskPillState extends State<_NewTaskPill> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () => showNewTaskModal(context, project: widget.project),
        child: Container(
          height: 30,
          padding: const EdgeInsets.symmetric(horizontal: 7),
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: _hover ? AppTokens.overlayHover : const Color(0xFF1E2024),
            borderRadius: BorderRadius.circular(7),
            border: Border.all(color: AppTokens.border),
          ),
          child: const Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              AppIcon('plus', size: 12, color: AppTokens.textPrimary),
              SizedBox(width: 5),
              Text(
                'New Task',
                style: TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.w600,
                  color: AppTokens.textPrimary,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// 30×30 icon-only square button — matches the GitHub + Remove
/// affordances on the GPUI project page header.
class _SquareIconButton extends StatefulWidget {
  const _SquareIconButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    required this.iconSize,
    required this.iconColor,
  });

  final String icon;
  final String tooltip;
  final VoidCallback onPressed;
  final double iconSize;
  final Color iconColor;

  @override
  State<_SquareIconButton> createState() => _SquareIconButtonState();
}

class _SquareIconButtonState extends State<_SquareIconButton> {
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
            width: 30,
            height: 30,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: _hover ? AppTokens.overlayHover : const Color(0xFF1E2024),
              borderRadius: BorderRadius.circular(7),
              border: Border.all(color: AppTokens.border),
            ),
            child: AppIcon(
              widget.icon,
              size: widget.iconSize,
              color: widget.iconColor,
            ),
          ),
        ),
      ),
    );
  }
}

class _ProjectPageBody extends StatelessWidget {
  const _ProjectPageBody({required this.projectId});

  final String projectId;

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Open PRs section is filed under another-one-g7f.1; will
          // land here when its bridge surface ships. The
          // Configuration panel below is independent.
          ConfigurationSection(projectId: projectId),
        ],
      ),
    );
  }
}
