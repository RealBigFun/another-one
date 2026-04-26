// Keybindings settings sub-page (placeholder).
// Full port lands in another-one-e4f.5.

import 'package:flutter/material.dart';

import '../../../tokens.dart';

class SettingsKeybindingsSection extends StatelessWidget {
  const SettingsKeybindingsSection({super.key});

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: const [
        Text(
          'Keybindings',
          style: TextStyle(
            fontSize: 18,
            fontWeight: FontWeight.w600,
            color: AppTokens.textPrimary,
          ),
        ),
        SizedBox(height: 8),
        Text(
          'Shortcut capture rows — coming up next (e4f.5).',
          style: TextStyle(
            fontSize: 12,
            color: AppTokens.textMuted,
          ),
        ),
      ],
    );
  }
}
