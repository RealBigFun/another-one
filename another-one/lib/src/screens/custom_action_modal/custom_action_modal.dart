// Add/Edit project Custom Action modal.
//
// Direct port of `desktop/src/custom_actions_modal.rs` with the
// same field set, validation, and visual hierarchy. The GPUI
// version is 2,219 LOC because it hand-rolls every text input
// (cursor positioning, multiline rendering, IME, key handling).
// Flutter ships those as `TextField` + `TextEditingController`,
// so the body collapses to ~600 LOC of layout + state.
//
// Returns `true` when an action was saved or deleted, `false`
// when cancelled. Caller uses the return value to decide whether
// to invalidate `projectActionsProvider`.

import 'dart:async';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../../rust/api/iroh_client.dart' show AgentProvider;
import '../../rust/api/local_session.dart'
    show
        ProjectActionAccessDto,
        ProjectActionDto,
        ProjectActionIconDto,
        ProjectActionKindDto,
        ProjectActionKindDto_Agent,
        ProjectActionKindDto_Shell,
        ProjectActionScopeDto;
import '../../state/local_connection_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_toast.dart';

/// Open the Add / Edit Custom Action modal. `existing` is `null`
/// for the "Add" flow; passing one routes to the "Edit" flow with
/// the form pre-filled and the Delete button visible.
///
/// Resolves to `true` when the form was saved or the action was
/// deleted (caller should invalidate `projectActionsProvider`),
/// `false` on cancel / dismiss.
Future<bool> showCustomActionModal({
  required BuildContext context,
  required String projectId,
  ProjectActionDto? existing,
}) async {
  final result = await showDialog<bool>(
    context: context,
    barrierColor: const Color(0x80000000),
    builder: (ctx) =>
        _CustomActionModal(projectId: projectId, existing: existing),
  );
  return result ?? false;
}

enum _Kind { shell, agent }

enum _CustomActionDropdown { provider, model, traits, mode, access }

// Match GPUI's pre-save UUID allocation without pulling in a new package.
final Random _actionIdRandom = Random.secure();

String _newActionId() {
  final bytes = List<int>.generate(16, (_) => _actionIdRandom.nextInt(256));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  final hex = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  return '${hex.substring(0, 8)}-'
      '${hex.substring(8, 12)}-'
      '${hex.substring(12, 16)}-'
      '${hex.substring(16, 20)}-'
      '${hex.substring(20)}';
}

class _CustomActionModal extends ConsumerStatefulWidget {
  const _CustomActionModal({required this.projectId, required this.existing});

  final String projectId;
  final ProjectActionDto? existing;

  @override
  ConsumerState<_CustomActionModal> createState() => _CustomActionModalState();
}

class _CustomActionModalState extends ConsumerState<_CustomActionModal> {
  // Card metrics from desktop/src/custom_actions_modal.rs.
  static const double _cardW = 520;

  late final TextEditingController _name;
  late final TextEditingController _command;
  late final TextEditingController _prompt;
  late final String _actionId;

  late ProjectActionIconDto _icon;
  late _Kind _kind;
  late AgentProvider _provider;
  String _model = '';
  String _traits = '';
  String _mode = '';
  ProjectActionAccessDto _access = ProjectActionAccessDto.default_;
  bool _runOnWorktreeCreate = false;
  bool _saveGlobalCopy = false;
  bool _busy = false;
  _CustomActionDropdown? _openDropdown;

  @override
  void initState() {
    super.initState();
    final existing = widget.existing;
    final existingId = existing?.id;
    _actionId = existingId != null && existingId.isNotEmpty
        ? existingId
        : _newActionId();
    _name = TextEditingController(text: existing?.name ?? '');
    _command = TextEditingController(
      text: existing?.kind is ProjectActionKindDto_Shell
          ? (existing!.kind as ProjectActionKindDto_Shell).command
          : '',
    );
    _prompt = TextEditingController(
      text: existing?.kind is ProjectActionKindDto_Agent
          ? (existing!.kind as ProjectActionKindDto_Agent).prompt
          : '',
    );
    _icon = existing?.icon ?? ProjectActionIconDto.play;
    _kind = existing?.kind is ProjectActionKindDto_Agent
        ? _Kind.agent
        : _Kind.shell;
    if (existing?.kind is ProjectActionKindDto_Agent) {
      final agent = existing!.kind as ProjectActionKindDto_Agent;
      _provider = agent.provider;
      _model = agent.model ?? '';
      _traits = agent.traits ?? '';
      _mode = agent.mode ?? '';
      _access = agent.access;
    } else {
      _provider = AgentProvider.codex;
    }
    _saveGlobalCopy = existing?.scope == ProjectActionScopeDto.global;
    _runOnWorktreeCreate = existing?.runOnWorktreeCreate ?? false;
  }

