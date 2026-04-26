// New-task modal — full GPUI parity port of
// `desktop/src/new_task_modal.rs`.
//
// Visual surface (constants from the GPUI source):
// * Card 440w, max 90% viewport-h, rounded lg, bg #2b2d31.
// * Header row: title "New task" + project subtitle, X close.
// * Source-branch section: project chip, New/Existing toggle pill,
//   38h dropdown trigger + 36h filter input + scrollable branch list.
// * Task-name field with generated-name placeholder.
// * Agent multi-select: chip list reading from
//   `enabledAgentsProvider`, plus a "Terminal" sentinel (CLI-only).
// * Workspace toggle Worktree | Direct, with danger banner under Direct.
// * Advanced options collapsible: GitHub + Jira issue pickers
//   (placeholder strings — same as GPUI today).
// * Footer: Cancel + Create task. Submit calls
//   `LocalSession::submit_new_task`.

import 'dart:async';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../../rust/api/iroh_client.dart';
import '../../rust/api/local_session.dart' show AgentSummaryDto;
import '../../state/local_connection_provider.dart';
import '../../state/new_task_data_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';

Future<void> showNewTaskModal(
  BuildContext context, {
  required ProjectSummary project,
}) {
  return showDialog<void>(
    context: context,
    barrierColor: AppTokens.scrimBg,
    builder: (_) => _NewTaskModal(project: project),
  );
}

enum _BranchMode { newBranch, existingBranch }

class _NewTaskModal extends ConsumerStatefulWidget {
  const _NewTaskModal({required this.project});
  final ProjectSummary project;

  @override
  ConsumerState<_NewTaskModal> createState() => _NewTaskModalState();
}

class _NewTaskModalState extends ConsumerState<_NewTaskModal> {
  static const double _cardW = 440;

  late final TextEditingController _taskNameCtrl;
  late final String _generatedTaskName;
  final TextEditingController _branchFilterCtrl = TextEditingController();

