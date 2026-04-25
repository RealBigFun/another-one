// New-task modal — minimum-viable port of
// `desktop/src/new_task_modal.rs`.
//
// Today's modal exposes the core knobs needed to spawn a worktree
// task: task name, source branch, agent provider. Advanced surface
// from the GPUI modal (multi-agent select, worktree-vs-direct
// toggle, prewarm, automatic actions, advanced collapsible) lands
// in subsequent commits as the supporting bridge verbs come online.
//
// Submission calls `DaemonConnection.createWorktreeTask` (today
// served by `LocalTransport`; remote daemons get the same surface
// once the iroh wire grows `Control::CreateTask`). On success the
// modal closes and the new task's section is selected so the
// terminal pane drops the user straight into it.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../rust/api/iroh_client.dart';
import '../../state/local_connection_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../widgets/agent_provider_icon.dart';
import '../../widgets/pill.dart';

Future<void> showNewTaskModal(
  BuildContext context, {
  required ProjectSummary project,
}) {
  return showDialog<void>(
    context: context,
    barrierColor: AppTokens.scrimBg,
    builder: (_) => NewTaskModal(project: project),
  );
}

class NewTaskModal extends ConsumerStatefulWidget {
  const NewTaskModal({super.key, required this.project});

  final ProjectSummary project;

  @override
  ConsumerState<NewTaskModal> createState() => _NewTaskModalState();
}

class _NewTaskModalState extends ConsumerState<NewTaskModal> {
  late final TextEditingController _taskNameCtrl;
  late final TextEditingController _branchCtrl;
  AgentProvider? _provider;
  bool _submitting = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _taskNameCtrl = TextEditingController();
    // Pre-populate the source branch with the project's current
    // branch — matches the GPUI default in
    // `open_new_task_modal::primary_branch_for_project`.
    _branchCtrl =
        TextEditingController(text: widget.project.currentBranch ?? 'main');
  }

  @override
  void dispose() {
    _taskNameCtrl.dispose();
    _branchCtrl.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final taskName = _taskNameCtrl.text.trim();
    final branch = _branchCtrl.text.trim();
    if (taskName.isEmpty) {
      setState(() => _error = 'Task name is required');
      return;
    }
    if (branch.isEmpty) {
      setState(() => _error = 'Source branch is required');
      return;
    }
    setState(() {
      _submitting = true;
      _error = null;
    });
    try {
      final transport = ref.read(localConnectionProvider);
      final sectionId = await transport.createWorktreeTask(
        projectId: widget.project.id,
        taskName: taskName,
        sourceBranch: branch,
        agentProvider: _provider,
      );
      // The new task's first tab id isn't known until the daemon
      // launches the section; for now we set selection on the
      // section + a synthetic tab id of '0' (matches the daemon's
      // first tab assignment in `attach_or_start_prewarmed_terminal`
      // / `next_tab_id`). A subsequent listProjects refresh fills
      // in the real id when the user clicks the row.
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
    return Dialog(
      backgroundColor: AppTokens.cardBg,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.radiusXl),
        side: const BorderSide(color: AppTokens.border),
      ),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.space7),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _Header(projectName: widget.project.name),
              const SizedBox(height: AppTokens.space5),
              _LabeledField(
                label: 'Task name',
                child: TextField(
                  controller: _taskNameCtrl,
                  autofocus: true,
                  enabled: !_submitting,
                  decoration: _decoration('e.g. silver-moonwalk-zooms'),
                ),
              ),
              const SizedBox(height: AppTokens.space5),
              _LabeledField(
                label: 'Source branch',
                child: TextField(
                  controller: _branchCtrl,
                  enabled: !_submitting,
                  decoration: _decoration('main'),
                  style: const TextStyle(
                    fontFamily: AppTokens.fontFamilyMono,
                    fontSize: AppTokens.fontBodyLg,
                    color: AppTokens.textPrimary,
                  ),
                ),
              ),
              const SizedBox(height: AppTokens.space5),
              const _LabelText('Agent'),
              const SizedBox(height: AppTokens.space2),
              _AgentChips(
                selected: _provider,
                onChanged: _submitting
                    ? null
                    : (value) => setState(() => _provider = value),
              ),
              if (_error != null) ...[
                const SizedBox(height: AppTokens.space5),
                Text(
                  _error!,
                  style: const TextStyle(
                    fontSize: AppTokens.fontBody,
                    color: AppTokens.errorText,
                  ),
                ),
              ],
              const SizedBox(height: AppTokens.space7),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                    onPressed:
                        _submitting ? null : () => Navigator.of(context).pop(),
                    child: const Text('Cancel'),
                  ),
                  const SizedBox(width: AppTokens.space3),
                  FilledButton(
                    onPressed: _submitting ? null : _submit,
                    child: _submitting
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Text('Create task'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  InputDecoration _decoration(String hint) => InputDecoration(
        hintText: hint,
        hintStyle: const TextStyle(color: AppTokens.textPlaceholder),
        isDense: true,
        contentPadding: const EdgeInsets.symmetric(
          horizontal: AppTokens.space4,
          vertical: AppTokens.space3,
        ),
        filled: true,
        fillColor: AppTokens.sunkenBg,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
          borderSide: const BorderSide(color: AppTokens.border),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
          borderSide: const BorderSide(color: AppTokens.border),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
          borderSide: BorderSide(color: AppTokens.focusRing, width: 1.5),
        ),
      );
}

