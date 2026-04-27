// Agents settings sub-page — full port of
// `desktop/src/settings_page.rs::settings_agents_content`.
//
// Per-agent rows with branded glyph + label + description, a
// launch-args input + Add button + chip list under it, a Make
// default / Default radio pill, and an Enabled / Disabled toggle.
// Reads from `agentSettingsProvider`; mutations (toggle enabled,
// set default, add/remove launch arg) call through
// `LocalSession` and invalidate the provider so the rebuild
// picks up the new state.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../../../rust/api/local_session.dart' show AgentSettingsRow;
import '../../../state/local_connection_provider.dart';
import '../../../state/new_task_data_provider.dart';
import '../../../tokens.dart';
import '../../../widgets/app_toast.dart';
import 'settings_async_state.dart';

class SettingsAgentsSection extends ConsumerWidget {
  const SettingsAgentsSection({super.key});

  static const Color _panelBg = Color(0xFF23252A);
  static const Color _rowBg = Color(0xFF1F2125);
  static const Color _activeBg = Color(0xFF2E67B8);

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settingsAsync = ref.watch(agentSettingsProvider);
    final body = settingsAsync.when<Widget>(
      data: (view) {
        if (view == null) {
          return ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 860),
            child: const SettingsSectionStatePanel(
              panelBg: _panelBg,
              title: 'Not available on this connection',
              message: 'This daemon does not expose agent settings yet.',
            ),
          );
        }
        final enabledCount = view.agents.where((a) => a.enabled).length;
        return Column(
          children: [
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 860),
              child: _AvailabilityPanel(
                enabledCount: enabledCount,
                panelBg: _panelBg,
              ),
            ),
            const SizedBox(height: 12),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 860),
              child: _TokenRulesPanel(panelBg: _panelBg),
            ),
            const SizedBox(height: 16),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 860),
              child: Container(
                decoration: BoxDecoration(
                  color: _rowBg,
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(color: AppTokens.border),
                ),
                clipBehavior: Clip.antiAlias,
                child: Column(
                  children: [
                    for (var i = 0; i < view.agents.length; i++)
                      _AgentRow(
                        row: view.agents[i],
                        isFirst: i == 0,
                        activeBg: _activeBg,
                        onChanged: () => ref.invalidate(agentSettingsProvider),
                      ),
                  ],
                ),
              ),
            ),
          ],
        );
      },
      error: (error, _) => ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 860),
        child: SettingsSectionStatePanel(
          panelBg: _panelBg,
          title: 'Could not load agent settings',
          message: '$error',
          error: true,
        ),
      ),
      loading: SettingsSectionLoading.new,
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text(
          'Agents',
          style: TextStyle(
            fontSize: 18,
            fontWeight: FontWeight.w600,
            color: AppTokens.textPrimary,
          ),
        ),
        const SizedBox(height: 4),
        ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 760),
          child: const Text(
            'Manage per-agent argv tokens and availability. Disabled agents stay here so they can be re-enabled, but they are hidden from New Task and Add Agent pickers. Changes save immediately.',
            style: TextStyle(
              fontSize: 12,
              height: 1.5,
              color: AppTokens.textSecondary,
            ),
          ),
        ),
        const SizedBox(height: 24),
        body,
      ],
    );
  }
}

class _AvailabilityPanel extends StatelessWidget {
  const _AvailabilityPanel({required this.enabledCount, required this.panelBg});
  final int enabledCount;
  final Color panelBg;

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        color: panelBg,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTokens.border),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      child: Row(
        children: [
          const Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Availability',
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                    color: AppTokens.textPrimary,
                  ),
                ),
                SizedBox(height: 4),
                Text(
                  'Choose which enabled agent is used first for new tasks and new agent tabs. Disabled agents can still be re-enabled and edited here.',
                  style: TextStyle(
                    fontSize: 11,
                    color: AppTokens.textSecondary,
                  ),
                ),
              ],
            ),
          ),
          Text(
            '$enabledCount enabled',
            style: const TextStyle(
              fontSize: 11,
              fontWeight: FontWeight.w500,
              color: AppTokens.textPrimary,
            ),
          ),
        ],
      ),
    );
  }
}

