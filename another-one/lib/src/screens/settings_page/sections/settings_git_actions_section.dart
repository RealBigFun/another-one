// Git Actions settings sub-page (placeholder).
// Full port lands in another-one-e4f.4.

import 'package:flutter/material.dart';

import '../../../tokens.dart';

class SettingsGitActionsSection extends StatelessWidget {
  const SettingsGitActionsSection({super.key});

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: const [
        Text(
          'Git Actions',
          style: TextStyle(
            fontSize: 18,
            fontWeight: FontWeight.w600,
            color: AppTokens.textPrimary,
          ),
        ),
        SizedBox(height: 8),
        Text(
          'Commit message + PR description script editors — coming up next (e4f.4).',
          style: TextStyle(
            fontSize: 12,
            color: AppTokens.textMuted,
          ),
        ),
      ],
    );
  }
}
