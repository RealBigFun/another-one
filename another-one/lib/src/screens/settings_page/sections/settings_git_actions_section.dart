// Git Actions settings sub-page — port of
// `desktop/src/settings_page.rs::settings_git_actions_content`.
//
// Two stacked panels — Commit message instructions + PR title/body
// instructions — each with:
//   * Header: title + "Currently using the default..." / "...custom
//     template..." subtitle + "Reset to Default" pill (filled
//     accent when a custom template is active, ghost otherwise).
//   * Body: 280-480px scrolling text editor with mono font.
//
// Loads the resolved current scripts via `read_git_action_scripts`
// (a single round-trip) and writes through `set_git_commit_script`
// / `set_git_pr_script` / their reset siblings. Writes are
// debounced 500ms after the last keystroke so each character
// doesn't round-trip into the registry.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../rust/api/local_session.dart' show GitActionScriptsView;
import '../../../state/local_connection_provider.dart';
import '../../../tokens.dart';

final _gitScriptsProvider =
    FutureProvider.autoDispose<GitActionScriptsView?>((ref) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.readGitActionScripts();
  } on UnimplementedError {
    return null;
  }
});

enum _GitScriptKind { commit, pr }

class SettingsGitActionsSection extends ConsumerWidget {
  const SettingsGitActionsSection({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final viewAsync = ref.watch(_gitScriptsProvider);
    final view = viewAsync.valueOrNull;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text(
          'Git Actions',
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
            'Customize the instructions sent to the LLM when the app generates commit messages and pull request title/body content. The app appends the relevant git context automatically. Changes save immediately, and you can reset back to the built-in instructions at any time.',
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
        else ...[
          _ScriptPanel(
            kind: _GitScriptKind.commit,
            title: 'Commit message instructions',
            placeholder: 'Paste commit generation instructions here.',
            initialScript: view.commitScript,
            usingDefault: view.commitUsingDefault,
            onChanged: () => ref.invalidate(_gitScriptsProvider),
          ),
          const SizedBox(height: 24),
          _ScriptPanel(
            kind: _GitScriptKind.pr,
            title: 'PR title/body instructions',
            placeholder: 'Paste PR title/body instructions here.',
            initialScript: view.prScript,
            usingDefault: view.prUsingDefault,
            onChanged: () => ref.invalidate(_gitScriptsProvider),
          ),
        ],
      ],
    );
  }
}

class _ScriptPanel extends ConsumerStatefulWidget {
  const _ScriptPanel({
    required this.kind,
    required this.title,
    required this.placeholder,
    required this.initialScript,
    required this.usingDefault,
    required this.onChanged,
  });

  final _GitScriptKind kind;
  final String title;
  final String placeholder;
  final String initialScript;
  final bool usingDefault;
  final VoidCallback onChanged;

  @override
  ConsumerState<_ScriptPanel> createState() => _ScriptPanelState();
}

class _ScriptPanelState extends ConsumerState<_ScriptPanel> {
  static const Color _panelBg = Color(0xFF23252A);
  static const Color _editorBg = Color(0xFF191B1F);
  static const Color _activeBg = Color(0xFF2E5DC2);

