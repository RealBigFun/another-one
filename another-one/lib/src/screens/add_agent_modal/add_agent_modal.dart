// Add Agent to Task modal — port of
// `desktop/src/add_agent_modal.rs`.
//
// Lets the user open a second agent (or plain Terminal) inside an
// existing task without changing the worktree. Single-select
// dropdown of enabled agents + a Terminal sentinel; on submit the
// bridge appends a new tab to the section and queues its PTY
// launch.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../../rust/api/local_session.dart' show AgentSummaryDto;
import '../../state/local_connection_provider.dart';
import '../../state/new_task_data_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_toast.dart';

Future<bool> showAddAgentModal({
  required BuildContext context,
  required String sectionId,
  String? seededAgentId,
}) async {
  final result = await showDialog<bool>(
    context: context,
    barrierColor: AppTokens.scrimBg,
    builder: (_) =>
        _AddAgentModal(sectionId: sectionId, seededAgentId: seededAgentId),
  );
  return result ?? false;
}

class _AddAgentModal extends ConsumerStatefulWidget {
  const _AddAgentModal({required this.sectionId, required this.seededAgentId});
  final String sectionId;
  final String? seededAgentId;

  @override
  ConsumerState<_AddAgentModal> createState() => _AddAgentModalState();
}

class _AddAgentModalState extends ConsumerState<_AddAgentModal> {
  static const double _cardW = 440;

  String? _selectedAgentId;
  bool _dropdownOpen = false;
  bool _submitting = false;
  bool _seeded = false;

  String? _resolvedSelection(
    String? selectedAgentId,
    String? fallbackAgentId,
    List<AgentSummaryDto> agents,
  ) {
    if (selectedAgentId == null) return null;
    if (agents.any((agent) => agent.id == selectedAgentId)) {
      return selectedAgentId;
    }
    if (fallbackAgentId != null &&
        agents.any((agent) => agent.id == fallbackAgentId)) {
      return fallbackAgentId;
    }
    return agents.isNotEmpty ? agents.first.id : null;
  }

  String? _initialSelection(
    List<AgentSummaryDto> agents,
    String? defaultAgentId,
  ) {
    final seeded = widget.seededAgentId;
    return _resolvedSelection(seeded, defaultAgentId, agents);
  }

  void _syncSelection(List<AgentSummaryDto> agents, String? defaultAgentId) {
    _selectedAgentId = _seeded
        ? _resolvedSelection(_selectedAgentId, defaultAgentId, agents)
        : _initialSelection(agents, defaultAgentId);
    _seeded = true;
  }