class _TokenRulesPanel extends StatelessWidget {
  const _TokenRulesPanel({required this.panelBg});
  final Color panelBg;

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        color: panelBg,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTokens.border),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: const [
          Text(
            'Token rules',
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w600,
              color: AppTokens.textPrimary,
            ),
          ),
          SizedBox(height: 4),
          Text(
            'Whitespace is rejected because spaces would create multiple argv tokens. Reorder by removing and re-adding.',
            style: TextStyle(fontSize: 11, color: AppTokens.textSecondary),
          ),
        ],
      ),
    );
  }
}

class _AgentRow extends ConsumerStatefulWidget {
  const _AgentRow({
    required this.row,
    required this.isFirst,
    required this.activeBg,
    required this.onChanged,
  });
  final AgentSettingsRow row;
  final bool isFirst;
  final Color activeBg;
  final VoidCallback onChanged;

  @override
  ConsumerState<_AgentRow> createState() => _AgentRowState();
}

class _AgentRowState extends ConsumerState<_AgentRow> {
  static final RegExp _whitespacePattern = RegExp(r'\s');

  late final TextEditingController _draft;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _draft = TextEditingController();
  }

  @override
  void dispose() {
    _draft.dispose();
    super.dispose();
  }

  Future<void> _addArg() async {
    final value = _draft.text.trim();
    if (value.isEmpty || _busy) return;
    if (_whitespacePattern.hasMatch(value)) {
      _toast('Launch args must be a single token.');
      return;
    }
    final current = List<String>.from(widget.row.launchArgs);
    if (current.contains(value)) {
      _draft.clear();
      return;
    }
    current.add(value);
    setState(() => _busy = true);
    try {
      await ref
          .read(localConnectionProvider)
          .setAgentLaunchArgs(agentId: widget.row.id, args: current);
      _draft.clear();
      widget.onChanged();
    } catch (e) {
      if (!mounted) return;
      _toast('Could not save args: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _removeArg(int index) async {
    if (_busy) return;
    final current = List<String>.from(widget.row.launchArgs)..removeAt(index);
    setState(() => _busy = true);
    try {
      await ref
          .read(localConnectionProvider)
          .setAgentLaunchArgs(agentId: widget.row.id, args: current);
      widget.onChanged();
    } catch (e) {
      if (!mounted) return;
      _toast('Could not remove arg: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _toggleEnabled() async {
    if (_busy) return;
    setState(() => _busy = true);
    try {
      await ref
          .read(localConnectionProvider)
          .setAgentEnabled(
            agentId: widget.row.id,
            enabled: !widget.row.enabled,
          );
      widget.onChanged();
    } catch (e) {
      if (!mounted) return;
      _toast('Could not toggle: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _makeDefault() async {
    if (_busy || widget.row.isDefault || !widget.row.enabled) return;
    setState(() => _busy = true);
    try {
      await ref.read(localConnectionProvider).setDefaultAgent(widget.row.id);
      widget.onChanged();
    } catch (e) {
      if (!mounted) return;
      _toast('Could not set default: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  void _toast(String message) {
    showAppToast(context, message: message);
  }

  @override
  Widget build(BuildContext context) {
    final row = widget.row;
    return Container(
      decoration: BoxDecoration(
        border: widget.isFirst
            ? null
            : const Border(top: BorderSide(color: AppTokens.border)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 540),
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.center,
                    children: [
                      _AgentGlyph(iconPath: row.iconPath, size: 18),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              row.label,
                              style: const TextStyle(
                                fontSize: 13,
                                fontWeight: FontWeight.w500,
                                color: AppTokens.textPrimary,
                              ),
                            ),
                            const SizedBox(height: 4),
                            Text(
                              'Extra argv tokens passed to ${row.label} on every launch and resume.',
                              style: const TextStyle(
                                fontSize: 11,
                                color: AppTokens.textSecondary,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                ),
              ),
              const SizedBox(width: 20),
              Flexible(
                child: Wrap(
                  alignment: WrapAlignment.end,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    _ArgInput(
                      controller: _draft,
                      enabled: !_busy,
                      onSubmit: _addArg,
                    ),
                    _AddButton(busy: _busy, onTap: _addArg),
                    _DefaultPill(
                      isDefault: row.isDefault,
                      enabled: row.enabled && !_busy,
                      activeBg: widget.activeBg,
                      onTap: _makeDefault,
                    ),
                    _EnabledPill(
                      enabled: row.enabled,
                      busy: _busy,
                      activeBg: widget.activeBg,
                      onTap: _toggleEnabled,
                    ),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          _ArgPills(args: row.launchArgs, onRemove: _removeArg),
        ],
      ),
    );
  }
}

class _ArgInput extends StatelessWidget {
  const _ArgInput({
    required this.controller,
    required this.enabled,
    required this.onSubmit,
  });

  final TextEditingController controller;
  final bool enabled;
  final Future<void> Function() onSubmit;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 180,
      height: 34,
      padding: const EdgeInsets.symmetric(horizontal: 10),
      decoration: BoxDecoration(
        color: AppTokens.overlayHover,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: AppTokens.border),
      ),
      child: TextField(
        controller: controller,
        enabled: enabled,
        style: const TextStyle(
          fontSize: 12,
          fontFamily: AppTokens.fontFamilyMono,
          color: AppTokens.textPrimary,
        ),
        cursorColor: AppTokens.textPrimary,
        decoration: const InputDecoration(
          isDense: true,
          contentPadding: EdgeInsets.symmetric(vertical: 9),
          border: InputBorder.none,
          hintText: '--flag=value',
          hintStyle: TextStyle(fontSize: 12, color: AppTokens.textPlaceholder),
        ),
        onSubmitted: (_) => unawaited(onSubmit()),
      ),
    );
  }
}

class _AddButton extends StatelessWidget {
  const _AddButton({required this.busy, required this.onTap});
  final bool busy;
  final Future<void> Function() onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      borderRadius: BorderRadius.circular(8),
      onTap: busy ? null : () => unawaited(onTap()),
      child: Container(
        height: 34,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        alignment: Alignment.center,
        decoration: BoxDecoration(
          color: AppTokens.overlayHover,
          borderRadius: BorderRadius.circular(8),
          border: Border.all(color: AppTokens.border),
        ),
        child: const Text(
          'Add',
          style: TextStyle(
            fontSize: 12,
            fontWeight: FontWeight.w500,
            color: AppTokens.textPrimary,
          ),
        ),
      ),
    );
  }
}

class _DefaultPill extends StatelessWidget {
  const _DefaultPill({
    required this.isDefault,
    required this.enabled,
    required this.activeBg,
    required this.onTap,
  });
  final bool isDefault;
  final bool enabled;
  final Color activeBg;
  final Future<void> Function() onTap;

  @override
  Widget build(BuildContext context) {
    final bg = isDefault ? activeBg : AppTokens.overlayHover;
    final textColor = isDefault ? Colors.white : AppTokens.textSecondary;
    return InkWell(
      borderRadius: BorderRadius.circular(8),
      onTap: enabled ? () => unawaited(onTap()) : null,
      child: Opacity(
        opacity: enabled ? 1.0 : 0.5,
        child: Container(
          height: 34,
          padding: const EdgeInsets.symmetric(horizontal: 10),
          decoration: BoxDecoration(
            color: bg,
            borderRadius: BorderRadius.circular(8),
            border: Border.all(
              color: isDefault
                  ? activeBg.withValues(alpha: 0.85)
                  : AppTokens.border,
            ),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                isDefault ? 'Default' : 'Make default',
                style: TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.w500,
                  color: textColor,
                ),
              ),
              const SizedBox(width: 8),
              Container(
                width: 16,
                height: 16,
                decoration: BoxDecoration(
                  color: isDefault
                      ? Colors.white.withValues(alpha: 0.16)
                      : Colors.transparent,
                  border: Border.all(
                    color: isDefault
                        ? Colors.white.withValues(alpha: 0.85)
                        : AppTokens.border,
                  ),
                  borderRadius: BorderRadius.circular(999),
                ),
                child: isDefault
                    ? Center(
                        child: Container(
                          width: 7,
                          height: 7,
                          decoration: const BoxDecoration(
                            color: Colors.white,
                            shape: BoxShape.circle,
                          ),
                        ),
                      )
                    : null,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _EnabledPill extends StatelessWidget {
  const _EnabledPill({
    required this.enabled,
    required this.busy,
    required this.activeBg,
    required this.onTap,
  });
  final bool enabled;
  final bool busy;
  final Color activeBg;
  final Future<void> Function() onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      borderRadius: BorderRadius.circular(8),
      onTap: busy ? null : () => unawaited(onTap()),
      child: Container(
        height: 34,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(
          color: AppTokens.overlayHover,
          borderRadius: BorderRadius.circular(8),
          border: Border.all(color: AppTokens.border),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              enabled ? 'Enabled' : 'Disabled',
              style: TextStyle(
                fontSize: 11,
                fontWeight: FontWeight.w500,
                color: enabled ? Colors.white : AppTokens.textSecondary,
              ),
            ),
            const SizedBox(width: 8),
            Container(
              width: 16,
              height: 16,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: enabled ? activeBg : Colors.transparent,
                border: Border.all(
                  color: enabled
                      ? activeBg.withValues(alpha: 0.85)
                      : AppTokens.border,
                ),
                borderRadius: BorderRadius.circular(4),
              ),
              child: enabled
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
          ],
        ),
      ),
    );
  }
}

class _ArgPills extends StatelessWidget {
  const _ArgPills({required this.args, required this.onRemove});
  final List<String> args;
  final ValueChanged<int> onRemove;

  @override
  Widget build(BuildContext context) {
    if (args.isEmpty) {
      return Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        decoration: BoxDecoration(
          color: const Color(0xFF2A2D33),
          borderRadius: BorderRadius.circular(8),
          border: Border.all(color: AppTokens.border),
        ),
        child: const Text(
          'No extra args',
          style: TextStyle(
            fontSize: 12,
            fontFamily: AppTokens.fontFamilyMono,
            color: AppTokens.textSecondary,
          ),
        ),
      );
    }
    return Wrap(
      spacing: 8,
      runSpacing: 8,
      children: [
        for (var i = 0; i < args.length; i++)
          _ArgPill(value: args[i], onRemove: () => onRemove(i)),
      ],
    );
  }
}

class _ArgPill extends StatelessWidget {
  const _ArgPill({required this.value, required this.onRemove});
  final String value;
  final VoidCallback onRemove;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.fromLTRB(10, 6, 4, 6),
      decoration: BoxDecoration(
        color: const Color(0xFF2A2D33),
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: AppTokens.border),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            value,
            style: const TextStyle(
              fontSize: 12,
              fontFamily: AppTokens.fontFamilyMono,
              color: AppTokens.textPrimary,
            ),
          ),
          const SizedBox(width: 8),
          InkWell(
            borderRadius: BorderRadius.circular(5),
            onTap: onRemove,
            child: Container(
              width: 18,
              height: 18,
              alignment: Alignment.center,
              child: const Text(
                'x',
                style: TextStyle(fontSize: 11, color: AppTokens.textSecondary),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _AgentGlyph extends StatelessWidget {
  const _AgentGlyph({required this.iconPath, required this.size});
  final String iconPath;
  final double size;

  @override
  Widget build(BuildContext context) {
    if (iconPath.endsWith('.svg')) {
      return SvgPicture.asset(
        iconPath,
        width: size,
        height: size,
        colorFilter: const ColorFilter.mode(
          AppTokens.textPrimary,
          BlendMode.srcIn,
        ),
      );
    }
    return Image.asset(iconPath, width: size, height: size);
  }
}
