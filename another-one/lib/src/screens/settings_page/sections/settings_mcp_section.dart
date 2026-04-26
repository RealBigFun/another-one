// MCP settings sub-page (placeholder).
// Full port lands in another-one-e4f.6.

import 'package:flutter/material.dart';

import '../../../tokens.dart';

class SettingsMcpSection extends StatelessWidget {
  const SettingsMcpSection({super.key});

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: const [
        Text(
          'MCP',
          style: TextStyle(
            fontSize: 18,
            fontWeight: FontWeight.w600,
            color: AppTokens.textPrimary,
          ),
        ),
        SizedBox(height: 8),
        Text(
          'MCP server registry — coming up next (e4f.6).',
          style: TextStyle(
            fontSize: 12,
            color: AppTokens.textMuted,
          ),
        ),
      ],
    );
  }
}