  Future<void> _submit() async {
    if (_submitting) return;
    final agentsView = ref.read(enabledAgentsProvider).valueOrNull;
    final selectedAgentId = agentsView == null
        ? _selectedAgentId
        : _resolvedSelection(
            _selectedAgentId,
            agentsView.defaultAgentId,
            agentsView.agents,
          );

    setState(() {
      _selectedAgentId = selectedAgentId;
      _submitting = true;
    });
    try {
      await ref
          .read(localConnectionProvider)
          .addAgentToSection(
            sectionId: widget.sectionId,
            agentId: selectedAgentId ?? '',
          );
      if (!mounted) return;
      Navigator.of(context).pop(true);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _submitting = false;
      });
      showAppToast(context, message: 'Could not add agent tab: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final agentsAsync = ref.watch(enabledAgentsProvider);
    final view = agentsAsync.valueOrNull;
    if (view != null) {
      _syncSelection(view.agents, view.defaultAgentId);
    }
    return PopScope(
      canPop: !_submitting,
      child: Shortcuts(
        shortcuts: const <ShortcutActivator, Intent>{
          SingleActivator(LogicalKeyboardKey.escape): _DismissIntent(),
          SingleActivator(LogicalKeyboardKey.enter): _SubmitIntent(),
        },
        child: Actions(
          actions: <Type, Action<Intent>>{
            _DismissIntent: CallbackAction<_DismissIntent>(
              onInvoke: (_) {
                if (!_submitting) Navigator.of(context).pop(false);
                return null;
              },
            ),
            _SubmitIntent: CallbackAction<_SubmitIntent>(
              onInvoke: (_) {
                unawaited(_submit());
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
                          onClose: _submitting
                              ? null
                              : () => Navigator.of(context).pop(false),
                        ),
                        Flexible(
                          child: SingleChildScrollView(
                            child: Padding(
                              padding: const EdgeInsets.fromLTRB(20, 4, 20, 16),
                              child: _AgentPicker(
                                agents: view?.agents ?? const [],
                                selectedId: _selectedAgentId,
                                dropdownOpen: _dropdownOpen,
                                onToggleDropdown: _submitting
                                    ? null
                                    : () => setState(
                                        () => _dropdownOpen = !_dropdownOpen,
                                      ),
                                onSelect: _submitting
                                    ? null
                                    : (id) => setState(() {
                                        _selectedAgentId = id;
                                        _dropdownOpen = false;
                                      }),
                              ),
                            ),
                          ),
                        ),
                        _Footer(
                          submitting: _submitting,
                          onCancel: _submitting
                              ? null
                              : () => Navigator.of(context).pop(false),
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
      ),
    );
  }
}

class _DismissIntent extends Intent {
  const _DismissIntent();
}

class _SubmitIntent extends Intent {
  const _SubmitIntent();
}

class _Header extends StatelessWidget {
  const _Header({required this.onClose});
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 20, 20, 12),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Add Agent to Task',
                  style: TextStyle(
                    fontSize: 16,
                    fontWeight: FontWeight.w600,
                    color: AppTokens.textPrimary,
                  ),
                ),
                SizedBox(height: 4),
                Text(
                  'Open another agent chat in the same task without changing the worktree.',
                  style: TextStyle(fontSize: 12, color: AppTokens.textMuted),
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

class _AgentPicker extends StatelessWidget {
  const _AgentPicker({
    required this.agents,
    required this.selectedId,
    required this.dropdownOpen,
    required this.onToggleDropdown,
    required this.onSelect,
  });

  final List<AgentSummaryDto> agents;
  final String? selectedId;
  final bool dropdownOpen;
  final VoidCallback? onToggleDropdown;
  final ValueChanged<String?>? onSelect;

  @override
  Widget build(BuildContext context) {
    AgentSummaryDto? selected;
    if (selectedId != null) {
      for (final agent in agents) {
        if (agent.id == selectedId) {
          selected = agent;
          break;
        }
      }
    }
    final triggerLabel = selected?.label ?? 'Terminal';
    final triggerIcon = selected?.iconPath;
    final helpText = selected != null
        ? 'The new tab will open in this task’s existing worktree.'
        : 'Open a plain shell in this task’s existing worktree.';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Padding(
          padding: EdgeInsets.only(bottom: 8),
          child: Text(
            'Agent',
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w600,
              color: AppTokens.textPrimary,
            ),
          ),
        ),
        InkWell(
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
          onTap: onToggleDropdown,
          child: Container(
            height: 38,
            padding: const EdgeInsets.symmetric(horizontal: 10),
            decoration: BoxDecoration(
              color: AppTokens.overlayHover,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              border: Border.all(
                color: dropdownOpen
                    ? const Color(0xFF6F86CB)
                    : AppTokens.border,
              ),
            ),
            child: Row(
              children: [
                _AgentGlyph(iconPath: triggerIcon, size: 18),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    triggerLabel,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: 13,
                      color: AppTokens.textPrimary,
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
        Padding(
          padding: const EdgeInsets.only(top: 6),
          child: Text(
            helpText,
            style: const TextStyle(fontSize: 11, color: AppTokens.textMuted),
          ),
        ),
        if (dropdownOpen)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: Container(
              decoration: BoxDecoration(
                color: AppTokens.cardBg,
                borderRadius: BorderRadius.circular(AppTokens.radiusMd),
                border: Border.all(color: AppTokens.border),
                boxShadow: const [
                  BoxShadow(
                    color: Color(0x55000000),
                    blurRadius: 8,
                    offset: Offset(0, 4),
                  ),
                ],
              ),
              clipBehavior: Clip.antiAlias,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  _AgentRow(
                    label: 'Terminal',
                    iconPath: null,
                    selected: selectedId == null,
                    onTap: onSelect == null ? null : () => onSelect!(null),
                  ),
                  for (final agent in agents)
                    _AgentRow(
                      label: agent.label,
                      iconPath: agent.iconPath,
                      selected: selectedId == agent.id,
                      onTap: onSelect == null
                          ? null
                          : () => onSelect!(agent.id),
                    ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

class _AgentGlyph extends StatelessWidget {
  const _AgentGlyph({required this.iconPath, required this.size});
  final String? iconPath;
  final double size;

  @override
  Widget build(BuildContext context) {
    if (iconPath == null) {
      return Icon(Icons.terminal, size: size, color: AppTokens.textPrimary);
    }
    if (iconPath!.endsWith('.svg')) {
      return SvgPicture.asset(
        iconPath!,
        width: size,
        height: size,
        colorFilter: const ColorFilter.mode(
          AppTokens.textPrimary,
          BlendMode.srcIn,
        ),
      );
    }
    // Branded PNG (e.g. Claude logo) — show as is, no recolor.
    return Image.asset(iconPath!, width: size, height: size);
  }
}

class _AgentRow extends StatefulWidget {
  const _AgentRow({
    required this.label,
    required this.iconPath,
    required this.selected,
    required this.onTap,
  });

  final String label;
  final String? iconPath;
  final bool selected;
  final VoidCallback? onTap;

  @override
  State<_AgentRow> createState() => _AgentRowState();
}

class _AgentRowState extends State<_AgentRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    const accent = Color(0xFF6F86CB);
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          height: 36,
          margin: const EdgeInsets.symmetric(horizontal: 4),
          padding: const EdgeInsets.symmetric(horizontal: 12),
          decoration: BoxDecoration(
            color: widget.selected
                ? AppTokens.overlayActive
                : (_hover ? AppTokens.overlayHover : Colors.transparent),
            borderRadius: BorderRadius.circular(AppTokens.radiusMd),
          ),
          child: Row(
            children: [
              Container(
                width: 18,
                height: 18,
                decoration: BoxDecoration(
                  color: widget.selected ? accent : Colors.transparent,
                  border: Border.all(
                    color: widget.selected ? accent : AppTokens.border,
                  ),
                  borderRadius: BorderRadius.circular(999),
                ),
                alignment: Alignment.center,
                child: widget.selected
                    ? SvgPicture.asset(
                        'assets/icons/icons__check.svg',
                        width: 11,
                        height: 11,
                        colorFilter: const ColorFilter.mode(
                          Colors.white,
                          BlendMode.srcIn,
                        ),
                      )
                    : null,
              ),
              const SizedBox(width: 10),
              _AgentGlyph(iconPath: widget.iconPath, size: 18),
              const SizedBox(width: 10),
              Expanded(
                child: Text(
                  widget.label,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: 13,
                    fontWeight: FontWeight.w500,
                    color: AppTokens.textSecondary,
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
        border: Border(top: BorderSide(color: AppTokens.divider)),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.end,
        children: [
          InkWell(
            borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            onTap: onCancel,
            child: Container(
              height: 30,
              padding: const EdgeInsets.symmetric(horizontal: 14),
              alignment: Alignment.center,
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(AppTokens.radiusMd),
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
            onTap: onSubmit,
            child: Container(
              height: 30,
              padding: const EdgeInsets.symmetric(horizontal: 16),
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: Colors.white,
                borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              ),
              child: submitting
                  ? const SizedBox(
                      width: 14,
                      height: 14,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        valueColor: AlwaysStoppedAnimation<Color>(
                          Color(0xFF1E1F22),
                        ),
                      ),
                    )
                  : const Text(
                      'Create',
                      style: TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w600,
                        color: Color(0xFF1E1F22),
                      ),
                    ),
            ),
          ),
        ],
      ),
    );
  }
}
