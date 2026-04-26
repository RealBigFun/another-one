// Keybindings settings sub-page — port of
// `desktop/src/settings_page.rs::settings_keybindings_content`.
//
// Lists every shortcut action with its current binding rendered
// as a row of modifier+key pills. Click a row to start capturing:
// the next non-modifier keystroke is recorded. Reset reverts the
// row to its built-in default; Clear empties it (the action goes
// inert until rebound).

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../rust/api/local_session.dart' show ShortcutSettingsRow;
import '../../../state/local_connection_provider.dart';
import '../../../tokens.dart';

final _shortcutSettingsProvider = FutureProvider.autoDispose((ref) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.readShortcutSettings();
  } on UnimplementedError {
    return null;
  }
});

class SettingsKeybindingsSection extends ConsumerStatefulWidget {
  const SettingsKeybindingsSection({super.key});

  @override
  ConsumerState<SettingsKeybindingsSection> createState() =>
      _SettingsKeybindingsSectionState();
}

class _SettingsKeybindingsSectionState
    extends ConsumerState<SettingsKeybindingsSection> {
  static const Color _panelBg = Color(0xFF23252A);
  static const Color _rowBg = Color(0xFF1F2125);
  static const Color _activeBg = Color(0xFF2E67B8);

  String? _capturingActionId;
  final FocusNode _focusNode = FocusNode();

  @override
  void dispose() {
    _focusNode.dispose();
    super.dispose();
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (_capturingActionId == null) return KeyEventResult.ignored;
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final key = event.logicalKey;
    if (_isModifierKey(key)) return KeyEventResult.handled;
    if (key == LogicalKeyboardKey.escape) {
      setState(() => _capturingActionId = null);
      return KeyEventResult.handled;
    }
    final binding = _bindingString(event);
    if (binding == null) return KeyEventResult.handled;
    final actionId = _capturingActionId!;
    setState(() => _capturingActionId = null);
    unawaited(_setBinding(actionId, binding));
    return KeyEventResult.handled;
  }

  Future<void> _setBinding(String actionId, String binding) async {
    try {
      await ref.read(localConnectionProvider).setShortcutBinding(
            actionId: actionId,
            binding: binding,
          );
      ref.invalidate(_shortcutSettingsProvider);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Could not set binding: $e'),
          backgroundColor: AppTokens.errorBg,
        ),
      );
    }
  }

  Future<void> _resetBinding(String actionId) async {
    try {
      await ref
          .read(localConnectionProvider)
          .resetShortcutBinding(actionId);
      ref.invalidate(_shortcutSettingsProvider);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Could not reset binding: $e'),
          backgroundColor: AppTokens.errorBg,
        ),
      );
    }
  }

  Future<void> _clearBinding(String actionId) async {
    try {
      await ref.read(localConnectionProvider).setShortcutBinding(
            actionId: actionId,
            binding: '',
          );
      ref.invalidate(_shortcutSettingsProvider);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Could not clear binding: $e'),
          backgroundColor: AppTokens.errorBg,
        ),
      );
    }
  }

  void _startCapture(String actionId) {
    setState(() => _capturingActionId = actionId);
    _focusNode.requestFocus();
  }

  @override
  Widget build(BuildContext context) {
    final settingsAsync = ref.watch(_shortcutSettingsProvider);
    final view = settingsAsync.valueOrNull;
    final actions =
        view?.actions ?? const <ShortcutSettingsRow>[];
    return Focus(
      focusNode: _focusNode,
      onKeyEvent: _onKey,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text(
            'Keybindings',
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
              'Click a binding to record a new keystroke. Modifiers (Cmd/Ctrl/Shift/Alt) capture along with the next non-modifier key. Esc cancels capture without saving.',
              style: TextStyle(
                fontSize: 12,
                height: 1.5,
                color: AppTokens.textSecondary,
              ),
            ),
          ),
          const SizedBox(height: 24),
          if (view == null)
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 24),
              child: Center(
                child: SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
              ),
            )
          else
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
                    for (var i = 0; i < actions.length; i++)
                      _ShortcutRow(
                        row: actions[i],
                        isFirst: i == 0,
                        capturing:
                            actions[i].id == _capturingActionId,
                        activeBg: _activeBg,
                        panelBg: _panelBg,
                        onCapture: () => _startCapture(actions[i].id),
                        onReset: () => _resetBinding(actions[i].id),
                        onClear: () => _clearBinding(actions[i].id),
                      ),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }

  bool _isModifierKey(LogicalKeyboardKey key) {
    return key == LogicalKeyboardKey.shift ||
        key == LogicalKeyboardKey.shiftLeft ||
        key == LogicalKeyboardKey.shiftRight ||
        key == LogicalKeyboardKey.control ||
        key == LogicalKeyboardKey.controlLeft ||
        key == LogicalKeyboardKey.controlRight ||
        key == LogicalKeyboardKey.alt ||
        key == LogicalKeyboardKey.altLeft ||
        key == LogicalKeyboardKey.altRight ||
        key == LogicalKeyboardKey.meta ||
        key == LogicalKeyboardKey.metaLeft ||
        key == LogicalKeyboardKey.metaRight;
  }

  String? _bindingString(KeyEvent event) {
    final modifiers = <String>[];
    if (HardwareKeyboard.instance.isMetaPressed) {
      modifiers.add(defaultTargetPlatform == TargetPlatform.macOS
          ? 'cmd'
          : 'super');
    }
    if (HardwareKeyboard.instance.isControlPressed) {
      modifiers.add(defaultTargetPlatform == TargetPlatform.macOS
          ? 'control'
          : 'cmd'); // GPUI uses cmd for control on macOS
    }
    if (HardwareKeyboard.instance.isAltPressed) modifiers.add('alt');
    if (HardwareKeyboard.instance.isShiftPressed) modifiers.add('shift');
    final key = _normalizeKey(event);
    if (key == null) return null;
    return [...modifiers, key].join('-');
  }

  String? _normalizeKey(KeyEvent event) {
    final char = event.character;
    if (char != null && char.length == 1 && char != ' ') {
      return char.toLowerCase();
    }
    final key = event.logicalKey;
    if (key == LogicalKeyboardKey.space) return 'space';
    if (key == LogicalKeyboardKey.enter) return 'enter';
    if (key == LogicalKeyboardKey.tab) return 'tab';
    if (key == LogicalKeyboardKey.backspace) return 'backspace';
    if (key == LogicalKeyboardKey.arrowLeft) return 'left';
    if (key == LogicalKeyboardKey.arrowRight) return 'right';
    if (key == LogicalKeyboardKey.arrowUp) return 'up';
    if (key == LogicalKeyboardKey.arrowDown) return 'down';
    if (key == LogicalKeyboardKey.f1) return 'f1';
    if (key == LogicalKeyboardKey.f2) return 'f2';
    if (key == LogicalKeyboardKey.f3) return 'f3';
    if (key == LogicalKeyboardKey.f4) return 'f4';
    if (key == LogicalKeyboardKey.f5) return 'f5';
    if (key == LogicalKeyboardKey.f6) return 'f6';
    if (key == LogicalKeyboardKey.f7) return 'f7';
    if (key == LogicalKeyboardKey.f8) return 'f8';
    if (key == LogicalKeyboardKey.f9) return 'f9';
    if (key == LogicalKeyboardKey.f10) return 'f10';
    if (key == LogicalKeyboardKey.f11) return 'f11';
    if (key == LogicalKeyboardKey.f12) return 'f12';
    return null;
  }
}