  @override
  void dispose() {
    _name.dispose();
    _command.dispose();
    _prompt.dispose();
    super.dispose();
  }

  bool get _isEditing => widget.existing != null;

  @override
  Widget build(BuildContext context) {
    return Shortcuts(
      shortcuts: const <ShortcutActivator, Intent>{
        SingleActivator(LogicalKeyboardKey.escape): _DismissIntent(),
        SingleActivator(LogicalKeyboardKey.enter, meta: true): _SubmitIntent(),
        SingleActivator(LogicalKeyboardKey.enter, control: true):
            _SubmitIntent(),
      },
      child: Actions(
        actions: <Type, Action<Intent>>{
          _DismissIntent: CallbackAction<_DismissIntent>(
            onInvoke: (_) {
              if (_openDropdown != null) {
                setState(() => _openDropdown = null);
                return null;
              }
              Navigator.of(context).pop(false);
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
                maxHeight: MediaQuery.of(context).size.height * 0.92,
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
                      _buildHeader(),
                      Flexible(
                        child: SingleChildScrollView(
                          padding: const EdgeInsets.fromLTRB(20, 0, 20, 16),
                          child: _buildBody(),
                        ),
                      ),
                      _buildFooter(),
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

  Widget _buildHeader() {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 20, 20, 12),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  _isEditing ? 'Edit Action' : 'Add Action',
                  style: const TextStyle(
                    fontSize: 16,
                    fontWeight: FontWeight.w600,
                    color: AppTokens.textPrimary,
                  ),
                ),
                const SizedBox(height: 4),
                const Text(
                  'Save project commands or agent prompts for this repository.',
                  style: TextStyle(fontSize: 12, color: AppTokens.textMuted),
                ),
              ],
            ),
          ),
          Tooltip(
            message: 'Close',
            child: InkWell(
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              onTap: () => Navigator.of(context).pop(false),
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
          ),
        ],
      ),
    );
  }

  Widget _buildBody() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: 4),
        _buildKindSelector(),
        _FieldLabel('Name'),
        _TextFieldShell(
          controller: _name,
          placeholder: 'Action name',
          autofocus: !_isEditing,
        ),
        _FieldLabel('Icon'),
        _buildIconPicker(),
        if (_kind == _Kind.shell) ...[
          _FieldLabel('Command'),
          _TextFieldShell(controller: _command, placeholder: 'npm test'),
        ] else ...[
          _FieldLabel('Provider'),
          _DropdownPicker<AgentProvider>(
            value: _provider,
            label: _providerLabel(_provider),
            open: _openDropdown == _CustomActionDropdown.provider,
            onToggle: () => _toggleDropdown(_CustomActionDropdown.provider),
            options: const [AgentProvider.codex, AgentProvider.claudeCode],
            optionLabel: _providerLabel,
            onChanged: (next) {
              setState(() {
                _openDropdown = null;
                if (_provider != next) {
                  _provider = next;
                  _model = '';
                  _traits = '';
                  _mode = '';
                }
              });
            },
            optionBuilder: (ctx, option, selected) =>
                _ProviderOption(provider: option, selected: selected),
          ),
          _FieldLabel('Prompt'),
          _TextFieldShell(
            controller: _prompt,
            placeholder: 'Ask the agent to do something',
            multiline: true,
            minHeight: 190,
          ),
          _FieldLabel('Model'),
          _DropdownPicker<String>(
            value: _model,
            label: _modelLabel(_provider, _model),
            open: _openDropdown == _CustomActionDropdown.model,
            onToggle: () => _toggleDropdown(_CustomActionDropdown.model),
            options: _modelOptions(
              _provider,
              _model,
            ).map((o) => o.value).toList(),
            optionLabel: (v) => _modelLabel(_provider, v),
            onChanged: (next) {
              setState(() {
                _openDropdown = null;
                _model = next;
                _traits = '';
              });
            },
          ),
          _FieldLabel('Traits / effort'),
          _DropdownPicker<String>(
            value: _traits,
            label: _traitsLabel(_provider, _model, _traits),
            open: _openDropdown == _CustomActionDropdown.traits,
            onToggle: () => _toggleDropdown(_CustomActionDropdown.traits),
            options: _traitsOptions(
              _provider,
              _model,
              _traits,
            ).map((o) => o.value).toList(),
            optionLabel: (v) => _traitsLabel(_provider, _model, v),
            onChanged: (next) => setState(() {
              _openDropdown = null;
              _traits = next;
            }),
          ),
          _FieldLabel('Mode'),
          _DropdownPicker<String>(
            value: _mode,
            label: _modeLabel(_mode),
            open: _openDropdown == _CustomActionDropdown.mode,
            onToggle: () => _toggleDropdown(_CustomActionDropdown.mode),
            options: const ['', 'default', 'plan'],
            optionLabel: _modeLabel,
            onChanged: (next) => setState(() {
              _openDropdown = null;
              _mode = next;
            }),
          ),
          _FieldLabel('Access'),
          _DropdownPicker<ProjectActionAccessDto>(
            value: _access,
            label: _accessLabel(_access),
            open: _openDropdown == _CustomActionDropdown.access,
            onToggle: () => _toggleDropdown(_CustomActionDropdown.access),
            options: const [
              ProjectActionAccessDto.default_,
              ProjectActionAccessDto.readOnly,
              ProjectActionAccessDto.workspaceWrite,
              ProjectActionAccessDto.fullAccess,
            ],
            optionLabel: _accessLabel,
            onChanged: (next) => setState(() {
              _openDropdown = null;
              _access = next;
            }),
          ),
        ],
        const SizedBox(height: 16),
        _Toggle(
          label: 'Run automatically on worktree creation',
          value: _runOnWorktreeCreate,
          onChanged: (v) => setState(() => _runOnWorktreeCreate = v),
        ),
        _Toggle(
          label: 'Global action',
          value: _saveGlobalCopy,
          onChanged: (v) => setState(() => _saveGlobalCopy = v),
        ),
      ],
    );
  }

  Widget _buildKindSelector() {
    return Container(
      margin: const EdgeInsets.only(top: 4),
      padding: const EdgeInsets.all(3),
      decoration: BoxDecoration(
        color: AppTokens.overlayHover,
        borderRadius: BorderRadius.circular(AppTokens.radiusSm),
      ),
      child: Row(
        children: [
          _kindOption('Shell', _Kind.shell),
          const SizedBox(width: 2),
          _kindOption('Agent', _Kind.agent),
        ],
      ),
    );
  }

  Widget _kindOption(String label, _Kind option) {
    final selected = _kind == option;
    return Expanded(
      child: InkWell(
        onTap: () => setState(() {
          _kind = option;
          _openDropdown = null;
        }),
        borderRadius: BorderRadius.circular(AppTokens.radiusXs + 1),
        child: Container(
          height: 32,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: selected ? AppTokens.overlayActive : Colors.transparent,
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

  Widget _buildIconPicker() {
    return Wrap(
      spacing: 6,
      runSpacing: 6,
      children: [
        for (final icon in ProjectActionIconDto.values)
          _IconPickerButton(
            icon: icon,
            selected: icon == _icon,
            onTap: () => setState(() => _icon = icon),
          ),
      ],
    );
  }

  Widget _buildFooter() {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 14),
      decoration: const BoxDecoration(
        border: Border(top: BorderSide(color: AppTokens.border)),
      ),
      child: Row(
        children: [
          if (_isEditing)
            _DeleteButton(busy: _busy, onTap: _delete)
          else
            const SizedBox.shrink(),
          const Spacer(),
          _GhostButton(
            label: 'Cancel',
            onTap: _busy ? null : () => Navigator.of(context).pop(false),
          ),
          const SizedBox(width: 8),
          _PrimaryButton(
            label: 'Save',
            busy: _busy,
            onTap: _busy ? null : _submit,
          ),
        ],
      ),
    );
  }

  Future<void> _submit() async {
    if (_busy) return;
    final name = _name.text.trim();
    if (name.isEmpty) {
      _toastError('Custom actions need a name.');
      return;
    }
    final ProjectActionKindDto kind;
    if (_kind == _Kind.shell) {
      final command = _command.text.trim();
      if (command.isEmpty) {
        _toastError('Shell actions need a command.');
        return;
      }
      kind = ProjectActionKindDto.shell(command: command);
    } else {
      final prompt = _prompt.text.trim();
      if (prompt.isEmpty) {
        _toastError('Agent actions need a prompt.');
        return;
      }
      kind = ProjectActionKindDto.agent(
        prompt: prompt,
        provider: _provider,
        model: _trimToOption(_model),
        traits: _trimToOption(_traits),
        mode: _trimToOption(_mode),
        access: _access,
      );
    }
    final action = ProjectActionDto(
      id: _actionId,
      name: name,
      icon: _icon,
      runOnWorktreeCreate: _runOnWorktreeCreate,
      // Bridge `upsert_project_action` re-stamps scope based on
      // saveGlobalCopy. Stamp it Project here so the DTO is fully
      // populated; the round-trip to disk uses the bridge value.
      scope: ProjectActionScopeDto.project,
      kind: kind,
    );
    setState(() => _busy = true);
    try {
      await ref
          .read(localConnectionProvider)
          .saveProjectAction(
            projectId: widget.projectId,
            action: action,
            saveGlobalCopy: _saveGlobalCopy,
          );
      if (!mounted) return;
      _toast('Action saved.', warning: false);
      Navigator.of(context).pop(true);
    } catch (e) {
      if (!mounted) return;
      _toastError('Could not save action: $e');
      setState(() => _busy = false);
    }
  }

  Future<void> _delete() async {
    final id = widget.existing?.id;
    if (id == null || id.isEmpty || _busy) return;
    setState(() => _busy = true);
    try {
      final deleted = await ref
          .read(localConnectionProvider)
          .deleteProjectAction(projectId: widget.projectId, actionId: id);
      if (!mounted) return;
      if (!deleted) {
        setState(() => _busy = false);
        return;
      }
      _toast('Action deleted.', warning: false);
      Navigator.of(context).pop(true);
    } catch (e) {
      if (!mounted) return;
      _toastError('Could not delete action: $e');
      setState(() => _busy = false);
    }
  }

  void _toast(String message, {bool warning = true}) {
    showAppToast(context, message: message, warning: warning);
  }

  void _toggleDropdown(_CustomActionDropdown dropdown) {
    setState(() {
      _openDropdown = _openDropdown == dropdown ? null : dropdown;
    });
  }

  void _toastError(String message) {
    _toast(message);
  }
}

class _DismissIntent extends Intent {
  const _DismissIntent();
}

class _SubmitIntent extends Intent {
  const _SubmitIntent();
}

class _FieldLabel extends StatelessWidget {
  const _FieldLabel(this.text);
  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(0, 14, 0, 8),
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

class _TextFieldShell extends StatelessWidget {
  const _TextFieldShell({
    required this.controller,
    required this.placeholder,
    this.multiline = false,
    this.minHeight,
    this.autofocus = false,
  });

  final TextEditingController controller;
  final String placeholder;
  final bool multiline;
  final double? minHeight;
  final bool autofocus;

  @override
  Widget build(BuildContext context) {
    final h = minHeight ?? 38;
    return Container(
      constraints: BoxConstraints(minHeight: h),
      decoration: BoxDecoration(
        color: AppTokens.overlayHover,
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        border: Border.all(color: AppTokens.border),
      ),
      padding: EdgeInsets.symmetric(
        horizontal: 12,
        vertical: multiline ? 10 : 8,
      ),
      child: TextField(
        controller: controller,
        autofocus: autofocus,
        maxLines: multiline ? null : 1,
        minLines: multiline ? 6 : 1,
        keyboardType: multiline ? TextInputType.multiline : TextInputType.text,
        style: const TextStyle(fontSize: 13, color: AppTokens.textPrimary),
        cursorColor: AppTokens.textPrimary,
        decoration: InputDecoration(
          isDense: true,
          contentPadding: EdgeInsets.zero,
          border: InputBorder.none,
          hintText: placeholder,
          hintStyle: const TextStyle(fontSize: 13, color: AppTokens.textMuted),
        ),
      ),
    );
  }
}

class _IconPickerButton extends StatelessWidget {
  const _IconPickerButton({
    required this.icon,
    required this.selected,
    required this.onTap,
  });

  final ProjectActionIconDto icon;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: _iconLabel(icon),
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        onTap: onTap,
        child: Container(
          width: 34,
          height: 30,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: selected ? AppTokens.overlayActive : AppTokens.overlayHover,
            borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            border: Border.all(
              color: selected ? const Color(0xFF6F86CB) : AppTokens.border,
            ),
          ),
          child: SvgPicture.asset(
            _iconAssetPath(icon),
            width: 14,
            height: 14,
            colorFilter: const ColorFilter.mode(
              AppTokens.textPrimary,
              BlendMode.srcIn,
            ),
          ),
        ),
      ),
    );
  }
}

