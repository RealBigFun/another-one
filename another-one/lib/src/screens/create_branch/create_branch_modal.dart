// "Create Branch" modal — pixel-precise port of
// `desktop/src/create_branch_modal.rs`. Opened from the titlebar
// git-actions dropdown's last row.
//
// Two creation paths gated by the "Use current task" toggle:
//   * use_current_task=true  → switch the active checkout in
//     place to the new branch (no new project).
//   * use_current_task=false → spawn a new git worktree with the
//     new branch + insert it as a fresh task in the daemon's
//     store. `migrate_changes` decides whether uncommitted
//     working-tree changes are stashed into the worktree.
//
// `migrate_changes` is locked ON when `use_current_task` is on
// (matches GPUI's defensive default — switching branches in place
// always brings working-tree changes along).

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../state/local_connection_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';

/// Open the modal as a route. Returns the new task's `sectionId`
/// when the user submitted in worktree mode (the caller can flip
/// `selectedTabProvider` to navigate); empty string when they
/// submitted in current-task mode; `null` when they cancelled.
Future<String?> showCreateBranchModal({
  required BuildContext context,
  required String projectId,
}) {
  return showDialog<String>(
    context: context,
    barrierColor: const Color(0x80000000),
    barrierDismissible: true,
    builder: (ctx) => _CreateBranchModal(projectId: projectId),
  );
}

class _CreateBranchModal extends ConsumerStatefulWidget {
  const _CreateBranchModal({required this.projectId});

  final String projectId;

  @override
  ConsumerState<_CreateBranchModal> createState() => _CreateBranchModalState();
}

class _CreateBranchModalState extends ConsumerState<_CreateBranchModal> {
  late final TextEditingController _controller;
  late final FocusNode _focusNode;

  bool _useCurrentTask = false;
  bool _migrateChanges = false;
  bool _submitting = false;
  String _slug = '';
  String _error = '';

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController();
    _focusNode = FocusNode();
    _controller.addListener(_onTextChanged);
    // Match GPUI: focus the input as soon as the modal opens.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _focusNode.requestFocus();
    });
  }

  @override
  void dispose() {
    _controller.removeListener(_onTextChanged);
    _controller.dispose();
    _focusNode.dispose();
    super.dispose();
  }

  Future<void> _onTextChanged() async {
    final input = _controller.text;
    if (input.trim().isEmpty) {
      if (_slug.isNotEmpty) setState(() => _slug = '');
      return;
    }
    final connection = ref.read(localConnectionProvider);
    try {
      final next = await connection.slugifyBranchName(input);
      if (mounted && next != _slug) setState(() => _slug = next);
    } catch (_) {
      // Slug preview is best-effort — failure here just leaves
      // the previous preview onscreen until the user edits again.
    }
  }

  Future<void> _submit() async {
    final name = _controller.text.trim();
    if (name.isEmpty || _submitting) return;
    setState(() {
      _submitting = true;
      _error = '';
    });
    final connection = ref.read(localConnectionProvider);
    try {
      final result = await connection.createBranch(
        projectId: widget.projectId,
        branchName: name,
        useCurrentTask: _useCurrentTask,
        migrateChanges: _migrateChanges || _useCurrentTask,
      );
      if (!mounted) return;
      // Worktree mode returns a section_id; current-task mode
      // returns empty string. In both cases, dismiss the modal
      // with whatever the caller might want to navigate to.
      if (result.isNotEmpty) {
        ref
            .read(selectedTabProvider.notifier)
            .set(TabSelection(sectionId: result, tabId: '0'));
      }
      Navigator.of(context).pop(result);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _submitting = false;
        _error = e.toString();
      });
    }
  }

  void _cancel() {
    if (_submitting) return;
    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    return PopScope(
      canPop: !_submitting,
      child: Dialog(
        backgroundColor: Colors.transparent,
        elevation: 0,
        insetPadding: EdgeInsets.zero,
        child: Shortcuts(
          shortcuts: const {
            SingleActivator(LogicalKeyboardKey.escape):
                _CreateBranchDismissIntent(),
            SingleActivator(LogicalKeyboardKey.enter):
                _CreateBranchSubmitIntent(),
            SingleActivator(LogicalKeyboardKey.numpadEnter):
                _CreateBranchSubmitIntent(),
          },
          child: Actions(
            actions: {
              _CreateBranchDismissIntent:
                  CallbackAction<_CreateBranchDismissIntent>(
                    onInvoke: (_) {
                      _cancel();
                      return null;
                    },
                  ),
              _CreateBranchSubmitIntent:
                  CallbackAction<_CreateBranchSubmitIntent>(
                    onInvoke: (_) {
                      _submit();
                      return null;
                    },
                  ),
            },
            child: Focus(
              autofocus: true,
              child: Container(
                width: 420,
                decoration: BoxDecoration(
                  color: AppTokens.cardBg,
                  borderRadius: BorderRadius.circular(AppTokens.radiusLg),
                  border: Border.all(color: AppTokens.border),
                  boxShadow: const [
                    BoxShadow(
                      color: Color(0x66000000),
                      blurRadius: 24,
                      offset: Offset(0, 8),
                    ),
                  ],
                ),
                clipBehavior: Clip.antiAlias,
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _Header(submitting: _submitting, onClose: _cancel),
                    Padding(
                      padding: const EdgeInsets.fromLTRB(20, 14, 20, 14),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          _BranchNameField(
                            controller: _controller,
                            focusNode: _focusNode,
                            submitting: _submitting,
                            onSubmitted: (_) => _submit(),
                          ),
                          const SizedBox(height: 14),
                          Text(
                            'Branch: ${_slug.isEmpty ? '—' : _slug}',
                            style: const TextStyle(
                              fontSize: 12,
                              color: AppTokens.textMuted,
                            ),
                          ),
                          const SizedBox(height: 14),
                          _Toggle(
                            label: 'Use current task',
                            checked: _useCurrentTask,
                            submitting: _submitting,
                            disabled: false,
                            onTap: () => setState(() {
                              _useCurrentTask = !_useCurrentTask;
                              if (_useCurrentTask) _migrateChanges = true;
                            }),
                          ),
                          const SizedBox(height: 14),
                          _Toggle(
                            label: 'Migrate changes to new branch',
                            checked: _migrateChanges || _useCurrentTask,
                            submitting: _submitting,
                            disabled: _useCurrentTask,
                            onTap: () => setState(
                              () => _migrateChanges = !_migrateChanges,
                            ),
                          ),
                          if (_error.isNotEmpty) ...[
                            const SizedBox(height: 14),
                            Text(
                              _error,
                              style: const TextStyle(
                                fontSize: 11,
                                color: AppTokens.errorText,
                              ),
                            ),
                          ],
                        ],
                      ),
                    ),
                    _Footer(
                      submitting: _submitting,
                      canSubmit: _controller.text.trim().isNotEmpty,
                      onCancel: _cancel,
                      onSubmit: _submit,
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

class _CreateBranchDismissIntent extends Intent {
  const _CreateBranchDismissIntent();
}

class _CreateBranchSubmitIntent extends Intent {
  const _CreateBranchSubmitIntent();
}

class _Header extends StatelessWidget {
  const _Header({required this.submitting, required this.onClose});

  final bool submitting;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 20, 20, 10),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Create Branch',
                  style: TextStyle(
                    fontSize: 16,
                    fontWeight: FontWeight.w600,
                    color: AppTokens.textPrimary,
                  ),
                ),
                SizedBox(height: 4),
                Text(
                  'Create a branch here or in a separate worktree.',
                  style: TextStyle(fontSize: 12, color: AppTokens.textMuted),
                ),
              ],
            ),
          ),
          _CloseButton(submitting: submitting, onTap: onClose),
        ],
      ),
    );
  }
}