class _ShortcutRow extends StatefulWidget {
  const _ShortcutRow({
    required this.row,
    required this.isFirst,
    required this.capturing,
    required this.activeBg,
    required this.panelBg,
    required this.onCapture,
    required this.onReset,
    required this.onClear,
  });

  final ShortcutSettingsRow row;
  final bool isFirst;
  final bool capturing;
  final Color activeBg;
  final Color panelBg;
  final VoidCallback onCapture;
  final Future<void> Function() onReset;
  final Future<void> Function() onClear;

  @override
  State<_ShortcutRow> createState() => _ShortcutRowState();
}

class _ShortcutRowState extends State<_ShortcutRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final row = widget.row;
    final usingDefault = row.currentBinding == row.defaultBinding;
    final empty = row.currentBinding.isEmpty;
    return Container(
      decoration: BoxDecoration(
        border: widget.isFirst
            ? null
            : const Border(top: BorderSide(color: AppTokens.border)),
      ),
      padding:
          const EdgeInsets.symmetric(horizontal: 18, vertical: 12),
      child: MouseRegion(
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: Row(
          children: [
            Expanded(
              child: Text(
                row.label,
                style: const TextStyle(
                  fontSize: 13,
                  fontWeight: FontWeight.w500,
                  color: AppTokens.textPrimary,
                ),
              ),
            ),
            const SizedBox(width: 16),
            if (widget.capturing)
              Container(
                height: 30,
                padding: const EdgeInsets.symmetric(horizontal: 12),
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  color: widget.activeBg.withValues(alpha: 0.18),
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(
                    color: widget.activeBg.withValues(alpha: 0.85),
                  ),
                ),
                child: const Text(
                  'Press a key…',
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w500,
                    color: AppTokens.textPrimary,
                  ),
                ),
              )
            else
              InkWell(
                borderRadius: BorderRadius.circular(8),
                onTap: widget.onCapture,
                child: Container(
                  height: 30,
                  padding: const EdgeInsets.symmetric(horizontal: 8),
                  alignment: Alignment.center,
                  decoration: BoxDecoration(
                    color: _hover
                        ? AppTokens.overlayHoverStrong
                        : AppTokens.overlayHover,
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: AppTokens.border),
                  ),
                  child: empty
                      ? const Text(
                          'Unbound',
                          style: TextStyle(
                            fontSize: 12,
                            color: AppTokens.textMuted,
                          ),
                        )
                      : _BindingPills(binding: row.currentBinding),
                ),
              ),
            const SizedBox(width: 10),
            if (!empty)
              _IconAction(
                tooltip: 'Clear binding',
                iconChar: '×',
                onTap: widget.onClear,
              ),
            const SizedBox(width: 6),
            _IconAction(
              tooltip: 'Reset to default',
              iconChar: '↺',
              dimmed: usingDefault,
              onTap: widget.onReset,
            ),
          ],
        ),
      ),
    );
  }
}

