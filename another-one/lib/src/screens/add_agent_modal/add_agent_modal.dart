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

import '../../rust/api/embedded_daemon.dart' as embedded_daemon;
import '../../rust/api/local_session.dart' show AgentSummaryDto;
import '../../state/local_connection_provider.dart';
import '../../state/new_task_data_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_toast.dart';

Future<String?> showAddAgentModal({
  required BuildContext context,
  required String sectionId,
  String? seededAgentId,
}) async {
  final result = await showDialog<String>(
    context: context,
    barrierColor: AppTokens.scrimBg,
    builder: (_) =>
        _AddAgentModal(sectionId: sectionId, seededAgentId: seededAgentId),
  );
  return result;
}

enum _AddAgentFocusKind { trigger, option, create, cancel }

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
  bool _submitted = false;
  bool _prewarmSyncScheduled = false;
  _AddAgentFocusKind _focusKind = _AddAgentFocusKind.create;
  int? _focusOptionIndex;
  String? _lastRequestedPrewarmAgentId;

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

  void _schedulePrewarmSync() {
    if (_submitted) return;
    final desiredAgentId = _selectedAgentId;
    if (_lastRequestedPrewarmAgentId == desiredAgentId &&
        !_prewarmSyncScheduled) {
      return;
    }
    _lastRequestedPrewarmAgentId = desiredAgentId;
    if (_prewarmSyncScheduled) return;
    _prewarmSyncScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      _prewarmSyncScheduled = false;
      if (!mounted || _submitted) return;
      final requestedAgentId = _lastRequestedPrewarmAgentId;
      try {
        await embedded_daemon.syncAddAgentModalPrewarm(
          sectionId: widget.sectionId,
          selectedAgentId: requestedAgentId,
        );
      } catch (_) {}
    });
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
      final tabId = await ref
          .read(localConnectionProvider)
          .addAgentToSection(
            sectionId: widget.sectionId,
            agentId: selectedAgentId ?? '',
          );
      _submitted = true;
      ref
          .read(selectedTabProvider.notifier)
          .set(TabSelection(sectionId: widget.sectionId, tabId: tabId));
      if (!mounted) return;
      Navigator.of(context).pop(tabId);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _submitting = false;
      });
      showAppToast(context, message: 'Could not add agent tab: $e');
    }
  }

  int _optionCount(List<AgentSummaryDto> agents) => agents.length + 1;

  int _selectedOptionIndex(List<AgentSummaryDto> agents) {
    final selectedAgentId = _selectedAgentId;
    if (selectedAgentId == null) return 0;
    for (var i = 0; i < agents.length; i++) {
      if (agents[i].id == selectedAgentId) return i + 1;
    }
    return 0;
  }

  String? _optionIdForIndex(int optionIndex, List<AgentSummaryDto> agents) {
    if (optionIndex == 0) return null;
    if (optionIndex > 0 && optionIndex <= agents.length) {
      return agents[optionIndex - 1].id;
    }
    return null;
  }

  void _moveFocus(List<AgentSummaryDto> agents, {required bool backwards}) {
    final order = <(_AddAgentFocusKind, int?)>[
      (_AddAgentFocusKind.trigger, null),
    ];
    if (_dropdownOpen) {
      for (var i = 0; i < _optionCount(agents); i++) {
        order.add((_AddAgentFocusKind.option, i));
      }
    }
    order.add((_AddAgentFocusKind.create, null));
    order.add((_AddAgentFocusKind.cancel, null));

    final currentIndex = order.indexWhere(
      (focus) => focus.$1 == _focusKind && focus.$2 == _focusOptionIndex,
    );
    final nextIndex = currentIndex == -1
        ? (backwards ? order.length - 1 : 0)
        : backwards
        ? (currentIndex + order.length - 1) % order.length
        : (currentIndex + 1) % order.length;
    final nextFocus = order[nextIndex];

    setState(() {
      _focusKind = nextFocus.$1;
      _focusOptionIndex = nextFocus.$2;
      if (_focusKind == _AddAgentFocusKind.create ||
          _focusKind == _AddAgentFocusKind.cancel) {
        _dropdownOpen = false;
      }
    });
  }

  void _moveOptionFocus(
    List<AgentSummaryDto> agents, {
    required bool backwards,
  }) {
    final optionCount = _optionCount(agents);
    final fallbackIndex = _selectedOptionIndex(
      agents,
    ).clamp(0, optionCount - 1);
    final currentIndex =
        _focusKind == _AddAgentFocusKind.option && _focusOptionIndex != null
        ? _focusOptionIndex!.clamp(0, optionCount - 1)
        : fallbackIndex;
    final nextIndex = backwards
        ? (currentIndex + optionCount - 1) % optionCount
        : (currentIndex + 1) % optionCount;

    setState(() {
      _focusKind = _AddAgentFocusKind.option;
      _focusOptionIndex = nextIndex;
    });
  }

  void _activateFocusedControl(List<AgentSummaryDto> agents) {
    switch (_focusKind) {
      case _AddAgentFocusKind.trigger:
        setState(() {
          _dropdownOpen = true;
          _focusKind = _AddAgentFocusKind.option;
          _focusOptionIndex = _selectedOptionIndex(agents);
        });
      case _AddAgentFocusKind.option:
        final optionCount = _optionCount(agents);
        final optionIndex =
            _focusOptionIndex?.clamp(0, optionCount - 1) ??
            _selectedOptionIndex(agents).clamp(0, optionCount - 1);
        setState(() {
          _selectedAgentId = _optionIdForIndex(optionIndex, agents);
          _dropdownOpen = false;
          _focusKind = _AddAgentFocusKind.trigger;
          _focusOptionIndex = null;
        });
      case _AddAgentFocusKind.create:
        unawaited(_submit());
      case _AddAgentFocusKind.cancel:
        if (!_submitting) {
          Navigator.of(context).pop(false);
        }
    }
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final agents =
        ref.read(enabledAgentsProvider).valueOrNull?.agents ??
        const <AgentSummaryDto>[];
    final key = event.logicalKey;

    if (key == LogicalKeyboardKey.tab) {
      _moveFocus(agents, backwards: HardwareKeyboard.instance.isShiftPressed);
      return KeyEventResult.handled;
    }

    if (_dropdownOpen &&
        (key == LogicalKeyboardKey.arrowUp ||
            key == LogicalKeyboardKey.arrowDown)) {
      _moveOptionFocus(agents, backwards: key == LogicalKeyboardKey.arrowUp);
      return KeyEventResult.handled;
    }

    if (key == LogicalKeyboardKey.escape) {
      if (_dropdownOpen) {
        setState(() {
          _dropdownOpen = false;
          _focusKind = _AddAgentFocusKind.trigger;
          _focusOptionIndex = null;
        });
      } else if (!_submitting) {
        Navigator.of(context).pop(false);
      }
      return KeyEventResult.handled;
    }

    if (key == LogicalKeyboardKey.enter ||
        key == LogicalKeyboardKey.numpadEnter) {
      if (!_submitting) {
        _activateFocusedControl(agents);
      }
      return KeyEventResult.handled;
    }

    return KeyEventResult.ignored;
  }

  @override
  void dispose() {
    if (!_submitted) {
      unawaited(embedded_daemon.cancelActiveAddAgentPrewarm());
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final agentsAsync = ref.watch(enabledAgentsProvider);
    final view = agentsAsync.valueOrNull;
    final agents = view?.agents ?? const <AgentSummaryDto>[];
    if (view != null) {
      _syncSelection(view.agents, view.defaultAgentId);
    }
    _schedulePrewarmSync();
    return PopScope(
      canPop: !_submitting,
      child: Focus(
        autofocus: true,
        onKeyEvent: _onKey,
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
                            agents: agents,
                            selectedId: _selectedAgentId,
                            dropdownOpen: _dropdownOpen,
                            triggerFocused:
                                _focusKind == _AddAgentFocusKind.trigger,
                            focusedOptionIndex:
                                _focusKind == _AddAgentFocusKind.option
                                ? _focusOptionIndex
                                : null,
                            onToggleDropdown: _submitting
                                ? null
                                : () => setState(() {
                                    if (_dropdownOpen) {
                                      _dropdownOpen = false;
                                      _focusKind = _AddAgentFocusKind.trigger;
                                      _focusOptionIndex = null;
                                    } else {
                                      _dropdownOpen = true;
                                      _focusKind = _AddAgentFocusKind.option;
                                      _focusOptionIndex = _selectedOptionIndex(
                                        agents,
                                      );
                                    }
                                  }),
                            onSelect: _submitting
                                ? null
                                : (id) => setState(() {
                                    _selectedAgentId = id;
                                    _dropdownOpen = false;
                                    _focusKind = _AddAgentFocusKind.trigger;
                                    _focusOptionIndex = null;
                                  }),
                          ),
                        ),
                      ),
                    ),
                    _Footer(
                      submitting: _submitting,
                      createFocused: _focusKind == _AddAgentFocusKind.create,
                      cancelFocused: _focusKind == _AddAgentFocusKind.cancel,
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
    );
  }
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
    required this.triggerFocused,
    required this.focusedOptionIndex,
    required this.onToggleDropdown,
    required this.onSelect,
  });

  final List<AgentSummaryDto> agents;
  final String? selectedId;
  final bool dropdownOpen;
  final bool triggerFocused;
  final int? focusedOptionIndex;
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
                color: (dropdownOpen || triggerFocused)
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
              child: SizedBox(
                height: (agents.length + 1).clamp(1, 6) * 36.0 + 8,
                child: SingleChildScrollView(
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        _AgentRow(
                          label: 'Terminal',
                          iconPath: null,
                          selected: selectedId == null,
                          focused: focusedOptionIndex == 0,
                          onTap: onSelect == null
                              ? null
                              : () => onSelect!(null),
                        ),
                        for (var i = 0; i < agents.length; i++)
                          _AgentRow(
                            label: agents[i].label,
                            iconPath: agents[i].iconPath,
                            selected: selectedId == agents[i].id,
                            focused: focusedOptionIndex == i + 1,
                            onTap: onSelect == null
                                ? null
                                : () => onSelect!(agents[i].id),
                          ),
                      ],
                    ),
                  ),
                ),
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
    required this.focused,
    required this.onTap,
  });

  final String label;
  final String? iconPath;
  final bool selected;
  final bool focused;
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
            border: Border.all(
              color: widget.focused ? accent : Colors.transparent,
            ),
            color: widget.selected
                ? AppTokens.overlayActive
                : (widget.focused || _hover
                      ? AppTokens.overlayHover
                      : Colors.transparent),
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
    required this.createFocused,
    required this.cancelFocused,
    required this.onCancel,
    required this.onSubmit,
  });
  final bool submitting;
  final bool createFocused;
  final bool cancelFocused;
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
                border: Border.all(
                  color: cancelFocused
                      ? const Color(0xFF6F86CB)
                      : AppTokens.border,
                ),
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
                border: Border.all(
                  color: createFocused
                      ? const Color(0xFF6F86CB)
                      : Colors.transparent,
                ),
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
