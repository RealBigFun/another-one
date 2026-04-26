// Agents settings sub-page — port of
// `desktop/src/settings_page.rs::settings_agents_content`.
//
// Per-agent rows with branded icon, label, description, launch-args
// chip list + add input, Make-default radio, Enable/Disable pill.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../../../rust/api/local_session.dart' show AgentSummaryDto;
import '../../../state/new_task_data_provider.dart';
import '../../../tokens.dart';

class SettingsAgentsSection extends ConsumerWidget {
  const SettingsAgentsSection({super.key});

  static const Color _panelBg = Color(0xFF23252A);
  static const Color _rowBg = Color(0xFF1F2125);
  static const Color _activeBg = Color(0xFF2E5DC2);

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final agentsAsync = ref.watch(enabledAgentsProvider);
    final view = agentsAsync.valueOrNull;
    final enabledCount = view?.agents.length ?? 0;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text(
          'Agents',
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
            'Manage per-agent argv tokens and availability. Disabled agents stay here so they can be re-enabled, but they are hidden from New Task and Add Agent pickers. Changes save immediately.',
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
          child: Container(
            decoration: BoxDecoration(
              color: _panelBg,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: AppTokens.border),
            ),
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
            child: Row(
              children: [
                const Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Availability',
                        style: TextStyle(
                          fontSize: 12,
                          fontWeight: FontWeight.w600,
                          color: AppTokens.textPrimary,
                        ),
                      ),
                      SizedBox(height: 4),
                      Text(
                        'Choose which enabled agent is used first for new tasks and new agent tabs. Disabled agents can still be re-enabled and edited here.',
                        style: TextStyle(
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
                  for (var i = 0; i < view.agents.length; i++)
                    _AgentRow(
                      agent: view.agents[i],
                      isFirst: i == 0,
                      isDefault: view.agents[i].id == view.defaultAgentId,
                      activeBg: _activeBg,
                    ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

class _AgentRow extends StatelessWidget {
  const _AgentRow({
    required this.agent,
    required this.isFirst,
    required this.isDefault,
    required this.activeBg,
  });

  final AgentSummaryDto agent;
  final bool isFirst;
  final bool isDefault;
  final Color activeBg;

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        border: isFirst
            ? null
            : const Border(top: BorderSide(color: AppTokens.border)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 16),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _AgentGlyph(iconPath: agent.iconPath, size: 18),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  agent.label,
                  style: const TextStyle(
                    fontSize: 13,
                    fontWeight: FontWeight.w500,
                    color: AppTokens.textPrimary,
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  'Extra argv tokens passed to ${agent.label} on every launch and resume.',
                  style: const TextStyle(
                    fontSize: 11,
                    color: AppTokens.textSecondary,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(width: 16),
          _DefaultPill(isDefault: isDefault, activeBg: activeBg),
        ],
      ),
    );
  }
}

class _DefaultPill extends StatelessWidget {
  const _DefaultPill({required this.isDefault, required this.activeBg});
  final bool isDefault;
  final Color activeBg;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 30,
      padding: const EdgeInsets.symmetric(horizontal: 10),
      decoration: BoxDecoration(
        color: isDefault ? activeBg : AppTokens.overlayHover,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(
          color: isDefault
              ? activeBg.withValues(alpha: 0.85)
              : AppTokens.border,
        ),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            isDefault ? 'Default' : 'Make default',
            style: TextStyle(
              fontSize: 11,
              fontWeight: FontWeight.w500,
              color:
                  isDefault ? Colors.white : AppTokens.textSecondary,
            ),
          ),
          const SizedBox(width: 8),
          Container(
            width: 16,
            height: 16,
            decoration: BoxDecoration(
              color: isDefault
                  ? Colors.white.withValues(alpha: 0.16)
                  : Colors.transparent,
              border: Border.all(
                color: isDefault
                    ? Colors.white.withValues(alpha: 0.85)
                    : AppTokens.border,
              ),
              borderRadius: BorderRadius.circular(999),
            ),
            child: isDefault
                ? Center(
                    child: Container(
                      width: 7,
                      height: 7,
                      decoration: const BoxDecoration(
                        color: Colors.white,
                        shape: BoxShape.circle,
                      ),
                    ),
                  )
                : null,
          ),
        ],
      ),
    );
  }
}

class _AgentGlyph extends StatelessWidget {
  const _AgentGlyph({required this.iconPath, required this.size});
  final String iconPath;
  final double size;

  @override
  Widget build(BuildContext context) {
    if (iconPath.endsWith('.svg')) {
      return SvgPicture.asset(
        iconPath,
        width: size,
        height: size,
        colorFilter: const ColorFilter.mode(
          AppTokens.textPrimary,
          BlendMode.srcIn,
        ),
      );
    }
    return Image.asset(iconPath, width: size, height: size);
  }
}
