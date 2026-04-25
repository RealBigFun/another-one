// Terminal tab strip — visual-parity port of the
// `desktop/src/panels.rs` 36px tab bar that sits between the
// titlebar and the terminal pane.
//
// Today the Flutter port carries one selected `(sectionId, tabId)`;
// multi-tab support per task lands when the bridge starts streaming
// per-section tab lists. For now this widget renders the *active*
// tab as a single chip with provider icon + title + close ×.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../rust/api/iroh_client.dart';
import '../../state/local_connection_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../widgets/agent_provider_icon.dart';
import '../../widgets/app_icon.dart';

class DesktopTabStrip extends ConsumerWidget {
  const DesktopTabStrip({super.key, required this.selection});

  final TabSelection selection;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projectsAsync = ref.watch(desktopProjectsProvider);
    final tab = projectsAsync.maybeWhen(
      data: (projects) => _findTab(projects, selection),
      orElse: () => null,
    );
    final task = projectsAsync.maybeWhen(
      data: (projects) => _findTask(projects, selection),
      orElse: () => null,
    );
    final title = tab?.fixedTitle ?? tab?.title ?? task?.name ?? selection.tabId;
    return Container(
      height: AppTokens.tabStripHeight,
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(
          bottom: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: Row(
        children: [
          _TabChip(
            tab: tab,
            title: title,
            onClose: () => ref.read(selectedTabProvider.notifier).clear(),
          ),
          const Spacer(),
        ],
      ),
    );
  }

  static TabSummary? _findTab(
    List<ProjectSummary> projects,
    TabSelection selection,
  ) {
    for (final project in projects) {
      for (final task in project.tasks) {
        if (task.sectionId != selection.sectionId) continue;
        for (final tab in task.tabs) {
          if (tab.id == selection.tabId) return tab;
        }
      }
    }
    return null;
  }

  static TaskSummary? _findTask(
    List<ProjectSummary> projects,
    TabSelection selection,
  ) {
    for (final project in projects) {
      for (final task in project.tasks) {
        if (task.sectionId == selection.sectionId) return task;
      }
    }
    return null;
  }
}

class _TabChip extends StatefulWidget {
  const _TabChip({
    required this.tab,
    required this.title,
    required this.onClose,
  });

  final TabSummary? tab;
  final String title;
  final VoidCallback onClose;

  @override
  State<_TabChip> createState() => _TabChipState();
}

class _TabChipState extends State<_TabChip> {
  bool _hoverClose = false;

  @override
  Widget build(BuildContext context) {
    final tab = widget.tab;
    final pinned = tab?.pinned ?? false;
    return Container(
      height: AppTokens.tabStripHeight,
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.space5),
      decoration: const BoxDecoration(
        color: AppTokens.terminalBg,
        border: Border(
          right: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (pinned) ...[
            const AppIcon(
              'pin-off',
              size: 12,
              color: AppTokens.textPrimary,
            ),
            const SizedBox(width: AppTokens.space2),
          ],
          AgentProviderIcon(
            provider: tab?.provider,
            size: 13,
            color: AppTokens.textPrimary,
          ),
          const SizedBox(width: AppTokens.space2),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 280),
            child: Text(
              widget.title,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(
                fontSize: AppTokens.fontBodyLg,
                color: AppTokens.textPrimary,
              ),
            ),
          ),
          const SizedBox(width: AppTokens.space3),
          MouseRegion(
            cursor: SystemMouseCursors.click,
            onEnter: (_) => setState(() => _hoverClose = true),
            onExit: (_) => setState(() => _hoverClose = false),
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: widget.onClose,
              child: AppIcon(
                'close',
                size: 12,
                color: _hoverClose
                    ? AppTokens.textPrimary
                    : AppTokens.textPlaceholder,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