class _DropdownPicker<T> extends StatelessWidget {
  const _DropdownPicker({
    required this.value,
    required this.label,
    required this.open,
    required this.onToggle,
    required this.options,
    required this.optionLabel,
    required this.onChanged,
    this.optionBuilder,
  });

  final T value;
  final String label;
  final bool open;
  final VoidCallback onToggle;
  final List<T> options;
  final String Function(T) optionLabel;
  final ValueChanged<T> onChanged;
  final Widget Function(BuildContext, T, bool)? optionBuilder;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        InkWell(
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
          onTap: onToggle,
          child: Container(
            height: 38,
            padding: const EdgeInsets.symmetric(horizontal: 12),
            decoration: BoxDecoration(
              color: AppTokens.overlayHover,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              border: Border.all(
                color: open ? const Color(0xFF6F86CB) : AppTokens.border,
              ),
            ),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    label,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: 13,
                      color: AppTokens.textPrimary,
                    ),
                  ),
                ),
                SvgPicture.asset(
                  'assets/icons/icons__chevron-down.svg',
                  width: 14,
                  height: 14,
                  colorFilter: const ColorFilter.mode(
                    AppTokens.textMuted,
                    BlendMode.srcIn,
                  ),
                ),
              ],
            ),
          ),
        ),
        if (open)
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
                  for (final option in options)
                    InkWell(
                      onTap: () {
                        onChanged(option);
                      },
                      child: Container(
                        height: optionBuilder != null ? 36 : 32,
                        padding: const EdgeInsets.symmetric(horizontal: 12),
                        alignment: Alignment.centerLeft,
                        color: option == value
                            ? AppTokens.overlayActive
                            : Colors.transparent,
                        child: optionBuilder != null
                            ? optionBuilder!(context, option, option == value)
                            : Text(
                                optionLabel(option),
                                style: const TextStyle(
                                  fontSize: 13,
                                  color: AppTokens.textPrimary,
                                ),
                              ),
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

class _ProviderOption extends StatelessWidget {
  const _ProviderOption({required this.provider, required this.selected});
  final AgentProvider provider;
  final bool selected;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        SvgPicture.asset(
          _providerIconPath(provider),
          width: 18,
          height: 18,
          colorFilter: const ColorFilter.mode(
            AppTokens.textPrimary,
            BlendMode.srcIn,
          ),
        ),
        const SizedBox(width: 10),
        Text(
          _providerLabel(provider),
          style: const TextStyle(fontSize: 13, color: AppTokens.textPrimary),
        ),
      ],
    );
  }
}

