// Terminal tab strip — visual-parity port of
// `desktop/src/panels.rs::section_main_panel`'s tab bar half.
//
// Renders every tab in the active section as a 36px-tall chip with
// pin glyph (when pinned), provider icon, title, close button.
// Active tab uses terminalBg; inactive uses cardBg with a hover
// tint (#2F3136). Trailing "+" button opens the Add Agent modal.
// Right-click on a tab opens a small Pin/Unpin menu. Closing a
// pinned tab routes through a 364px confirmation dialog.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../rust/api/iroh_client.dart';
import '../../state/local_connection_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../widgets/agent_provider_icon.dart';
import '../../widgets/app_icon.dart';
import '../../widgets/app_toast.dart';
import '../add_agent_modal/add_agent_modal.dart';

const Color _tabBarBg = Color(0xFF27292E);
const Color _tabBgActive = AppTokens.terminalBg;
const Color _tabBgInactive = Color(0xFF2B2D31);
const Color _tabHover = Color(0xFF2F3136);

class DesktopTabStrip extends ConsumerWidget {
  const DesktopTabStrip({super.key, required this.selection});

  final TabSelection selection;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projectsAsync = ref.watch(desktopProjectsProvider);
    final task = projectsAsync.maybeWhen(
      data: (projects) => _findTask(projects, selection),
      orElse: () => null,
    );
    final tabs = task?.tabs ?? const <TabSummary>[];
    // Pinned-first sort, mirroring `SectionState::sort_tabs_by_pin`.
    final sorted = [...tabs]
      ..sort((a, b) {
        if (a.pinned == b.pinned) return 0;
        return a.pinned ? -1 : 1;
      });

    return Container(
      height: AppTokens.tabStripHeight,
      decoration: const BoxDecoration(
        color: _tabBarBg,
        border: Border(
          bottom: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: Row(
        children: [
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: Row(
                children: [
                  for (var i = 0; i < sorted.length; i++)
                    _Tab(
                      sectionId: selection.sectionId,
                      tab: sorted[i],
                      indexLabel: tabs.length > 1 ? (i + 1).toString() : null,
                      active: sorted[i].id == selection.tabId,
                    ),
                ],
              ),
            ),
          ),
          _AddAgentButton(sectionId: selection.sectionId),
          const SizedBox(width: 4),
        ],
      ),
    );
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

class _Tab extends ConsumerStatefulWidget {
  const _Tab({
    required this.sectionId,
    required this.tab,
    required this.indexLabel,
    required this.active,
  });
  final String sectionId;
  final TabSummary tab;
  final String? indexLabel;
  final bool active;

  @override
  ConsumerState<_Tab> createState() => _TabState();
}

class _TabState extends ConsumerState<_Tab> {
  bool _hover = false;
  bool _hoverClose = false;

  Future<void> _activate() async {
    if (widget.active) return;
    ref
        .read(selectedTabProvider.notifier)
        .set(TabSelection(sectionId: widget.sectionId, tabId: widget.tab.id));
    final connection = ref.read(localConnectionProvider);
    try {
      await connection.activateSectionTab(
        sectionId: widget.sectionId,
        tabId: widget.tab.id,
      );
    } catch (_) {
      // Best-effort persistence; UI selection already updated.
    }
  }

  Future<void> _close() async {
    if (widget.tab.pinned) {
      final confirmed = await _showPinnedTabCloseConfirm(
        context,
        title: widget.tab.fixedTitle ?? widget.tab.title,
      );
      if (!mounted || !confirmed) return;
    }
    final connection = ref.read(localConnectionProvider);
    try {
      final newActive = await connection.closeSectionTab(
        sectionId: widget.sectionId,
        tabId: widget.tab.id,
      );
      // Local transport mutates the persisted store; refresh the
      // project tree so the strip re-renders without this tab.
      await connection.listProjects();
      if (!mounted) return;
      if (widget.active) {
        if (newActive.isEmpty) {
          ref.read(selectedTabProvider.notifier).clear();
        } else {
          ref
              .read(selectedTabProvider.notifier)
              .set(TabSelection(sectionId: widget.sectionId, tabId: newActive));
        }
      }
    } catch (e) {
      if (!mounted) return;
      showAppToast(context, message: 'Could not close tab: $e');
    }
  }

