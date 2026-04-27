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

import '../../../rust/api/local_session.dart'
    show ShortcutSettingsRow, ShortcutSettingsView;
import '../../../state/local_connection_provider.dart';
import '../../../tokens.dart';
import '../../../widgets/app_toast.dart';
import 'settings_async_state.dart';

final _shortcutSettingsProvider =
    FutureProvider.autoDispose<ShortcutSettingsView?>((ref) async {
      final connection = ref.watch(localConnectionProvider);
      await waitForConnectedDaemon(connection);
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
    final actionId = _capturingActionId!;
    if (_isModifierKey(key)) return KeyEventResult.handled;
    if (key == LogicalKeyboardKey.escape) {
      setState(() => _capturingActionId = null);
      return KeyEventResult.handled;
    }

    if (_isClearKey(key) && !_hasCaptureModifier()) {
      setState(() => _capturingActionId = null);
      unawaited(_clearBinding(actionId));
      return KeyEventResult.handled;
    }

    final binding = _captureBinding(event);
    if (binding == null) return KeyEventResult.handled;

    final conflict = _conflictingAction(actionId, binding);
    if (conflict != null) {
      _toast('${conflict.label} already uses that shortcut.');
      return KeyEventResult.handled;
    }

    setState(() => _capturingActionId = null);
    unawaited(_setBinding(actionId, binding));
    return KeyEventResult.handled;
  }

  Future<void> _setBinding(String actionId, String binding) async {
    final actionLabel = _actionLabel(actionId);
    try {
      await ref
          .read(localConnectionProvider)
          .setShortcutBinding(actionId: actionId, binding: binding);
      ref.invalidate(_shortcutSettingsProvider);
      if (!mounted) return;
      _toast(
        actionLabel == null ? 'Updated shortcut.' : 'Updated $actionLabel.',
        warning: false,
      );
    } catch (e) {
      if (!mounted) return;
      _toast(
        actionLabel == null
            ? 'Could not update shortcut: $e'
            : 'Could not update $actionLabel: $e',
      );
    }
  }

  Future<void> _resetBinding(String actionId) async {
    final actionLabel = _actionLabel(actionId);
    if (_capturingActionId == actionId) {
      setState(() => _capturingActionId = null);
    }
    try {
      await ref.read(localConnectionProvider).resetShortcutBinding(actionId);
      ref.invalidate(_shortcutSettingsProvider);
      if (!mounted) return;
      _toast(
        actionLabel == null ? 'Reset shortcut.' : 'Reset $actionLabel.',
        warning: false,
      );
    } catch (e) {
      if (!mounted) return;
      _toast(
        actionLabel == null
            ? 'Could not reset shortcut: $e'
            : 'Could not reset $actionLabel: $e',
      );
    }
  }

  Future<void> _clearBinding(String actionId) async {
    final actionLabel = _actionLabel(actionId);
    if (_capturingActionId == actionId) {
      setState(() => _capturingActionId = null);
    }
    try {
      await ref
          .read(localConnectionProvider)
          .setShortcutBinding(actionId: actionId, binding: '');
      ref.invalidate(_shortcutSettingsProvider);
      if (!mounted) return;
      _toast(
        actionLabel == null ? 'Cleared shortcut.' : 'Cleared $actionLabel.',
        warning: false,
      );
    } catch (e) {
      if (!mounted) return;
      _toast(
        actionLabel == null
            ? 'Could not clear shortcut: $e'
            : 'Could not clear $actionLabel: $e',
      );
    }
  }

  void _startCapture(String actionId) {
    setState(() => _capturingActionId = actionId);
    _focusNode.requestFocus();
  }

  void _toast(String message, {bool warning = true}) {
    showAppToast(context, message: message, warning: warning);
  }

  @override
  Widget build(BuildContext context) {
    final settingsAsync = ref.watch(_shortcutSettingsProvider);
    final body = settingsAsync.when<Widget>(
      data: (view) {
        if (view == null) {
          return ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 860),
            child: const SettingsSectionStatePanel(
              panelBg: _panelBg,
              title: 'Not available on this connection',
              message: 'This daemon does not expose shortcut settings yet.',
            ),
          );
        }
        final actions = view.actions;
        return ConstrainedBox(
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
                    capturing: actions[i].id == _capturingActionId,
                    activeBg: _activeBg,
                    panelBg: _panelBg,
                    onCapture: () => _startCapture(actions[i].id),
                    onReset: () => _resetBinding(actions[i].id),
                    onClear: () => _clearBinding(actions[i].id),
                  ),
              ],
            ),
          ),
        );
      },
      error: (error, _) => ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 860),
        child: SettingsSectionStatePanel(
          panelBg: _panelBg,
          title: 'Could not load shortcut settings',
          message: '$error',
          error: true,
        ),
      ),
      loading: SettingsSectionLoading.new,
    );
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
              'Click a binding to record a new shortcut. Use at least one non-Shift modifier key; duplicate shortcuts are blocked. Esc cancels capture, and bare Backspace/Delete clears the binding.',
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

  bool _isClearKey(LogicalKeyboardKey key) {
    return key == LogicalKeyboardKey.backspace ||
        key == LogicalKeyboardKey.delete;
  }

  bool _hasCaptureModifier() {
    return HardwareKeyboard.instance.isMetaPressed ||
        HardwareKeyboard.instance.isControlPressed ||
        HardwareKeyboard.instance.isAltPressed;
  }

  ShortcutSettingsRow? _actionRow(String actionId) {
    final actions = ref.read(_shortcutSettingsProvider).valueOrNull?.actions;
    if (actions == null) return null;
    for (final row in actions) {
      if (row.id == actionId) return row;
    }
    return null;
  }

  String? _actionLabel(String actionId) => _actionRow(actionId)?.label;

  ShortcutSettingsRow? _conflictingAction(String actionId, String binding) {
    final actions = ref.read(_shortcutSettingsProvider).valueOrNull?.actions;
    if (actions == null) return null;
    for (final row in actions) {
      if (row.id != actionId &&
          row.currentBinding.isNotEmpty &&
          row.currentBinding == binding) {
        return row;
      }
    }
    return null;
  }

  String? _captureBinding(KeyEvent event) {
    if (!_hasCaptureModifier()) {
      _toast('Use at least one modifier key.');
      return null;
    }

    final normalized = _normalizeKey(event);
    if (normalized == null) {
      _toast('That key is not supported for shortcuts.');
      return null;
    }

    final modifiers = <String>[];
    if (HardwareKeyboard.instance.isMetaPressed) {
      modifiers.add(
        defaultTargetPlatform == TargetPlatform.macOS ? 'cmd' : 'super',
      );
    }
    if (HardwareKeyboard.instance.isControlPressed) {
      modifiers.add(
        defaultTargetPlatform == TargetPlatform.macOS ? 'control' : 'cmd',
      ); // GPUI uses cmd for control on macOS
    }
    if (HardwareKeyboard.instance.isAltPressed) modifiers.add('alt');
    if (HardwareKeyboard.instance.isShiftPressed || normalized.impliedShift) {
      modifiers.add('shift');
    }
    return [...modifiers, normalized.token].join('-');
  }

  _NormalizedShortcutKey? _normalizeKey(KeyEvent event) {
    final key = event.logicalKey;
    if (key == LogicalKeyboardKey.space) {
      return const _NormalizedShortcutKey('space');
    }
    if (key == LogicalKeyboardKey.enter ||
        key == LogicalKeyboardKey.numpadEnter) {
      return const _NormalizedShortcutKey('enter');
    }
    if (key == LogicalKeyboardKey.tab) {
      return const _NormalizedShortcutKey('tab');
    }
    if (key == LogicalKeyboardKey.escape) {
      return const _NormalizedShortcutKey('escape');
    }
    if (key == LogicalKeyboardKey.backspace) {
      return const _NormalizedShortcutKey('backspace');
    }
    if (key == LogicalKeyboardKey.delete) {
      return const _NormalizedShortcutKey('delete');
    }
    if (key == LogicalKeyboardKey.arrowLeft) {
      return const _NormalizedShortcutKey('left');
    }
    if (key == LogicalKeyboardKey.arrowRight) {
      return const _NormalizedShortcutKey('right');
    }
    if (key == LogicalKeyboardKey.arrowUp) {
      return const _NormalizedShortcutKey('up');
    }
    if (key == LogicalKeyboardKey.arrowDown) {
      return const _NormalizedShortcutKey('down');
    }
    if (key == LogicalKeyboardKey.home) {
      return const _NormalizedShortcutKey('home');
    }
    if (key == LogicalKeyboardKey.end) {
      return const _NormalizedShortcutKey('end');
    }
    if (key == LogicalKeyboardKey.pageUp) {
      return const _NormalizedShortcutKey('pageup');
    }
    if (key == LogicalKeyboardKey.pageDown) {
      return const _NormalizedShortcutKey('pagedown');
    }
    if (key == LogicalKeyboardKey.f1) return const _NormalizedShortcutKey('f1');
    if (key == LogicalKeyboardKey.f2) return const _NormalizedShortcutKey('f2');
    if (key == LogicalKeyboardKey.f3) return const _NormalizedShortcutKey('f3');
    if (key == LogicalKeyboardKey.f4) return const _NormalizedShortcutKey('f4');
    if (key == LogicalKeyboardKey.f5) return const _NormalizedShortcutKey('f5');
    if (key == LogicalKeyboardKey.f6) return const _NormalizedShortcutKey('f6');
    if (key == LogicalKeyboardKey.f7) return const _NormalizedShortcutKey('f7');
    if (key == LogicalKeyboardKey.f8) return const _NormalizedShortcutKey('f8');
    if (key == LogicalKeyboardKey.f9) return const _NormalizedShortcutKey('f9');
    if (key == LogicalKeyboardKey.f10) {
      return const _NormalizedShortcutKey('f10');
    }
    if (key == LogicalKeyboardKey.f11) {
      return const _NormalizedShortcutKey('f11');
    }
    if (key == LogicalKeyboardKey.f12) {
      return const _NormalizedShortcutKey('f12');
    }

    final keyLabel = key.keyLabel;
    if (keyLabel.isNotEmpty) {
      final normalized = _normalizeKeyToken(keyLabel);
      if (normalized != null) return normalized;
    }

    final char = event.character;
    if (char != null) {
      final normalized = _normalizeKeyToken(char);
      if (normalized != null) return normalized;
    }

    return null;
  }

  // Mirror the old GPUI shortcut capture so shifted symbols round-trip
  // to the base key token used in stored bindings.
  _NormalizedShortcutKey? _normalizeKeyToken(String token) {
    if (token.isEmpty) return null;
    if (token.length == 1) {
      final ch = token[0];
      if (_alphaNumeric.hasMatch(ch)) {
        return _NormalizedShortcutKey(ch.toLowerCase());
      }
      return switch (ch) {
        '-' => const _NormalizedShortcutKey('minus'),
        '=' ||
        '[' ||
        ']' ||
        '\\' ||
        ';' ||
        '\'' ||
        ',' ||
        '.' ||
        '/' ||
        '`' => _NormalizedShortcutKey(ch),
        '_' => const _NormalizedShortcutKey('minus', impliedShift: true),
        '+' => const _NormalizedShortcutKey('=', impliedShift: true),
        '{' => const _NormalizedShortcutKey('[', impliedShift: true),
        '}' => const _NormalizedShortcutKey(']', impliedShift: true),
        '|' => const _NormalizedShortcutKey('\\', impliedShift: true),
        ':' => const _NormalizedShortcutKey(';', impliedShift: true),
        '"' => const _NormalizedShortcutKey('\'', impliedShift: true),
        '<' => const _NormalizedShortcutKey(',', impliedShift: true),
        '>' => const _NormalizedShortcutKey('.', impliedShift: true),
        '?' => const _NormalizedShortcutKey('/', impliedShift: true),
        '~' => const _NormalizedShortcutKey('`', impliedShift: true),
        '!' => const _NormalizedShortcutKey('1', impliedShift: true),
        '@' => const _NormalizedShortcutKey('2', impliedShift: true),
        '#' => const _NormalizedShortcutKey('3', impliedShift: true),
        r'$' => const _NormalizedShortcutKey('4', impliedShift: true),
        '%' => const _NormalizedShortcutKey('5', impliedShift: true),
        '^' => const _NormalizedShortcutKey('6', impliedShift: true),
        '&' => const _NormalizedShortcutKey('7', impliedShift: true),
        '*' => const _NormalizedShortcutKey('8', impliedShift: true),
        '(' => const _NormalizedShortcutKey('9', impliedShift: true),
        ')' => const _NormalizedShortcutKey('0', impliedShift: true),
        _ => null,
      };
    }

    return switch (token.toLowerCase()) {
      'minus' => const _NormalizedShortcutKey('minus'),
      _ => null,
    };
  }

  static final RegExp _alphaNumeric = RegExp(r'[A-Za-z0-9]');
}