class _Toggle extends StatelessWidget {
  const _Toggle({
    required this.label,
    required this.value,
    required this.onChanged,
  });

  final String label;
  final bool value;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.radiusMd),
      onTap: () => onChanged(!value),
      child: Container(
        height: 34,
        padding: const EdgeInsets.symmetric(horizontal: 8),
        child: Row(
          children: [
            Container(
              width: 18,
              height: 18,
              decoration: BoxDecoration(
                color: value ? const Color(0xFF6F86CB) : Colors.transparent,
                border: Border.all(
                  color: value ? const Color(0xFF6F86CB) : AppTokens.border,
                ),
                borderRadius: BorderRadius.circular(AppTokens.radiusXs),
              ),
              alignment: Alignment.center,
              child: value
                  ? SvgPicture.asset(
                      'assets/icons/icons__check.svg',
                      width: 12,
                      height: 12,
                      colorFilter: const ColorFilter.mode(
                        Colors.white,
                        BlendMode.srcIn,
                      ),
                    )
                  : null,
            ),
            const SizedBox(width: 10),
            Text(
              label,
              style: const TextStyle(
                fontSize: 13,
                color: AppTokens.textPrimary,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _GhostButton extends StatelessWidget {
  const _GhostButton({required this.label, required this.onTap});
  final String label;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.radiusMd),
      onTap: onTap,
      child: Container(
        height: 32,
        padding: const EdgeInsets.symmetric(horizontal: 14),
        alignment: Alignment.center,
        child: Text(
          label,
          style: const TextStyle(fontSize: 13, color: AppTokens.textPrimary),
        ),
      ),
    );
  }
}

class _PrimaryButton extends StatelessWidget {
  const _PrimaryButton({
    required this.label,
    required this.busy,
    required this.onTap,
  });

  final String label;
  final bool busy;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.radiusMd),
      onTap: onTap,
      child: Container(
        height: 32,
        padding: const EdgeInsets.symmetric(horizontal: 14),
        alignment: Alignment.center,
        decoration: BoxDecoration(
          color: const Color(0xFF6F86CB),
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        ),
        child: busy
            ? const SizedBox(
                width: 14,
                height: 14,
                child: CircularProgressIndicator(
                  strokeWidth: 2,
                  valueColor: AlwaysStoppedAnimation<Color>(Colors.white),
                ),
              )
            : Text(
                label,
                style: const TextStyle(
                  fontSize: 13,
                  fontWeight: FontWeight.w600,
                  color: Colors.white,
                ),
              ),
      ),
    );
  }
}

