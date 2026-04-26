// Open In settings sub-page (placeholder).
// Full port lands in another-one-e4f.3.

import 'package:flutter/material.dart';

import '../../../tokens.dart';

class SettingsOpenInSection extends StatelessWidget {
  const SettingsOpenInSection({super.key});

  @override
  Widget build(BuildContext context) {
    return const _Placeholder(
      title: 'Open In',
      hint: 'Per-app toggles port — coming up next (e4f.3).',
    );
  }
}

class _Placeholder extends StatelessWidget {
  const _Placeholder({required this.title, required this.hint});
  final String title;
  final String hint;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          title,
          style: const TextStyle(
            fontSize: 18,
            fontWeight: FontWeight.w600,
            color: AppTokens.textPrimary,
          ),
        ),
        const SizedBox(height: 8),
        Text(
          hint,
          style: const TextStyle(
            fontSize: 12,
            color: AppTokens.textMuted,
          ),
        ),
      ],
    );
  }
}