  late final TextEditingController _controller;
  Timer? _debounce;
  bool _focused = false;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.initialScript);
    _controller.addListener(_onChanged);
  }

  @override
  void didUpdateWidget(covariant _ScriptPanel old) {
    super.didUpdateWidget(old);
    if (widget.initialScript != _controller.text &&
        widget.initialScript != old.initialScript) {
      // External change (e.g. reset to default) — sync the editor
      // text without re-firing the debounce save.
      _controller.removeListener(_onChanged);
      _controller.text = widget.initialScript;
      _controller.addListener(_onChanged);
    }
  }

  @override
  void dispose() {
    _debounce?.cancel();
    _controller.removeListener(_onChanged);
    _controller.dispose();
    super.dispose();
  }

  void _onChanged() {
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 500), _save);
  }

  Future<void> _save() async {
    final text = _controller.text;
    final connection = ref.read(localConnectionProvider);
    setState(() => _busy = true);
    try {
      switch (widget.kind) {
        case _GitScriptKind.commit:
          await connection.setGitCommitScript(text);
        case _GitScriptKind.pr:
          await connection.setGitPrScript(text);
      }
      widget.onChanged();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Could not save script: $e'),
          backgroundColor: AppTokens.errorBg,
        ),
      );
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _resetToDefault() async {
    if (_busy) return;
    setState(() => _busy = true);
    final connection = ref.read(localConnectionProvider);
    try {
      switch (widget.kind) {
        case _GitScriptKind.commit:
          await connection.resetGitCommitScript();
        case _GitScriptKind.pr:
          await connection.resetGitPrScript();
      }
      widget.onChanged();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Could not reset script: $e'),
          backgroundColor: AppTokens.errorBg,
        ),
      );
    }
    if (mounted) setState(() => _busy = false);
  }

  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 960),
      child: Container(
        decoration: BoxDecoration(
          color: _panelBg,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: AppTokens.border),
        ),
        clipBehavior: Clip.antiAlias,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Container(
              padding:
                  const EdgeInsets.symmetric(horizontal: 18, vertical: 14),
              decoration: const BoxDecoration(
                border: Border(
                  bottom: BorderSide(color: AppTokens.border),
                ),
              ),
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          widget.title,
                          style: const TextStyle(
                            fontSize: 13,
                            fontWeight: FontWeight.w500,
                            color: AppTokens.textPrimary,
                          ),
                        ),
                        const SizedBox(height: 4),
                        Text(
                          widget.usingDefault
                              ? 'Currently using the default built-in template.'
                              : 'Currently using a custom template from settings.',
                          style: const TextStyle(
                            fontSize: 11,
                            color: AppTokens.textSecondary,
                          ),
                        ),
                      ],
                    ),
                  ),
                  _ResetButton(
                    usingDefault: widget.usingDefault,
                    busy: _busy,
                    onTap: _resetToDefault,
                    activeBg: _activeBg,
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.all(18),
              child: Container(
                constraints: const BoxConstraints(
                  minHeight: 280,
                  maxHeight: 480,
                ),
                decoration: BoxDecoration(
                  color: _editorBg,
                  borderRadius: BorderRadius.circular(10),
                  border: Border.all(
                    color: _focused
                        ? _activeBg.withValues(alpha: 0.85)
                        : AppTokens.border,
                  ),
                ),
                padding: const EdgeInsets.symmetric(
                  horizontal: 14,
                  vertical: 12,
                ),
                child: Focus(
                  onFocusChange: (f) => setState(() => _focused = f),
                  child: TextField(
                    controller: _controller,
                    maxLines: null,
                    expands: true,
                    keyboardType: TextInputType.multiline,
                    style: const TextStyle(
                      fontSize: 12,
                      height: 1.5,
                      fontFamily: AppTokens.fontFamilyMono,
                      color: AppTokens.textPrimary,
                    ),
                    cursorColor: AppTokens.textPrimary,
                    decoration: InputDecoration(
                      isDense: true,
                      contentPadding: EdgeInsets.zero,
                      border: InputBorder.none,
                      hintText: widget.placeholder,
                      hintStyle: const TextStyle(
                        fontSize: 12,
                        height: 1.5,
                        fontFamily: AppTokens.fontFamilyMono,
                        color: AppTokens.textPlaceholder,
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _ResetButton extends StatelessWidget {
  const _ResetButton({
    required this.usingDefault,
    required this.busy,
    required this.onTap,
    required this.activeBg,
  });

  final bool usingDefault;
  final bool busy;
  final Future<void> Function() onTap;
  final Color activeBg;

  @override
  Widget build(BuildContext context) {
    final bg = usingDefault ? AppTokens.overlayHover : activeBg;
    final textColor = usingDefault ? AppTokens.textPrimary : Colors.white;
    final borderColor = usingDefault
        ? AppTokens.border
        : activeBg.withValues(alpha: 0.85);
    return InkWell(
      borderRadius: BorderRadius.circular(8),
      onTap: busy ? null : () => unawaited(onTap()),
      child: Container(
        height: 30,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        alignment: Alignment.center,
        decoration: BoxDecoration(
          color: bg,
          borderRadius: BorderRadius.circular(8),
          border: Border.all(color: borderColor),
        ),
        child: Text(
          'Reset to Default',
          style: TextStyle(
            fontSize: 12,
            fontWeight: FontWeight.w500,
            color: textColor,
          ),
        ),
      ),
    );
  }
}