class _DeleteButton extends StatelessWidget {
  const _DeleteButton({required this.busy, required this.onTap});
  final bool busy;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    const danger = Color(0xFFEB6F77);
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.radiusMd),
      onTap: busy ? null : onTap,
      child: Container(
        height: 32,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        ),
        child: Row(
          children: [
            SvgPicture.asset(
              'assets/icons/icons__discard.svg',
              width: 13,
              height: 13,
              colorFilter: const ColorFilter.mode(danger, BlendMode.srcIn),
            ),
            const SizedBox(width: 6),
            const Text(
              'Delete',
              style: TextStyle(
                fontSize: 13,
                fontWeight: FontWeight.w500,
                color: danger,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// ── Helpers (mirror desktop/src/custom_actions_modal.rs) ───────

String _iconAssetPath(ProjectActionIconDto icon) {
  switch (icon) {
    case ProjectActionIconDto.play:
      return 'assets/icons/action__play.svg';
    case ProjectActionIconDto.test:
      return 'assets/icons/action__test.svg';
    case ProjectActionIconDto.lint:
      return 'assets/icons/action__lint.svg';
    case ProjectActionIconDto.configure:
      return 'assets/icons/action__configure.svg';
    case ProjectActionIconDto.build:
      return 'assets/icons/action__build.svg';
    case ProjectActionIconDto.debug:
      return 'assets/icons/action__debug.svg';
    case ProjectActionIconDto.agent:
      return 'assets/icons/action__agent.svg';
  }
}

String _iconLabel(ProjectActionIconDto icon) {
  return switch (icon) {
    ProjectActionIconDto.play => 'Play',
    ProjectActionIconDto.test => 'Test',
    ProjectActionIconDto.lint => 'Lint',
    ProjectActionIconDto.configure => 'Configure',
    ProjectActionIconDto.build => 'Build',
    ProjectActionIconDto.debug => 'Debug',
    ProjectActionIconDto.agent => 'Agent',
  };
}

String _providerLabel(AgentProvider provider) {
  return switch (provider) {
    AgentProvider.claudeCode => 'Claude Code',
    AgentProvider.codex => 'Codex',
    AgentProvider.cursorAgent => 'Cursor Agent',
    AgentProvider.pi => 'Pi',
    AgentProvider.gemini => 'Gemini',
    AgentProvider.openCode => 'Open Code',
    AgentProvider.amp => 'Amp',
    AgentProvider.rovoDev => 'Rovo Dev',
    AgentProvider.forge => 'Forge',
    AgentProvider.shell => 'Shell',
  };
}

String _providerIconPath(AgentProvider provider) {
  return switch (provider) {
    AgentProvider.claudeCode => 'assets/icons/icons__claude-ai.svg',
    AgentProvider.codex => 'assets/icons/icons__codex-ai.svg',
    AgentProvider.cursorAgent => 'assets/icons/icons__cursor-ai.svg',
    AgentProvider.gemini => 'assets/icons/icons__gemini-ai.svg',
    _ => 'assets/icons/action__agent.svg',
  };
}

String _accessLabel(ProjectActionAccessDto access) {
  return switch (access) {
    ProjectActionAccessDto.default_ => 'Default',
    ProjectActionAccessDto.readOnly => 'Read only',
    ProjectActionAccessDto.workspaceWrite => 'Workspace write',
    ProjectActionAccessDto.fullAccess => 'Full access',
  };
}

String _modeLabel(String value) {
  return switch (value) {
    'default' => 'Build',
    'plan' => 'Plan',
    _ => 'Default',
  };
}

class _Option {
  const _Option(this.value, this.label);
  final String value;
  final String label;
}

const _codexModelOptions = <_Option>[
  _Option('gpt-5.4', 'GPT-5.4'),
  _Option('gpt-5.4-mini', 'GPT-5.4 Mini'),
  _Option('gpt-5.3-codex', 'GPT-5.3 Codex'),
  _Option('gpt-5.3-codex-spark', 'GPT-5.3 Codex Spark'),
];

const _claudeModelOptions = <_Option>[
  _Option('claude-opus-4-7', 'Claude Opus 4.7'),
  _Option('claude-opus-4-6', 'Claude Opus 4.6'),
  _Option('claude-opus-4-5', 'Claude Opus 4.5'),
  _Option('claude-sonnet-4-6', 'Claude Sonnet 4.6'),
  _Option('claude-haiku-4-5', 'Claude Haiku 4.5'),
];

const _codexTraitsOptions = <_Option>[
  _Option('xhigh', 'Extra high'),
  _Option('high', 'High'),
  _Option('medium', 'Medium'),
  _Option('low', 'Low'),
];

const _claudeOpus47TraitsOptions = <_Option>[
  _Option('low', 'Low'),
  _Option('medium', 'Medium'),
  _Option('high', 'High'),
  _Option('xhigh', 'Extra high'),
  _Option('max', 'Max'),
  _Option('ultrathink', 'Ultrathink'),
];

const _claudeOpus46TraitsOptions = <_Option>[
  _Option('low', 'Low'),
  _Option('medium', 'Medium'),
  _Option('high', 'High'),
  _Option('max', 'Max'),
  _Option('ultrathink', 'Ultrathink'),
];

const _claudeOpus45TraitsOptions = <_Option>[
  _Option('low', 'Low'),
  _Option('medium', 'Medium'),
  _Option('high', 'High'),
  _Option('max', 'Max'),
];

const _claudeSonnet46TraitsOptions = <_Option>[
  _Option('low', 'Low'),
  _Option('medium', 'Medium'),
  _Option('high', 'High'),
  _Option('ultrathink', 'Ultrathink'),
];

List<_Option> _modelOptions(AgentProvider provider, String current) {
  final base = <_Option>[const _Option('', 'Default')];
  final source = switch (provider) {
    AgentProvider.codex => _codexModelOptions,
    AgentProvider.claudeCode => _claudeModelOptions,
    _ => <_Option>[],
  };
  base.addAll(source);
  _appendCurrent(base, current);
  return base;
}

List<_Option> _traitsOptions(
  AgentProvider provider,
  String model,
  String current,
) {
  final base = <_Option>[const _Option('', 'Default')];
  final source = switch (provider) {
    AgentProvider.codex => _codexTraitsOptions,
    AgentProvider.claudeCode => switch (model) {
      'claude-opus-4-7' => _claudeOpus47TraitsOptions,
      'claude-opus-4-6' => _claudeOpus46TraitsOptions,
      'claude-opus-4-5' => _claudeOpus45TraitsOptions,
      'claude-sonnet-4-6' => _claudeSonnet46TraitsOptions,
      _ => <_Option>[],
    },
    _ => <_Option>[],
  };
  base.addAll(source);
  _appendCurrent(base, current);
  return base;
}

void _appendCurrent(List<_Option> options, String current) {
  final trimmed = current.trim();
  if (trimmed.isEmpty) return;
  if (options.any((o) => o.value == trimmed)) return;
  options.add(_Option(trimmed, trimmed));
}

String _modelLabel(AgentProvider provider, String value) {
  return _optionLabel(_modelOptions(provider, value), value);
}

String _traitsLabel(AgentProvider provider, String model, String value) {
  return _optionLabel(_traitsOptions(provider, model, value), value);
}

String _optionLabel(List<_Option> options, String value) {
  for (final o in options) {
    if (o.value == value) return o.label;
  }
  final trimmed = value.trim();
  return trimmed.isEmpty ? 'Default' : trimmed;
}

String? _trimToOption(String value) {
  final trimmed = value.trim();
  return trimmed.isEmpty ? null : trimmed;
}