  String _sourceBranch = '';
  _BranchMode _branchMode = _BranchMode.newBranch;
  bool _branchDropdownOpen = false;
  final Set<String> _selectedAgents = {};
  bool _agentsSeeded = false;
  bool _worktreeMode = true;
  bool _advancedExpanded = false;
  bool _submitting = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _taskNameCtrl = TextEditingController();
    _generatedTaskName = _generateTaskName();
    _sourceBranch = widget.project.currentBranch ?? '';
  }

  @override
  void dispose() {
    _taskNameCtrl.dispose();
    _branchFilterCtrl.dispose();
    super.dispose();
  }

  void _seedDefaults(List<String> branches, AgentSummaryDto? defaultAgent) {
    if (_sourceBranch.isEmpty && branches.isNotEmpty) {
      _sourceBranch = branches.first;
    }
    if (!_agentsSeeded && defaultAgent != null) {
      _selectedAgents.add(defaultAgent.id);
      _agentsSeeded = true;
    }
  }

  Future<void> _submit() async {
    if (_submitting) return;
    final taskName = _taskNameCtrl.text.trim();
    final effectiveTaskName =
        taskName.isEmpty ? _generatedTaskName : taskName;
    if (_sourceBranch.trim().isEmpty) {
      setState(() => _error = 'Source branch is required');
      return;
    }
    setState(() {
      _submitting = true;
      _error = null;
    });
    try {
      final connection = ref.read(localConnectionProvider);
      final sectionId = await connection.submitNewTask(
        projectId: widget.project.id,
        taskName: effectiveTaskName,
        sourceBranch: _sourceBranch,
        agentIds: _selectedAgents.toList(),
        branchModeExisting: _branchMode == _BranchMode.existingBranch,
        worktreeMode: _worktreeMode,
      );
      // Refresh project list so the new task appears in the sidebar.
      await connection.listProjects();
      ref.read(selectedTabProvider.notifier).set(
            TabSelection(sectionId: sectionId, tabId: '0'),
          );
      if (!mounted) return;
      Navigator.of(context).pop();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _submitting = false;
        _error = '$e';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final branchesAsync =
        ref.watch(projectBranchesProvider(widget.project.id));
    final agentsAsync = ref.watch(enabledAgentsProvider);
    final branches = branchesAsync.valueOrNull ?? const <String>[];
    final agents = agentsAsync.valueOrNull;
    if (agents != null) {
      final defaultAgent = agents.defaultAgentId == null
          ? null
          : agents.agents.firstWhere(
              (a) => a.id == agents.defaultAgentId,
              orElse: () => agents.agents.isNotEmpty
                  ? agents.agents.first
                  : const AgentSummaryDto(
                      id: '',
                      label: '',
                      iconPath: '',
                    ),
            );
      _seedDefaults(branches, defaultAgent);
    }

    return Shortcuts(
      shortcuts: const <ShortcutActivator, Intent>{
        SingleActivator(LogicalKeyboardKey.escape): _DismissIntent(),
      },
      child: Actions(
        actions: <Type, Action<Intent>>{
          _DismissIntent: CallbackAction<_DismissIntent>(
            onInvoke: (_) {
              if (!_submitting) Navigator.of(context).pop();
              return null;
            },
          ),
        },
        child: Focus(
          autofocus: true,
          child: Center(
            child: ConstrainedBox(
              constraints: BoxConstraints(
                maxWidth: _cardW,
                maxHeight: MediaQuery.of(context).size.height * 0.9,
              ),
              child: Material(
                color: Colors.transparent,
                child: Container(
                  decoration: BoxDecoration(
                    color: AppTokens.cardBg,
                    borderRadius: BorderRadius.circular(AppTokens.radiusLg),
                    border: Border.all(color: AppTokens.border),
                    boxShadow: const [
                      BoxShadow(
                        color: Color(0x66000000),
                        blurRadius: 20,
                        offset: Offset(0, 8),
                      ),
                    ],
                  ),
                  clipBehavior: Clip.antiAlias,
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      _Header(
                        projectName: widget.project.name,
                        onClose: _submitting
                            ? null
                            : () => Navigator.of(context).pop(),
                      ),
                      Flexible(
                        child: SingleChildScrollView(
                          padding: const EdgeInsets.symmetric(vertical: 4),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              _SourceBranchSection(
                                branches: branches,
                                currentBranch: widget.project.currentBranch,
                                selected: _sourceBranch,
                                branchMode: _branchMode,
                                worktreeMode: _worktreeMode,
                                dropdownOpen: _branchDropdownOpen,
                                filterCtrl: _branchFilterCtrl,
                                submitting: _submitting,
                                onBranchSelected: (b) => setState(() {
                                  _sourceBranch = b;
                                  _branchDropdownOpen = false;
                                }),
                                onModeChanged: _submitting
                                    ? null
                                    : (m) =>
                                        setState(() => _branchMode = m),
                                onToggleDropdown: () => setState(() {
                                  _branchDropdownOpen =
                                      !_branchDropdownOpen;
                                }),
                                onTypeBranch: (text) {
                                  if (_branchMode ==
                                      _BranchMode.newBranch) {
                                    setState(() => _sourceBranch = text);
                                  }
                                },
                              ),
                              _TaskNameField(
                                controller: _taskNameCtrl,
                                placeholder: _generatedTaskName,
                                enabled: !_submitting,
                              ),
                              _AgentMultiSelect(
                                agents: agents?.agents ?? const [],
                                selected: _selectedAgents,
                                submitting: _submitting,
                                onToggle: (id) => setState(() {
                                  if (_selectedAgents.contains(id)) {
                                    _selectedAgents.remove(id);
                                  } else {
                                    _selectedAgents.add(id);
                                  }
                                }),
                              ),
                              _WorkspaceToggle(
                                worktreeMode: _worktreeMode,
                                submitting: _submitting,
                                onChanged: (worktree) => setState(() {
                                  _worktreeMode = worktree;
                                  _branchDropdownOpen = false;
                                }),
                              ),
                              _AdvancedOptions(
                                expanded: _advancedExpanded,
                                submitting: _submitting,
                                onToggle: () => setState(() {
                                  _advancedExpanded = !_advancedExpanded;
                                  _branchDropdownOpen = false;
                                }),
                              ),
                              if (_error != null) _ErrorBanner(text: _error!),
                              const SizedBox(height: 12),
                            ],
                          ),
                        ),
                      ),
                      _Footer(
                        submitting: _submitting,
                        onCancel: _submitting
                            ? null
                            : () => Navigator.of(context).pop(),
                        onSubmit: _submitting ? null : _submit,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _DismissIntent extends Intent {
  const _DismissIntent();
}

class _Header extends StatelessWidget {
  const _Header({required this.projectName, required this.onClose});
  final String projectName;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 12),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  'New task',
                  style: TextStyle(
                    fontSize: 16,
                    fontWeight: FontWeight.w600,
                    color: AppTokens.textPrimary,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  'in $projectName',
                  style: const TextStyle(
                    fontSize: 12,
                    color: AppTokens.textMuted,
                  ),
                ),
              ],
            ),
          ),
          InkWell(
            borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            onTap: onClose,
            child: SizedBox(
              width: 24,
              height: 24,
              child: Center(
                child: SvgPicture.asset(
                  'assets/icons/icons__close.svg',
                  width: 14,
                  height: 14,
                  colorFilter: const ColorFilter.mode(
                    AppTokens.textMuted,
                    BlendMode.srcIn,
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _SectionLabel extends StatelessWidget {
  const _SectionLabel(this.text);
  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 6),
      child: Text(
        text,
        style: const TextStyle(
          fontSize: 12,
          fontWeight: FontWeight.w600,
          color: AppTokens.textPrimary,
        ),
      ),
    );
  }
}

class _SourceBranchSection extends StatelessWidget {
  const _SourceBranchSection({
    required this.branches,
    required this.currentBranch,
    required this.selected,
    required this.branchMode,
    required this.worktreeMode,
    required this.dropdownOpen,
    required this.filterCtrl,
    required this.submitting,
    required this.onBranchSelected,
    required this.onModeChanged,
    required this.onToggleDropdown,
    required this.onTypeBranch,
  });

  final List<String> branches;
  final String? currentBranch;
  final String selected;
  final _BranchMode branchMode;
  final bool worktreeMode;
  final bool dropdownOpen;
  final TextEditingController filterCtrl;
  final bool submitting;
  final ValueChanged<String> onBranchSelected;
  final ValueChanged<_BranchMode>? onModeChanged;
  final VoidCallback onToggleDropdown;
  final ValueChanged<String> onTypeBranch;

  bool get _isNewBranch => branchMode == _BranchMode.newBranch;

  @override
  Widget build(BuildContext context) {
    final branchModeAvailable = worktreeMode;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const _SectionLabel('Source branch'),
        if (branchModeAvailable)
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 0, 20, 8),
            child: _BranchModeToggle(
              mode: branchMode,
              onChanged: onModeChanged,
            ),
          ),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 20),
          child: _BranchTrigger(
            label: selected.isEmpty
                ? (currentBranch ?? 'Pick a branch')
                : selected,
            isNewBranch: _isNewBranch,
            open: dropdownOpen,
            onTap: submitting ? null : onToggleDropdown,
            onTypeBranch: _isNewBranch ? onTypeBranch : null,
          ),
        ),
        if (dropdownOpen)
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 6, 20, 0),
            child: _BranchDropdown(
              branches: branches,
              filterCtrl: filterCtrl,
              selected: selected,
              onBranchSelected: onBranchSelected,
            ),
          ),
      ],
    );
  }
}