class _BindingPills extends StatelessWidget {
  const _BindingPills({required this.binding});
  final String binding;

  @override
  Widget build(BuildContext context) {
    final parts = binding.split('-');
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (var i = 0; i < parts.length; i++) ...[
          if (i > 0) const SizedBox(width: 4),
          Container(
            padding: const EdgeInsets.symmetric(
              horizontal: 6,
              vertical: 2,
            ),
            decoration: BoxDecoration(
              color: const Color(0xFF2A2D33),
              borderRadius: BorderRadius.circular(4),
              border: Border.all(color: AppTokens.border),
            ),
            child: Text(
              _displayKey(parts[i]),
              style: const TextStyle(
                fontSize: 11,
                fontFamily: AppTokens.fontFamilyMono,
                color: AppTokens.textPrimary,
              ),
            ),
          ),
        ],
      ],
    );
  }

  String _displayKey(String part) {
    return switch (part) {
      'cmd' => '⌘',
      'control' => '⌃',
      'ctrl' => '⌃',
      'alt' => '⌥',
      'option' => '⌥',
      'shift' => '⇧',
      'super' => 'Super',
      _ => part.toUpperCase(),
    };
  }
}

class _IconAction extends StatefulWidget {
  const _IconAction({
    required this.tooltip,
    required this.iconChar,
    required this.onTap,
    this.dimmed = false,
  });

  final String tooltip;
  final String iconChar;
  final Future<void> Function() onTap;
  final bool dimmed;

  @override
  State<_IconAction> createState() => _IconActionState();
}

class _IconActionState extends State<_IconAction> {
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
          onTap: () => unawaited(widget.onTap()),
          child: Container(
            width: 24,
            height: 24,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: _hover
                  ? AppTokens.overlayHoverStrong
                  : Colors.transparent,
              borderRadius: BorderRadius.circular(5),
            ),
            child: Text(
              widget.iconChar,
              style: TextStyle(
                fontSize: 14,
                color: widget.dimmed
                    ? AppTokens.textMuted
                    : AppTokens.textSecondary,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