class _Header extends StatelessWidget {
  const _Header({required this.projectName});

  final String projectName;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'New task',
          style: const TextStyle(
            fontSize: AppTokens.fontHeading,
            fontWeight: FontWeight.w600,
            color: AppTokens.textPrimary,
          ),
        ),
        const SizedBox(height: 2),
        Text(
          'in $projectName',
          style: const TextStyle(
            fontSize: AppTokens.fontBody,
            color: AppTokens.textMuted,
          ),
        ),
      ],
    );
  }
}

class _LabeledField extends StatelessWidget {
  const _LabeledField({required this.label, required this.child});

  final String label;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _LabelText(label),
        const SizedBox(height: AppTokens.space2),
        child,
      ],
    );
  }
}

class _LabelText extends StatelessWidget {
  const _LabelText(this.text);

  final String text;

  @override
  Widget build(BuildContext context) {
    return Text(
      text,
      style: const TextStyle(
        fontSize: AppTokens.fontCaption,
        fontWeight: FontWeight.w600,
        color: AppTokens.textPlaceholder,
        letterSpacing: 0.6,
      ),
    );
  }
}

const List<({AgentProvider? value, String label})> _agentOptions = [
  (value: null, label: 'Shell'),
  (value: AgentProvider.claudeCode, label: 'Claude Code'),
  (value: AgentProvider.codex, label: 'Codex'),
  (value: AgentProvider.cursorAgent, label: 'Cursor'),
  (value: AgentProvider.gemini, label: 'Gemini'),
  (value: AgentProvider.pi, label: 'Pi'),
  (value: AgentProvider.openCode, label: 'OpenCode'),
  (value: AgentProvider.amp, label: 'Amp'),
];

class _AgentChips extends StatelessWidget {
  const _AgentChips({required this.selected, required this.onChanged});

  final AgentProvider? selected;
  final ValueChanged<AgentProvider?>? onChanged;

  @override
  Widget build(BuildContext context) {
    return Wrap(
      spacing: AppTokens.space2,
      runSpacing: AppTokens.space2,
      children: [
        for (final option in _agentOptions)
          _AgentChip(
            label: option.label,
            value: option.value,
            selected: option.value == selected,
            onTap: onChanged == null ? null : () => onChanged!(option.value),
          ),
      ],
    );
  }
}

class _AgentChip extends StatelessWidget {
  const _AgentChip({
    required this.label,
    required this.value,
    required this.selected,
    required this.onTap,
  });

  final String label;
  final AgentProvider? value;
  final bool selected;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return Pill(
      label: label,
      active: selected,
      onTap: onTap,
      iconWidget: value == null
          ? null
          : AgentProviderIcon(
              provider: value,
              size: 13,
              color: AppTokens.textPrimary,
            ),
      // Filled at-rest with a border (matches GPUI's chip). Right-
      // sidebar tabs override these to transparent + no border.
      restBg: AppTokens.overlayRest,
      borderColor: AppTokens.border,
      activeBorderColor: AppTokens.focusRing,
      activeColor: AppTokens.textPrimary,
      inactiveColor: AppTokens.textPrimary,
      horizontalPadding: AppTokens.space4,
      iconSize: 13,
    );
  }
}