class _BranchModeToggle extends StatelessWidget {
  const _BranchModeToggle({required this.mode, required this.onChanged});
  final _BranchMode mode;
  final ValueChanged<_BranchMode>? onChanged;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(3),
      decoration: BoxDecoration(
        color: AppTokens.overlayHover,
        borderRadius: BorderRadius.circular(AppTokens.radiusSm),
      ),
      child: Row(
        children: [
          _ModeChip(
            label: 'New branch',
            selected: mode == _BranchMode.newBranch,
            onTap: onChanged == null
                ? null
                : () => onChanged!(_BranchMode.newBranch),
          ),
          const SizedBox(width: 2),
          _ModeChip(
            label: 'Existing',
            selected: mode == _BranchMode.existingBranch,
            onTap: onChanged == null
                ? null
                : () => onChanged!(_BranchMode.existingBranch),
          ),
        ],
      ),
    );
  }
}

class _ModeChip extends StatelessWidget {
  const _ModeChip({
    required this.label,
    required this.selected,
    required this.onTap,
  });
  final String label;
  final bool selected;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return Expanded(
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.radiusXs + 1),
        onTap: onTap,
        child: Container(
          height: 28,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color:
                selected ? AppTokens.overlayActive : Colors.transparent,
            borderRadius: BorderRadius.circular(AppTokens.radiusXs + 1),
          ),
          child: Text(
            label,
            style: const TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w500,
              color: AppTokens.textPrimary,
            ),
          ),
        ),
      ),
    );
  }
}

