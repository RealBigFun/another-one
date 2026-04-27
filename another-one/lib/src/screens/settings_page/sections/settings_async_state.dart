import 'package:flutter/material.dart';

import '../../../tokens.dart';

class SettingsSectionLoading extends StatelessWidget {
  const SettingsSectionLoading({super.key});

  @override
  Widget build(BuildContext context) {
    return const Padding(
      padding: EdgeInsets.symmetric(vertical: 24),
      child: Center(
        child: SizedBox(
          width: 18,
          height: 18,
          child: CircularProgressIndicator(strokeWidth: 2),
        ),
      ),
    );
  }
}

class SettingsSectionStatePanel extends StatelessWidget {
  const SettingsSectionStatePanel({
    super.key,
    required this.panelBg,
    required this.title,
    required this.message,
    this.error = false,
  });

  final Color panelBg;
  final String title;
  final String message;
  final bool error;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 18),
      decoration: BoxDecoration(
        color: panelBg,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTokens.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            title,
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w600,
              color: error ? AppTokens.errorText : AppTokens.textPrimary,
            ),
          ),
          const SizedBox(height: 4),
          Text(
            message,
            style: const TextStyle(
              fontSize: 12,
              height: 1.5,
              color: AppTokens.textSecondary,
            ),
          ),
        ],
      ),
    );
  }
}
