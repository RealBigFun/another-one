// Open In settings sub-page — port of
// `desktop/src/settings_page.rs::settings_open_in_content`.
//
// Lists every detected Open-In app (Cursor, Zed, VS Code, File
// Manager) with a checkbox per row that toggles the per-host
// enabled flag. Empty list shows a help panel telling the user
// to install one and restart.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../../../rust/api/local_session.dart'
    show OpenInAppSettingsRow;
import '../../../state/local_connection_provider.dart';
import '../../../tokens.dart';

final _openInSettingsProvider = FutureProvider.autoDispose((ref) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.readOpenInSettings();
  } on UnimplementedError {
    return null;
  }
});

class SettingsOpenInSection extends ConsumerWidget {
  const SettingsOpenInSection({super.key});

  static const Color _panelBg = Color(0xFF23252A);
  static const Color _rowBg = Color(0xFF1F2125);
  static const Color _activeBg = Color(0xFF2E5DC2);

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settingsAsync = ref.watch(_openInSettingsProvider);
    final view = settingsAsync.valueOrNull;
    final apps = view?.availableApps ?? const <OpenInAppSettingsRow>[];
    final enabledCount = apps.where((a) => a.enabled).length;
    final hasAny = apps.isNotEmpty;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text(
          'Open In',
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
            "Choose which detected apps appear in the project header's Open In menu.",
            style: TextStyle(
              fontSize: 12,
              height: 1.5,
              color: AppTokens.textSecondary,
            ),
          ),
        ),
        const SizedBox(height: 24),
        ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 860),
          child: _DetectedPanel(
            enabledCount: enabledCount,
            hasAny: hasAny,
            panelBg: _panelBg,
          ),
        ),
        const SizedBox(height: 16),
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
        else if (!hasAny)
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 860),
            child: Container(
              padding:
                  const EdgeInsets.symmetric(horizontal: 20, vertical: 18),
              decoration: BoxDecoration(
                color: _panelBg,
                borderRadius: BorderRadius.circular(12),
                border: Border.all(color: AppTokens.border),
              ),
              child: const Text(
                'Install Cursor, Zed, VS Code, or use your system file manager, then restart the app to refresh the menu.',
                style: TextStyle(
                  fontSize: 12,
                  height: 1.5,
                  color: AppTokens.textSecondary,
                ),
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
                  for (var i = 0; i < apps.length; i++)
                    _OpenInRow(
                      row: apps[i],
                      isFirst: i == 0,
                      activeBg: _activeBg,
                      onChanged: () =>
                          ref.invalidate(_openInSettingsProvider),
                    ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

class _DetectedPanel extends StatelessWidget {
  const _DetectedPanel({
    required this.enabledCount,
    required this.hasAny,
    required this.panelBg,
  });
  final int enabledCount;
  final bool hasAny;
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
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  'Detected apps',
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                    color: AppTokens.textPrimary,
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  hasAny
                      ? 'Only apps detected on this machine appear here. Changes save immediately.'
                      : 'No supported apps were detected on this machine.',
                  style: const TextStyle(
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

class _OpenInRow extends ConsumerStatefulWidget {
  const _OpenInRow({
    required this.row,
    required this.isFirst,
    required this.activeBg,
    required this.onChanged,
  });
  final OpenInAppSettingsRow row;
  final bool isFirst;
  final Color activeBg;
  final VoidCallback onChanged;

  @override
  ConsumerState<_OpenInRow> createState() => _OpenInRowState();
}

class _OpenInRowState extends ConsumerState<_OpenInRow> {
  bool _hover = false;
  bool _busy = false;

  Future<void> _toggle() async {
    if (_busy) return;
    setState(() => _busy = true);
    try {
      await ref.read(localConnectionProvider).setOpenInAppEnabled(
            appId: widget.row.id,
            enabled: !widget.row.enabled,
          );
      widget.onChanged();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Could not toggle: $e'),
          backgroundColor: AppTokens.errorBg,
        ),
      );
    }
    if (mounted) setState(() => _busy = false);
  }

  @override
  Widget build(BuildContext context) {
    final row = widget.row;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () => unawaited(_toggle()),
        child: Container(
          decoration: BoxDecoration(
            color: _hover
                ? AppTokens.overlayHover
                : Colors.transparent,
            border: widget.isFirst
                ? null
                : const Border(
                    top: BorderSide(color: AppTokens.border),
                  ),
          ),
          padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 14),
          child: Row(
            children: [
              SvgPicture.asset(
                row.iconPath,
                width: 16,
                height: 16,
                colorFilter: const ColorFilter.mode(
                  AppTokens.textPrimary,
                  BlendMode.srcIn,
                ),
              ),
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
                      row.description,
                      style: const TextStyle(
                        fontSize: 11,
                        color: AppTokens.textSecondary,
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 16),
              Text(
                row.enabled ? 'Enabled' : 'Disabled',
                style: TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.w500,
                  color: row.enabled
                      ? Colors.white
                      : AppTokens.textSecondary,
                ),
              ),
              const SizedBox(width: 10),
              Container(
                width: 18,
                height: 18,
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  color: row.enabled
                      ? widget.activeBg
                      : AppTokens.overlayHover,
                  borderRadius: BorderRadius.circular(5),
                  border: Border.all(
                    color: row.enabled
                        ? widget.activeBg.withValues(alpha: 0.85)
                        : AppTokens.border,
                  ),
                ),
                child: row.enabled
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
      ),
    );
  }
}