class _CloseButton extends StatefulWidget {
  const _CloseButton({required this.submitting, required this.onTap});

  final bool submitting;
  final VoidCallback onTap;

  @override
  State<_CloseButton> createState() => _CloseButtonState();
}

class _CloseButtonState extends State<_CloseButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: 'Close the create branch modal',
      child: MouseRegion(
        cursor: widget.submitting
            ? SystemMouseCursors.basic
            : SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.submitting ? null : widget.onTap,
          child: Opacity(
            opacity: widget.submitting ? 0.45 : 1.0,
            child: Container(
              width: 24,
              height: 24,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: !widget.submitting && _hover
                    ? AppTokens.overlayHover
                    : Colors.transparent,
                borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              ),
              child: const AppIcon(
                'close',
                size: 14,
                color: AppTokens.textMuted,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _BranchNameField extends StatelessWidget {
  const _BranchNameField({
    required this.controller,
    required this.focusNode,
    required this.submitting,
    required this.onSubmitted,
  });

  final TextEditingController controller;
  final FocusNode focusNode;
  final bool submitting;
  final ValueChanged<String> onSubmitted;

  @override
  Widget build(BuildContext context) {
    return Opacity(
      opacity: submitting ? 0.55 : 1.0,
      child: Container(
        height: 38,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(
          color: const Color(0x24000000), // black @ 0.14
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
          border: Border.all(color: const Color(0x1FFFFFFF)), // white @ 0.12
        ),
        child: Center(
          child: TextField(
            controller: controller,
            focusNode: focusNode,
            enabled: !submitting,
            onSubmitted: onSubmitted,
            style: const TextStyle(fontSize: 14, color: AppTokens.textPrimary),
            decoration: const InputDecoration(
              isDense: true,
              contentPadding: EdgeInsets.zero,
              border: InputBorder.none,
              hintText: 'Branch name',
              hintStyle: TextStyle(
                fontSize: 14,
                color: AppTokens.textPlaceholder,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _Toggle extends StatefulWidget {
  const _Toggle({
    required this.label,
    required this.checked,
    required this.submitting,
    required this.disabled,
    required this.onTap,
  });

  final String label;
  final bool checked;
  final bool submitting;
  final bool disabled;
  final VoidCallback onTap;

  @override
  State<_Toggle> createState() => _ToggleState();
}

class _ToggleState extends State<_Toggle> {
  bool _hover = false;

  // hsla(0.58, 0.62, 0.48) ≈ #2F8CC4
  static const Color _onColor = Color(0xFF2F8CC4);

  @override
  Widget build(BuildContext context) {
    final interactive = !widget.submitting && !widget.disabled;
    return Opacity(
      opacity: interactive ? 1.0 : 0.55,
      child: MouseRegion(
        cursor: interactive
            ? SystemMouseCursors.click
            : SystemMouseCursors.basic,
        onEnter: interactive ? (_) => setState(() => _hover = true) : null,
        onExit: interactive ? (_) => setState(() => _hover = false) : null,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: interactive ? widget.onTap : null,
          child: Container(
            padding: const EdgeInsets.symmetric(vertical: 6),
            color: interactive && _hover
                ? AppTokens.overlayHover
                : Colors.transparent,
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    widget.label,
                    style: const TextStyle(
                      fontSize: 13,
                      fontWeight: FontWeight.w500,
                      color: AppTokens.textSecondary,
                    ),
                  ),
                ),
                const SizedBox(width: 12),
                _ToggleSwitch(checked: widget.checked, onColor: _onColor),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _ToggleSwitch extends StatelessWidget {
  const _ToggleSwitch({required this.checked, required this.onColor});

  final bool checked;
  final Color onColor;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 34,
      height: 20,
      padding: const EdgeInsets.all(2),
      decoration: BoxDecoration(
        color: checked ? onColor : const Color(0x1FFFFFFF), // white @ 0.12
        borderRadius: BorderRadius.circular(999),
      ),
      child: Align(
        alignment: checked ? Alignment.centerRight : Alignment.centerLeft,
        child: Container(
          width: 16,
          height: 16,
          decoration: const BoxDecoration(
            color: Colors.white,
            shape: BoxShape.circle,
          ),
        ),
      ),
    );
  }
}

class _Footer extends StatelessWidget {
  const _Footer({
    required this.submitting,
    required this.canSubmit,
    required this.onCancel,
    required this.onSubmit,
  });

  final bool submitting;
  final bool canSubmit;
  final VoidCallback onCancel;
  final VoidCallback onSubmit;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 8, 20, 16),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.end,
        children: [
          _FooterButton(
            label: 'Cancel',
            tooltip: 'Close without creating a branch',
            background: AppTokens.overlayHoverStrong,
            hoverBg: const Color(0x24FFFFFF),
            color: AppTokens.textPrimary,
            fontWeight: FontWeight.w500,
            enabled: !submitting,
            onTap: onCancel,
          ),
          const SizedBox(width: 8),
          _FooterButton(
            label: submitting ? 'Creating…' : 'Create',
            tooltip: 'Create the branch',
            // Same focus-ring blue as the active toggle
            background: const Color(0xFF2F8CC4),
            hoverBg: const Color(0xFF4FA1D4),
            color: AppTokens.textPrimary,
            fontWeight: FontWeight.w600,
            enabled: !submitting && canSubmit,
            onTap: onSubmit,
          ),
        ],
      ),
    );
  }
}

class _FooterButton extends StatefulWidget {
  const _FooterButton({
    required this.label,
    required this.tooltip,
    required this.background,
    required this.hoverBg,
    required this.color,
    required this.fontWeight,
    required this.enabled,
    required this.onTap,
  });

  final String label;
  final String tooltip;
  final Color background;
  final Color hoverBg;
  final Color color;
  final FontWeight fontWeight;
  final bool enabled;
  final VoidCallback onTap;

  @override
  State<_FooterButton> createState() => _FooterButtonState();
}

class _FooterButtonState extends State<_FooterButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: widget.enabled
            ? SystemMouseCursors.click
            : SystemMouseCursors.basic,
        onEnter: widget.enabled ? (_) => setState(() => _hover = true) : null,
        onExit: widget.enabled ? (_) => setState(() => _hover = false) : null,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.enabled ? widget.onTap : null,
          child: Opacity(
            opacity: widget.enabled ? 1.0 : 0.55,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 6),
              decoration: BoxDecoration(
                color: widget.enabled && _hover
                    ? widget.hoverBg
                    : widget.background,
                borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              ),
              child: Text(
                widget.label,
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: widget.fontWeight,
                  color: widget.color,
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
