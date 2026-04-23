// Settings page, pushed from the gear icon on `ProjectsDrawerPage`.
// Small, focused scaffold that exposes:
//
//   • the currently-paired endpoint URL (read-only),
//   • an "Unlink daemon" button (wipes prefs + pops to Pair page),
//   • a "Rescan QR" button (replaces the saved endpoint in-place).

import 'package:flutter/material.dart';

import 'qr_scan_page.dart';
import 'tokens.dart';

class SettingsPage extends StatelessWidget {
  const SettingsPage({
    super.key,
    required this.endpoint,
    required this.onUnlink,
    required this.onReplaceEndpoint,
  });

  /// The currently-saved endpoint URL.
  final String endpoint;

  /// Called after the user confirms the unlink dialog. Caller should
  /// clear SharedPreferences, tear down the transport, and pop this
  /// page back to `PairDevicePage`.
  final Future<void> Function() onUnlink;

  /// Called with a new endpoint URL after a successful rescan.
  final Future<void> Function(String newEndpoint) onReplaceEndpoint;

  Future<void> _confirmUnlink(BuildContext context) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Unlink daemon?'),
        content: const Text(
          'This will remove the saved endpoint and disconnect from\n'
          'the daemon. You can pair again from the onboarding screen.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(ctx).colorScheme.error,
              foregroundColor: Theme.of(ctx).colorScheme.onError,
            ),
            onPressed: () => Navigator.of(ctx).pop(true),
            child: const Text('Unlink'),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    await onUnlink();
  }

  Future<void> _rescan(BuildContext context) async {
    final result = await Navigator.of(context).push<String>(
      MaterialPageRoute(builder: (_) => const QrScanPage()),
    );
    final trimmed = result?.trim() ?? '';
    if (trimmed.isEmpty) return;
    await onReplaceEndpoint(trimmed);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(AppTokens.space7),
          children: [
            const Text(
              'Paired endpoint',
              style: TextStyle(
                color: AppTokens.textSecondary,
                fontSize: AppTokens.fontBody,
                fontWeight: FontWeight.w600,
              ),
            ),
            const SizedBox(height: AppTokens.space2),
            Container(
              padding: const EdgeInsets.all(AppTokens.space5),
              decoration: BoxDecoration(
                color: AppTokens.sunkenBg,
                borderRadius: BorderRadius.circular(AppTokens.radiusMd),
                border: Border.all(color: AppTokens.border),
              ),
              child: SelectableText(
                endpoint.isEmpty ? '(none)' : endpoint,
                style: const TextStyle(
                  fontFamily: AppTokens.fontFamilyMono,
                  fontSize: AppTokens.fontBody,
                  color: AppTokens.textPrimary,
                ),
              ),
            ),
            const SizedBox(height: AppTokens.space9),
            OutlinedButton.icon(
              onPressed: () => _rescan(context),
              icon: const Icon(Icons.qr_code_scanner),
              label: const Text('Rescan QR'),
            ),
            const SizedBox(height: AppTokens.space5),
            FilledButton.icon(
              onPressed: () => _confirmUnlink(context),
              icon: const Icon(Icons.link_off),
              label: const Text('Unlink daemon'),
              style: FilledButton.styleFrom(
                backgroundColor: Theme.of(context).colorScheme.error,
                foregroundColor: Theme.of(context).colorScheme.onError,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
