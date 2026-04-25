// Pair-mobile modal: shows the embedded daemon's QR + URL so a
// phone can scan it to pair.
//
// Port of `desktop/src/pair_mobile.rs`. Same surface:
//   - QR (320×320 on a white card, the PNG itself is black-on-
//     transparent so a white background helps scanning).
//   - Pairing URL in monospace below the QR.
//   - "Reset pairings" button with a two-click guard (first click
//     arms; second confirms).
//   - "Close" button. Click outside the modal body also closes.
//   - Empty state ("Mobile daemon is still starting…") when the
//     daemon hasn't published pairing material yet.
//
// Uses `pairingInfo()` from the FRB-generated bridge surface. The
// bridge returns `null` until the host binary registers the
// embedded daemon's pair handle (see
// `another-one-bridge/src/local_pair.rs`).

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../rust/api/pair.dart' as pair_api;
import '../../tokens.dart';

const double _modalWidth = 520;
const double _qrSize = 320;

/// Shows the pair-mobile modal as a barrier dialog. Returns when the
/// user closes (click outside, Close button, or `Esc`).
Future<void> showPairMobileModal(BuildContext context) {
  return showDialog<void>(
    context: context,
    barrierColor: AppTokens.scrimBg,
    builder: (_) => const PairMobileModal(),
  );
}

class PairMobileModal extends ConsumerStatefulWidget {
  const PairMobileModal({super.key});

  @override
  ConsumerState<PairMobileModal> createState() => _PairMobileModalState();
}

class _PairMobileModalState extends ConsumerState<PairMobileModal> {
  pair_api.PairingInfo? _info;
  bool _loading = true;
  bool _resetPending = false;
  Object? _error;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final info = await pair_api.pairingInfo();
      if (!mounted) return;
      setState(() {
        _info = info;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e;
        _loading = false;
      });
    }
  }

  Future<void> _onReset() async {
    if (!_resetPending) {
      setState(() => _resetPending = true);
      return;
    }
    setState(() => _resetPending = false);
    try {
      await pair_api.regenerateLocalPairing();
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = e);
      return;
    }
    await _refresh();
  }

  void _close() {
    Navigator.of(context).pop();
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
        constraints: const BoxConstraints(maxWidth: _modalWidth),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.space10),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              const Text(
                'Pair mobile device',
                style: TextStyle(
                  fontSize: AppTokens.fontHeading,
                  fontWeight: FontWeight.w600,
                  color: AppTokens.textPrimary,
                ),
              ),
              const SizedBox(height: AppTokens.space7),
              _buildBody(),
              const SizedBox(height: AppTokens.space7),
              _buildActions(),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildBody() {
    if (_loading) {
      return const SizedBox(
        width: _qrSize,
        height: _qrSize,
        child: Center(child: CircularProgressIndicator()),
      );
    }
    if (_error != null) {
      return _emptyState(
        title: 'Failed to read pairing info',
        body: '$_error',
      );
    }
    final info = _info;
    if (info == null) {
      return _emptyState(
        title: 'Mobile daemon is still starting…',
        body:
            'The embedded iroh endpoint boots on app start. Close and reopen this dialog in a moment.',
      );
    }
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          width: _qrSize,
          height: _qrSize,
          decoration: BoxDecoration(
            color: Colors.white,
            borderRadius: BorderRadius.circular(AppTokens.radiusMd),
          ),
          padding: const EdgeInsets.all(AppTokens.space3),
          child: Image.memory(
            info.qrPngBytes,
            fit: BoxFit.contain,
            filterQuality: FilterQuality.none,
            gaplessPlayback: true,
          ),
        ),
        const SizedBox(height: AppTokens.space7),
        SelectableText(
          info.url,
          style: const TextStyle(
            fontFamily: AppTokens.fontFamilyMono,
            fontSize: AppTokens.fontSmall,
            color: AppTokens.textMuted,
          ),
          textAlign: TextAlign.center,
        ),
      ],
    );
  }

  Widget _emptyState({required String title, required String body}) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.space5),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            title,
            style: const TextStyle(
              fontSize: AppTokens.fontBodyLg,
              color: AppTokens.textPrimary,
            ),
          ),
          const SizedBox(height: AppTokens.space3),
          Text(
            body,
            style: const TextStyle(
              fontSize: AppTokens.fontBody,
              color: AppTokens.textMuted,
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildActions() {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        _ResetButton(
          pending: _resetPending,
          onPressed: _info == null ? null : _onReset,
        ),
        const SizedBox(width: AppTokens.space3),
        TextButton(
          onPressed: _close,
          style: TextButton.styleFrom(
            foregroundColor: AppTokens.textSecondary,
            padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.space7,
              vertical: AppTokens.space2,
            ),
          ),
          child: const Text('Close'),
        ),
      ],
    );
  }
}

class _ResetButton extends StatelessWidget {
  const _ResetButton({required this.pending, required this.onPressed});

  final bool pending;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final bg = pending ? const Color(0xFF8A2A2A) : const Color(0xFF4A2A2A);
    final hoverBg =
        pending ? const Color(0xFFA03A3A) : const Color(0xFF5A3636);
    return TextButton(
      onPressed: onPressed,
      style: ButtonStyle(
        foregroundColor: const WidgetStatePropertyAll(AppTokens.textPrimary),
        padding: const WidgetStatePropertyAll(
          EdgeInsets.symmetric(
            horizontal: AppTokens.space6,
            vertical: AppTokens.space2,
          ),
        ),
        backgroundColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.hovered)) return hoverBg;
          return bg;
        }),
        shape: WidgetStatePropertyAll(
          RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(AppTokens.radiusMd),
          ),
        ),
      ),
      child: Text(pending ? 'Confirm reset?' : 'Reset pairings'),
    );
  }
}