class _BranchTrigger extends StatelessWidget {
  const _BranchTrigger({
    required this.label,
    required this.isNewBranch,
    required this.open,
    required this.onTap,
    required this.onTypeBranch,
  });

  final String label;
  final bool isNewBranch;
  final bool open;
  final VoidCallback? onTap;
  final ValueChanged<String>? onTypeBranch;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 38,
      decoration: BoxDecoration(
        color: AppTokens.overlayHover,
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        border: Border.all(
          color: open ? const Color(0xFF6F86CB) : AppTokens.border,
        ),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 12),
      child: Row(
        children: [
          SvgPicture.asset(
            'assets/icons/icons__git-branch.svg',
            width: 14,
            height: 14,
            colorFilter: const ColorFilter.mode(
              AppTokens.textMuted,
              BlendMode.srcIn,
            ),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: onTap,
              child: Text(
                label,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(
                  fontSize: 13,
                  color: AppTokens.textPrimary,
                ),
              ),
            ),
          ),
          GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: onTap,
            child: SvgPicture.asset(
              'assets/icons/icons__chevron-down.svg',
              width: 12,
              height: 12,
              colorFilter: const ColorFilter.mode(
                AppTokens.textMuted,
                BlendMode.srcIn,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _BranchDropdown extends StatefulWidget {
  const _BranchDropdown({
    required this.branches,
    required this.filterCtrl,
    required this.selected,
    required this.onBranchSelected,
  });

  final List<String> branches;
  final TextEditingController filterCtrl;
  final String selected;
  final ValueChanged<String> onBranchSelected;

  @override
  State<_BranchDropdown> createState() => _BranchDropdownState();
}

class _BranchDropdownState extends State<_BranchDropdown> {
  String _filter = '';

  @override
  void initState() {
    super.initState();
    widget.filterCtrl.addListener(_onFilterChanged);
  }

  @override
  void dispose() {
    widget.filterCtrl.removeListener(_onFilterChanged);
    super.dispose();
  }

  void _onFilterChanged() {
    setState(() => _filter = widget.filterCtrl.text);
  }

  @override
  Widget build(BuildContext context) {
    final filterLower = _filter.trim().toLowerCase();
    final filtered = filterLower.isEmpty
        ? widget.branches
        : widget.branches
            .where((b) => b.toLowerCase().contains(filterLower))
            .toList();
    return Container(
      decoration: BoxDecoration(
        color: AppTokens.cardBg,
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        border: Border.all(color: AppTokens.border),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(10, 8, 10, 4),
            child: TextField(
              controller: widget.filterCtrl,
              autofocus: true,
              style: const TextStyle(
                fontSize: 13,
                color: AppTokens.textPrimary,
              ),
              decoration: InputDecoration(
                isDense: true,
                contentPadding: const EdgeInsets.symmetric(
                  vertical: 6,
                  horizontal: 8,
                ),
                hintText: 'Filter branches…',
                hintStyle: const TextStyle(
                  fontSize: 13,
                  color: AppTokens.textMuted,
                ),
                filled: true,
                fillColor: AppTokens.overlayHover,
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(6),
                  borderSide: const BorderSide(color: AppTokens.border),
                ),
                enabledBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(6),
                  borderSide: const BorderSide(color: AppTokens.border),
                ),
                focusedBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(6),
                  borderSide: const BorderSide(color: Color(0xFF6F86CB)),
                ),
              ),
            ),
          ),
          ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 200),
            child: ListView(
              shrinkWrap: true,
              padding: const EdgeInsets.symmetric(vertical: 4),
              children: [
                for (final branch in filtered)
                  _BranchRow(
                    branch: branch,
                    selected: branch == widget.selected,
                    onTap: () {
                      widget.filterCtrl.clear();
                      widget.onBranchSelected(branch);
                    },
                  ),
                if (filtered.isEmpty)
                  const Padding(
                    padding: EdgeInsets.symmetric(
                      vertical: 12,
                      horizontal: 12,
                    ),
                    child: Text(
                      'No branches match.',
                      style: TextStyle(
                        fontSize: 12,
                        color: AppTokens.textMuted,
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _BranchRow extends StatefulWidget {
  const _BranchRow({
    required this.branch,
    required this.selected,
    required this.onTap,
  });
  final String branch;
  final bool selected;
  final VoidCallback onTap;

  @override
  State<_BranchRow> createState() => _BranchRowState();
}

class _BranchRowState extends State<_BranchRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          height: 30,
          padding: const EdgeInsets.symmetric(horizontal: 10),
          color: widget.selected
              ? AppTokens.overlayActive
              : (_hover ? AppTokens.overlayHover : Colors.transparent),
          alignment: Alignment.centerLeft,
          child: Text(
            widget.branch,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(
              fontFamily: AppTokens.fontFamilyMono,
              fontSize: 12,
              color: AppTokens.textPrimary,
            ),
          ),
        ),
      ),
    );
  }
}

class _TaskNameField extends StatelessWidget {
  const _TaskNameField({
    required this.controller,
    required this.placeholder,
    required this.enabled,
  });
  final TextEditingController controller;
  final String placeholder;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 6, 20, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const _SectionLabelInline(text: 'Task name'),
          const SizedBox(height: 6),
          Container(
            decoration: BoxDecoration(
              color: AppTokens.overlayHover,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              border: Border.all(color: AppTokens.border),
            ),
            padding: const EdgeInsets.symmetric(horizontal: 12),
            child: TextField(
              controller: controller,
              autofocus: true,
              enabled: enabled,
              style: const TextStyle(
                fontSize: 13,
                color: AppTokens.textPrimary,
              ),
              cursorColor: AppTokens.textPrimary,
              decoration: InputDecoration(
                isDense: true,
                contentPadding: const EdgeInsets.symmetric(vertical: 11),
                border: InputBorder.none,
                hintText: placeholder,
                hintStyle: const TextStyle(
                  fontSize: 13,
                  color: AppTokens.textMuted,
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _SectionLabelInline extends StatelessWidget {
  const _SectionLabelInline({required this.text});
  final String text;

  @override
  Widget build(BuildContext context) {
    return Text(
      text,
      style: const TextStyle(
        fontSize: 12,
        fontWeight: FontWeight.w600,
        color: AppTokens.textPrimary,
      ),
    );
  }
}

class _AgentMultiSelect extends StatelessWidget {
  const _AgentMultiSelect({
    required this.agents,
    required this.selected,
    required this.submitting,
    required this.onToggle,
  });
  final List<AgentSummaryDto> agents;
  final Set<String> selected;
  final bool submitting;
  final ValueChanged<String> onToggle;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const _SectionLabelInline(text: 'Agents'),
          const SizedBox(height: 8),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            children: [
              for (final agent in agents)
                _AgentChip(
                  agent: agent,
                  selected: selected.contains(agent.id),
                  onTap:
                      submitting ? null : () => onToggle(agent.id),
                ),
              _TerminalChip(
                selected: selected.isEmpty,
                onTap: submitting
                    ? null
                    : () {
                        // Terminal sentinel: clears every agent
                        // selection. Equivalent to GPUI's CLI Only
                        // entry which resolves to an empty
                        // agent_ids set + default launch_config.
                        for (final id in selected.toList()) {
                          onToggle(id);
                        }
                      },
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _AgentChip extends StatelessWidget {
  const _AgentChip({
    required this.agent,
    required this.selected,
    required this.onTap,
  });
  final AgentSummaryDto agent;
  final bool selected;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.radiusPill),
      onTap: onTap,
      child: Container(
        height: 30,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        alignment: Alignment.center,
        decoration: BoxDecoration(
          color: selected
              ? AppTokens.overlayActive
              : AppTokens.overlayRest,
          borderRadius: BorderRadius.circular(AppTokens.radiusPill),
          border: Border.all(
            color:
                selected ? const Color(0xFF6F86CB) : AppTokens.border,
          ),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (agent.iconPath.endsWith('.svg'))
              SvgPicture.asset(
                agent.iconPath,
                width: 13,
                height: 13,
                colorFilter: const ColorFilter.mode(
                  AppTokens.textPrimary,
                  BlendMode.srcIn,
                ),
              )
            else
              const Icon(Icons.smart_toy, size: 13, color: AppTokens.textPrimary),
            const SizedBox(width: 6),
            Text(
              agent.label,
              style: const TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w500,
                color: AppTokens.textPrimary,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _TerminalChip extends StatelessWidget {
  const _TerminalChip({required this.selected, required this.onTap});
  final bool selected;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.radiusPill),
      onTap: onTap,
      child: Container(
        height: 30,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        alignment: Alignment.center,
        decoration: BoxDecoration(
          color: selected
              ? AppTokens.overlayActive
              : AppTokens.overlayRest,
          borderRadius: BorderRadius.circular(AppTokens.radiusPill),
          border: Border.all(
            color:
                selected ? const Color(0xFF6F86CB) : AppTokens.border,
          ),
        ),
        child: const Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.terminal, size: 13, color: AppTokens.textPrimary),
            SizedBox(width: 6),
            Text(
              'Terminal',
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w500,
                color: AppTokens.textPrimary,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _WorkspaceToggle extends StatelessWidget {
  const _WorkspaceToggle({
    required this.worktreeMode,
    required this.submitting,
    required this.onChanged,
  });
  final bool worktreeMode;
  final bool submitting;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const _SectionLabelInline(text: 'Workspace'),
          const SizedBox(height: 8),
          Container(
            padding: const EdgeInsets.all(3),
            decoration: BoxDecoration(
              color: AppTokens.overlayHover,
              borderRadius: BorderRadius.circular(AppTokens.radiusSm),
            ),
            child: Row(
              children: [
                Expanded(
                  child: _WorkspaceOption(
                    iconPath: 'assets/icons/icons__git-worktree.svg',
                    label: 'Worktree',
                    selected: worktreeMode,
                    enabled: !submitting,
                    tooltip:
                        'Create a sibling worktree for this task',
                    onTap: () => onChanged(true),
                  ),
                ),
                const SizedBox(width: 2),
                Expanded(
                  child: _WorkspaceOption(
                    iconPath: 'assets/icons/icons__folder-plus.svg',
                    label: 'Direct',
                    selected: !worktreeMode,
                    enabled: !submitting,
                    tooltip:
                        'Open the original project directory directly',
                    onTap: () => onChanged(false),
                  ),
                ),
              ],
            ),
          ),
          if (!worktreeMode)
            const Padding(
              padding: EdgeInsets.only(top: 6),
              child: Text(
                'Direct uses the branch already checked out in the original project.',
                style: TextStyle(
                  fontSize: 11,
                  // hsla(0.0, 0.78, 0.68, 1.) → ~ #EB5959
                  color: Color(0xFFEB6F77),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _WorkspaceOption extends StatelessWidget {
  const _WorkspaceOption({
    required this.iconPath,
    required this.label,
    required this.selected,
    required this.enabled,
    required this.tooltip,
    required this.onTap,
  });
  final String iconPath;
  final String label;
  final bool selected;
  final bool enabled;
  final String tooltip;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip,
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.radiusXs + 1),
        onTap: enabled ? onTap : null,
        child: Container(
          height: 32,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color:
                selected ? AppTokens.overlayActive : Colors.transparent,
            borderRadius: BorderRadius.circular(AppTokens.radiusXs + 1),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              SvgPicture.asset(
                iconPath,
                width: 14,
                height: 14,
                colorFilter: ColorFilter.mode(
                  selected
                      ? AppTokens.textPrimary
                      : AppTokens.textMuted,
                  BlendMode.srcIn,
                ),
              ),
              const SizedBox(width: 6),
              Text(
                label,
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w500,
                  color: selected
                      ? AppTokens.textPrimary
                      : AppTokens.textMuted,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _AdvancedOptions extends StatelessWidget {
  const _AdvancedOptions({
    required this.expanded,
    required this.submitting,
    required this.onToggle,
  });
  final bool expanded;
  final bool submitting;
  final VoidCallback onToggle;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          InkWell(
            borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            onTap: submitting ? null : onToggle,
            child: Container(
              height: 40,
              padding: const EdgeInsets.symmetric(horizontal: 14),
              decoration: BoxDecoration(
                color: AppTokens.overlayHover,
                borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              ),
              child: Row(
                children: [
                  SvgPicture.asset(
                    'assets/icons/icons__settings.svg',
                    width: 15,
                    height: 15,
                    colorFilter: const ColorFilter.mode(
                      AppTokens.textMuted,
                      BlendMode.srcIn,
                    ),
                  ),
                  const SizedBox(width: 8),
                  const Expanded(
                    child: Text(
                      'Advanced options',
                      style: TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w500,
                        color: AppTokens.textSecondary,
                      ),
                    ),
                  ),
                  SvgPicture.asset(
                    expanded
                        ? 'assets/icons/icons__chevron-up.svg'
                        : 'assets/icons/icons__chevron-down.svg',
                    width: 11,
                    height: 11,
                    colorFilter: const ColorFilter.mode(
                      AppTokens.textMuted,
                      BlendMode.srcIn,
                    ),
                  ),
                ],
              ),
            ),
          ),
          if (expanded)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Column(
                children: [
                  _IssuePickerRow(
                    label: 'GitHub issue',
                    iconPath: 'assets/icons/icons__github.svg',
                    placeholder: 'Select a GitHub issue',
                  ),
                  const SizedBox(height: 12),
                  const _JiraPickerRow(),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

class _IssuePickerRow extends StatelessWidget {
  const _IssuePickerRow({
    required this.label,
    required this.iconPath,
    required this.placeholder,
  });
  final String label;
  final String iconPath;
  final String placeholder;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        SizedBox(
          width: 100,
          child: Text(
            label,
            style: const TextStyle(
              fontSize: 13,
              fontWeight: FontWeight.w600,
              color: AppTokens.textPrimary,
            ),
          ),
        ),
        const SizedBox(width: 12),
        Expanded(
          child: Container(
            height: 36,
            padding: const EdgeInsets.symmetric(horizontal: 10),
            decoration: BoxDecoration(
              color: AppTokens.overlayHover,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              border: Border.all(color: AppTokens.border),
            ),
            child: Row(
              children: [
                SvgPicture.asset(
                  iconPath,
                  width: 16,
                  height: 16,
                  colorFilter: const ColorFilter.mode(
                    AppTokens.textMuted,
                    BlendMode.srcIn,
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    placeholder,
                    style: const TextStyle(
                      fontSize: 12,
                      color: AppTokens.textPlaceholder,
                    ),
                  ),
                ),
                SvgPicture.asset(
                  'assets/icons/icons__chevron-down.svg',
                  width: 11,
                  height: 11,
                  colorFilter: const ColorFilter.mode(
                    AppTokens.textMuted,
                    BlendMode.srcIn,
                  ),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

class _JiraPickerRow extends StatelessWidget {
  const _JiraPickerRow();

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        const SizedBox(
          width: 100,
          child: Text(
            'Jira issue',
            style: TextStyle(
              fontSize: 13,
              fontWeight: FontWeight.w600,
              color: AppTokens.textPrimary,
            ),
          ),
        ),
        const SizedBox(width: 12),
        Expanded(
          child: Container(
            height: 36,
            padding: const EdgeInsets.symmetric(horizontal: 10),
            decoration: BoxDecoration(
              color: AppTokens.overlayHover,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              border: Border.all(color: AppTokens.border),
            ),
            child: Row(
              children: [
                Container(
                  width: 16,
                  height: 16,
                  alignment: Alignment.center,
                  decoration: BoxDecoration(
                    // hsla(220/360, 0.65, 0.52, 1.) ≈ #2D85DD
                    color: const Color(0xFF2D85DD),
                    borderRadius: BorderRadius.circular(3),
                  ),
                  child: const Text(
                    'J',
                    style: TextStyle(
                      fontSize: 9,
                      fontWeight: FontWeight.bold,
                      color: Colors.white,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                const Expanded(
                  child: Text(
                    'Select a Jira issue',
                    style: TextStyle(
                      fontSize: 12,
                      color: AppTokens.textPlaceholder,
                    ),
                  ),
                ),
                SvgPicture.asset(
                  'assets/icons/icons__chevron-down.svg',
                  width: 11,
                  height: 11,
                  colorFilter: const ColorFilter.mode(
                    AppTokens.textMuted,
                    BlendMode.srcIn,
                  ),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

class _ErrorBanner extends StatelessWidget {
  const _ErrorBanner({required this.text});
  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 0),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        decoration: BoxDecoration(
          color: AppTokens.errorBg,
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        ),
        child: Text(
          text,
          style: const TextStyle(
            fontSize: 12,
            color: AppTokens.errorText,
          ),
        ),
      ),
    );
  }
}

class _Footer extends StatelessWidget {
  const _Footer({
    required this.submitting,
    required this.onCancel,
    required this.onSubmit,
  });
  final bool submitting;
  final VoidCallback? onCancel;
  final VoidCallback? onSubmit;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 14),
      decoration: const BoxDecoration(
        border: Border(top: BorderSide(color: AppTokens.border)),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.end,
        children: [
          InkWell(
            borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            onTap: onCancel,
            child: Container(
              height: 32,
              padding: const EdgeInsets.symmetric(horizontal: 14),
              alignment: Alignment.center,
              child: const Text(
                'Cancel',
                style: TextStyle(
                  fontSize: 13,
                  color: AppTokens.textPrimary,
                ),
              ),
            ),
          ),
          const SizedBox(width: 8),
          InkWell(
            borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            onTap: onSubmit,
            child: Container(
              height: 32,
              padding: const EdgeInsets.symmetric(horizontal: 14),
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: const Color(0xFF6F86CB),
                borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              ),
              child: submitting
                  ? const SizedBox(
                      width: 14,
                      height: 14,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        valueColor:
                            AlwaysStoppedAnimation<Color>(Colors.white),
                      ),
                    )
                  : const Text(
                      'Create task',
                      style: TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        color: Colors.white,
                      ),
                    ),
            ),
          ),
        ],
      ),
    );
  }
}

// ── Generated task name ─────────────────────────────────────────
//
// Direct port of `desktop/src/new_task_modal.rs::generate_task_name`.
// Picks one of (FIRST × SECOND × THIRD = 21,952) word combos or a
// canned phrase. UUID-equivalent randomness comes from `Random()`.

const _firstWords = <String>[
  'quiet', 'silver', 'bright', 'steady', 'wild', 'mellow', 'brisk',
  'neat', 'rad', 'fresh', 'fly', 'phat', 'pixel', 'neon', 'grunge',
  'turbo', 'cosmic', 'mall', 'arcade', 'saturday', 'vhs', 'tamagotchi',
  'zelda', 'sonic', 'clueless', 'spice', 'matrix', 'dialup',
];

const _secondWords = <String>[
  'river', 'meadow', 'comet', 'signal', 'forest', 'ember', 'harbor',
  'planet', 'sitcom', 'beeper', 'rewind', 'moonwalk', 'blockbuster',
  'gameboy', 'dreamcast', 'trapper', 'windbreaker', 'discman',
  'boyband', 'chatroom', 'supernova', 'slammer', 'ranger', 'tamagotchi',
  'seinfeld', 'xfiles', 'jukebox', 'afterparty',
];

const _thirdWords = <String>[
  'sparks', 'travels', 'builds', 'drifts', 'guides', 'moves', 'lands',
  'echoes', 'remix', 'rewinds', 'glows', 'bounces', 'rips', 'slaps',
  'glitches', 'downloads', 'pages', 'beams', 'boogies', 'radicals',
  'jams', 'surfs', 'shuffles', 'blasts', 'hangs', 'grooves', 'rules',
  'zooms',
];

const _phrases = <String>[
  'you-sure-about-that', 'why-the-tables', 'coffin-flop', 'corncob-tv',
  'sloppy-steaks', 'lets-slop-em-up', 'white-ferrari', 'ghost-tour',
  'santa-brought-it-early', 'baby-of-the-year', 'karl-havoc', 'im-so-hot',
  'dan-flashes', 'jamie-taco', 'gimme-dat', 'brians-hat', 'turbo-team',
  'tc-tuggers', 'calico-cut-pants', 'its-not-a-joke', 'you-gotta-give',
  'motorcycle-guys',
];

String _generateTaskName() {
  final combo = _firstWords.length * _secondWords.length * _thirdWords.length;
  final total = combo + _phrases.length;
  final pick = Random().nextInt(total);
  if (pick < combo) {
    final third = _thirdWords.length;
    final secondThird = _secondWords.length * third;
    final first = pick ~/ secondThird;
    final remainder = pick % secondThird;
    final second = remainder ~/ third;
    final t = remainder % third;
    return '${_firstWords[first]}-${_secondWords[second]}-${_thirdWords[t]}';
  }
  return _phrases[pick - combo];
}
