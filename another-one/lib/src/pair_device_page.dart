// Onboarding screen shown when no daemon endpoint is saved. The
// entire focus is a single CTA: "Scan pairing QR". A subtle fallback
// ("Paste URL instead") opens a dialog — the expanded manual-entry
// form from the old list page is gone.
//
// Returns the scanned/pasted URL via the [onPaired] callback; the
// parent (`AppRoot`) is responsible for persisting the endpoint and
// kicking off a connection.

import 'package:flutter/material.dart';

import 'qr_scan_page.dart';
import 'tokens.dart';

class PairDevicePage extends StatelessWidget {
  const PairDevicePage({super.key, required this.onPaired});

  /// Called with a non-empty endpoint URL after either a QR scan or a
  /// manual paste. The caller should persist the URL and connect.
  final void Function(String url) onPaired;

  Future<void> _scanQr(BuildContext context) async {
    final result = await Navigator.of(context).push<String>(
      MaterialPageRoute(builder: (_) => const QrScanPage()),
    );
    final trimmed = result?.trim() ?? '';
    if (trimmed.isEmpty) return;
    onPaired(trimmed);
  }

  Future<void> _pasteDialog(BuildContext context) async {
    final controller = TextEditingController();
    final result = await showDialog<String>(
      context: context,
      builder: (ctx) {
        return AlertDialog(
          title: const Text('Paste endpoint URL'),
          content: TextField(
            controller: controller,
            autofocus: true,
            autocorrect: false,
            enableSuggestions: false,
            smartDashesType: SmartDashesType.disabled,
            smartQuotesType: SmartQuotesType.disabled,
            style: const TextStyle(
              fontFamily: AppTokens.fontFamilyMono,
              fontSize: AppTokens.fontBodyLg,
            ),
            decoration: const InputDecoration(
              hintText: 'iroh://… or ws://…',
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(ctx).pop(),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () =>
                  Navigator.of(ctx).pop(controller.text.trim()),
              child: const Text('Connect'),
            ),
          ],
        );
      },
    );
    controller.dispose();
    if (result == null || result.isEmpty) return;
    onPaired(result);
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.space10),
          child: Column(
            children: [
              const Spacer(),
              Icon(
                Icons.qr_code_scanner,
                size: 96,
                color: scheme.primary,
              ),
              const SizedBox(height: AppTokens.space7),
              const Text(
                'Pair with your daemon',
                style: TextStyle(
                  fontSize: AppTokens.fontHeadingLg,
                  fontWeight: FontWeight.w600,
                  color: AppTokens.textPrimary,
                ),
              ),
              const SizedBox(height: AppTokens.space3),
              const Text(
                'Open AnotherOne on your desktop and press the pairing\n'
                'button. Point your camera at the QR code it shows.',
                textAlign: TextAlign.center,
                style: TextStyle(
                  fontSize: AppTokens.fontBodyLg,
                  color: AppTokens.textSecondary,
                ),
              ),
              const SizedBox(height: AppTokens.space10),
              FilledButton.icon(
                onPressed: () => _scanQr(context),
                icon: const Icon(Icons.qr_code_scanner),
                label: const Text('Scan pairing QR'),
                style: FilledButton.styleFrom(
                  padding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.space10,
                    vertical: AppTokens.space5,
                  ),
                ),
              ),
              const Spacer(),
              TextButton(
                onPressed: () => _pasteDialog(context),
                child: const Text('Paste URL instead'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