class _NormalizedShortcutKey {
  const _NormalizedShortcutKey(this.token, {this.impliedShift = false});

  final String token;
  final bool impliedShift;
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
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 12),
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
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
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
    final normalized = part.toLowerCase();
    if (defaultTargetPlatform == TargetPlatform.macOS) {
      return switch (normalized) {
        'cmd' => '⌘',
        'control' => '⌃',
        'ctrl' => '⌃',
        'alt' => '⌥',
        'option' => '⌥',
        'shift' => '⇧',
        'function' => 'Fn',
        'up' => 'Up',
        'down' => 'Down',
        'left' => 'Left',
        'right' => 'Right',
        'pageup' => 'Page Up',
        'pagedown' => 'Page Down',
        'escape' => 'Esc',
        'enter' => 'Enter',
        'tab' => 'Tab',
        'space' => 'Space',
        'backspace' => 'Backspace',
        'delete' => 'Delete',
        'home' => 'Home',
        'end' => 'End',
        'minus' => '-',
        'super' => 'Super',
        _ when normalized.length == 1 => normalized.toUpperCase(),
        _ => normalized,
      };
    }
    return switch (normalized) {
      'cmd' || 'control' || 'ctrl' => 'Ctrl',
      'alt' || 'option' => 'Alt',
      'shift' => 'Shift',
      'function' => 'Fn',
      'up' => 'Up',
      'down' => 'Down',
      'left' => 'Left',
      'right' => 'Right',
      'pageup' => 'Page Up',
      'pagedown' => 'Page Down',
      'escape' => 'Esc',
      'enter' => 'Enter',
      'tab' => 'Tab',
      'space' => 'Space',
      'backspace' => 'Backspace',
      'delete' => 'Delete',
      'home' => 'Home',
      'end' => 'End',
      'minus' => '-',
      'super' => 'Super',
      _ when normalized.length == 1 => normalized.toUpperCase(),
      _ => normalized,
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
              color: _hover ? AppTokens.overlayHoverStrong : Colors.transparent,
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