  Future<void> _togglePinned() async {
    final connection = ref.read(localConnectionProvider);
    try {
      await connection.toggleSectionTabPinned(
        sectionId: widget.sectionId,
        tabId: widget.tab.id,
      );
      await connection.listProjects();
    } catch (e) {
      if (!mounted) return;
      showAppToast(context, message: 'Could not toggle pin: $e');
    }
  }

  void _showContextMenu(Offset globalPos) {
    final overlay =
        Overlay.of(context).context.findRenderObject() as RenderBox?;
    if (overlay == null) return;
    showMenu<void>(
      context: context,
      position: RelativeRect.fromLTRB(
        globalPos.dx,
        globalPos.dy,
        overlay.size.width - globalPos.dx,
        overlay.size.height - globalPos.dy,
      ),
      color: AppTokens.cardBg,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        side: const BorderSide(color: AppTokens.border),
      ),
      items: [
        PopupMenuItem<void>(
          height: 38,
          padding: const EdgeInsets.symmetric(horizontal: 14),
          onTap: _togglePinned,
          child: Row(
            children: [
              const AppIcon('pin-off', size: 15, color: AppTokens.textPrimary),
              const SizedBox(width: 10),
              Text(
                widget.tab.pinned ? 'Unpin' : 'Pin',
                style: const TextStyle(
                  fontSize: 13,
                  fontWeight: FontWeight.w500,
                  color: AppTokens.textPrimary,
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final tab = widget.tab;
    final title = tab.fixedTitle ?? tab.title;
    final displayTitle = widget.indexLabel != null
        ? '$title ${widget.indexLabel}'
        : title;
    final bg = widget.active
        ? _tabBgActive
        : (_hover ? _tabHover : _tabBgInactive);
    final textColor = widget.active
        ? AppTokens.textPrimary
        : const Color(0x8CFFFFFF);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () => unawaited(_activate()),
        onSecondaryTapDown: (details) =>
            _showContextMenu(details.globalPosition),
        child: Container(
          height: AppTokens.tabStripHeight,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          color: bg,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (tab.pinned) ...[
                AppIcon('pin-off', size: 12, color: textColor),
                const SizedBox(width: 6),
              ],
              AgentProviderIcon(
                provider: tab.provider,
                size: 14,
                color: textColor,
              ),
              const SizedBox(width: 6),
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 220),
                child: Text(
                  displayTitle,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(fontSize: 12, color: textColor),
                ),
              ),
              const SizedBox(width: 8),
              MouseRegion(
                cursor: SystemMouseCursors.click,
                onEnter: (_) => setState(() => _hoverClose = true),
                onExit: (_) => setState(() => _hoverClose = false),
                child: GestureDetector(
                  behavior: HitTestBehavior.opaque,
                  onTap: () => unawaited(_close()),
                  child: Container(
                    width: 18,
                    height: 18,
                    alignment: Alignment.center,
                    decoration: BoxDecoration(
                      color: _hoverClose
                          ? AppTokens.overlayHoverStrong
                          : Colors.transparent,
                      borderRadius: BorderRadius.circular(4),
                    ),
                    child: AppIcon(
                      'close',
                      size: 11,
                      color: _hoverClose
                          ? const Color(0xCCFFFFFF)
                          : const Color(0x73FFFFFF),
                    ),
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

class _AddAgentButton extends ConsumerStatefulWidget {
  const _AddAgentButton({required this.sectionId});
  final String sectionId;

  @override
  ConsumerState<_AddAgentButton> createState() => _AddAgentButtonState();
}

class _AddAgentButtonState extends ConsumerState<_AddAgentButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 4),
      child: Tooltip(
        message: 'Add an agent tab',
        child: MouseRegion(
          cursor: SystemMouseCursors.click,
          onEnter: (_) => setState(() => _hover = true),
          onExit: (_) => setState(() => _hover = false),
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: () async {
              final newTabId = await showAddAgentModal(
                context: context,
                sectionId: widget.sectionId,
              );
              if (!mounted || newTabId == null) return;
              await ref.read(localConnectionProvider).listProjects();
              ref
                  .read(selectedTabProvider.notifier)
                  .set(
                    TabSelection(sectionId: widget.sectionId, tabId: newTabId),
                  );
            },
            child: Container(
              width: 28,
              height: 28,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: _hover ? _tabHover : Colors.transparent,
                borderRadius: BorderRadius.circular(5),
              ),
              child: const AppIcon('plus', size: 14, color: Color(0x80FFFFFF)),
            ),
          ),
        ),
      ),
    );
  }
}

/// 364w confirmation modal for closing a pinned tab. Mirrors
/// `desktop/src/panels.rs::pinned_tab_close_confirm_modal`.
Future<bool> _showPinnedTabCloseConfirm(
  BuildContext context, {
  required String title,
}) async {
  final result = await showDialog<bool>(
    context: context,
    barrierColor: AppTokens.scrimBg,
    builder: (ctx) => Dialog(
      backgroundColor: Colors.transparent,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 364),
        child: Container(
          decoration: BoxDecoration(
            color: AppTokens.cardBg,
            borderRadius: BorderRadius.circular(AppTokens.radiusLg),
            border: Border.all(color: AppTokens.border),
            boxShadow: const [
              BoxShadow(
                color: Color(0x66000000),
                blurRadius: 16,
                offset: Offset(0, 6),
              ),
            ],
          ),
          clipBehavior: Clip.antiAlias,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(20, 20, 20, 14),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    const Text(
                      'Close pinned tab?',
                      style: TextStyle(
                        fontSize: 14,
                        fontWeight: FontWeight.w600,
                        color: AppTokens.textPrimary,
                      ),
                    ),
                    const SizedBox(height: 8),
                    Text(
                      'Close pinned tab "$title"? It will be removed from this task.',
                      style: const TextStyle(
                        fontSize: 12,
                        color: AppTokens.textSecondary,
                      ),
                    ),
                  ],
                ),
              ),
              Container(
                padding: const EdgeInsets.symmetric(
                  horizontal: 16,
                  vertical: 12,
                ),
                decoration: const BoxDecoration(
                  border: Border(top: BorderSide(color: AppTokens.divider)),
                ),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.end,
                  children: [
                    InkWell(
                      borderRadius: BorderRadius.circular(AppTokens.radiusMd),
                      onTap: () => Navigator.of(ctx).pop(false),
                      child: Container(
                        height: 28,
                        padding: const EdgeInsets.symmetric(horizontal: 12),
                        alignment: Alignment.center,
                        decoration: BoxDecoration(
                          borderRadius: BorderRadius.circular(
                            AppTokens.radiusMd,
                          ),
                          border: Border.all(color: AppTokens.border),
                        ),
                        child: const Text(
                          'Cancel',
                          style: TextStyle(
                            fontSize: 12,
                            fontWeight: FontWeight.w500,
                            color: AppTokens.textSecondary,
                          ),
                        ),
                      ),
                    ),
                    const SizedBox(width: 10),
                    InkWell(
                      borderRadius: BorderRadius.circular(AppTokens.radiusMd),
                      onTap: () => Navigator.of(ctx).pop(true),
                      child: Container(
                        height: 28,
                        padding: const EdgeInsets.symmetric(horizontal: 12),
                        alignment: Alignment.center,
                        decoration: BoxDecoration(
                          color: const Color(0xFFEB6F77),
                          borderRadius: BorderRadius.circular(
                            AppTokens.radiusMd,
                          ),
                        ),
                        child: const Text(
                          'Close',
                          style: TextStyle(
                            fontSize: 12,
                            fontWeight: FontWeight.w600,
                            color: Colors.white,
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    ),
  );
  return result ?? false;
}
