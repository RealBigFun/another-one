// Configuration panel on the project overview page —
// pixel-precise port of `desktop/src/project_page.rs::project_page_configuration_section`
// + branch_config_row + branch_config_option.
//
// Two rows: Default Branch + Default Target Branch. Each row has a
// trigger pill that toggles a dropdown listing 'Automatic' followed
// by every available branch. Picking an option fires a
// setBranchSetting bridge call and refreshes the section.
//
// Panel collapse, dropdown-open state, and helper-text formatting
// all match GPUI's logic in
// project_page_branch_config_row's helper_text match arm.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../rust/api/local_session.dart' show ResolvedProjectBranchSettingsDto;
import '../../state/branch_settings_provider.dart';
import '../../state/local_connection_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';

class ConfigurationSection extends ConsumerWidget {
  const ConfigurationSection({super.key, required this.projectId});

  final String projectId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settingsAsync = ref.watch(branchSettingsProvider(projectId));
    final expanded = ref.watch(configurationPanelExpandedProvider);
    return Padding(
      padding: const EdgeInsets.only(top: 28, bottom: 24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _ConfigurationHeader(expanded: expanded),
          if (expanded)
            settingsAsync.when(
              data: (s) => s == null
                  ? const _ConfigurationEmpty()
                  : _ConfigurationRows(projectId: projectId, settings: s),
              loading: () => const Padding(
                padding: EdgeInsets.only(top: 12),
                child: Text(
                  'Loading branch settings...',
                  style: TextStyle(
                    fontSize: 11,
                    color: AppTokens.textMuted,
                  ),
                ),
              ),
              error: (e, _) => Padding(
                padding: const EdgeInsets.only(top: 12),
                child: Text(
                  'Could not load branch settings: $e',
                  style: const TextStyle(
                    fontSize: 11,
                    color: AppTokens.textMuted,
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _ConfigurationHeader extends ConsumerStatefulWidget {
  const _ConfigurationHeader({required this.expanded});

  final bool expanded;

  @override
  ConsumerState<_ConfigurationHeader> createState() =>
      _ConfigurationHeaderState();
}

class _ConfigurationHeaderState extends ConsumerState<_ConfigurationHeader> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () => ref
            .read(configurationPanelExpandedProvider.notifier)
            .update((s) => !s),
        child: Container(
          padding: const EdgeInsets.symmetric(vertical: 4),
          color: _hover ? const Color(0x08FFFFFF) : Colors.transparent,
          child: Row(
            children: [
              AppIcon(
                widget.expanded ? 'chevron-down' : 'chevron-right',
                size: 15,
                color: AppTokens.textSecondary,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: const [
                    Text(
                      'Configuration',
                      style: TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        color: AppTokens.textPrimary,
                      ),
                    ),
                    SizedBox(height: 2),
                    Text(
                      'These defaults apply to the whole project group.',
                      style: TextStyle(
                        fontSize: 11,
                        color: AppTokens.textMuted,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ConfigurationEmpty extends StatelessWidget {
  const _ConfigurationEmpty();

  @override
  Widget build(BuildContext context) {
    return const Padding(
      padding: EdgeInsets.only(top: 12),
      child: Text(
        'No branch settings available — this project may not be tracked yet.',
        style: TextStyle(fontSize: 11, color: AppTokens.textMuted),
      ),
    );
  }
}

class _ConfigurationRows extends StatelessWidget {
  const _ConfigurationRows({required this.projectId, required this.settings});

  final String projectId;
  final ResolvedProjectBranchSettingsDto settings;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _BranchConfigRow(
            projectId: projectId,
            field: BranchSettingField.defaultBranch,
            title: 'Default Branch',
            description:
                'Preferred base branch for new tasks and worktrees.',
            settings: settings,
            configuredValue: settings.configuredDefaultBranch,
            effectiveValue: settings.effectiveDefaultBranch,
          ),
          const SizedBox(height: 12),
          _BranchConfigRow(
            projectId: projectId,
            field: BranchSettingField.defaultTargetBranch,
            title: 'Default Target Branch',
            description:
                'Used for PR creation and the compare view in the right sidebar.',
            settings: settings,
            configuredValue: settings.configuredDefaultTargetBranch,
            effectiveValue: settings.effectiveDefaultTargetBranch,
          ),
        ],
      ),
    );
  }
}

class _BranchConfigRow extends ConsumerWidget {
  const _BranchConfigRow({
    required this.projectId,
    required this.field,
    required this.title,
    required this.description,
    required this.settings,
    required this.configuredValue,
    required this.effectiveValue,
  });

  final String projectId;
  final BranchSettingField field;
  final String title;
  final String description;
  final ResolvedProjectBranchSettingsDto settings;
  final String? configuredValue;
  final String? effectiveValue;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final dropdownOpen = ref.watch(branchSettingDropdownProvider) == field;
    final selectedLabel = configuredValue ?? 'Automatic';
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: const Color(0x08FFFFFF),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: AppTokens.divider),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      title,
                      style: const TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w600,
                        color: AppTokens.textPrimary,
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      description,
                      style: const TextStyle(
                        fontSize: 11,
                        color: AppTokens.textMuted,
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 12),
              _BranchTriggerPill(
                label: selectedLabel,
                onTap: () => ref
                    .read(branchSettingDropdownProvider.notifier)
                    .toggle(field),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Text(
            _helperText(),
            style: const TextStyle(
              fontSize: 11,
              color: AppTokens.textMuted,
            ),
          ),
          if (dropdownOpen) ...[
            const SizedBox(height: 8),
            _BranchDropdown(
              projectId: projectId,
              field: field,
              configuredValue: configuredValue,
              availableBranches: settings.availableBranches,
            ),
          ],
        ],
      ),
    );
  }

  /// Mirrors GPUI's `helper_text` match arm in
  /// `project_page_branch_config_row`.
  String _helperText() {
    if (field == BranchSettingField.defaultBranch) {
      if (configuredValue != null) {
        if (effectiveValue != null) {
          return 'New worktree tasks will start from $effectiveValue.';
        }
      } else if (effectiveValue != null) {
        return 'Currently resolving automatically to $effectiveValue.';
      }
      if (effectiveValue == null) {
        return 'No branches are currently available.';
      }
    } else {
      if (configuredValue != null) {
        return 'PRs and compare mode currently target $configuredValue.';
      }
      return 'Unset keeps GitHub PR targeting on its default base and hides compare mode.';
    }
    return '';
  }
}

class _BranchTriggerPill extends StatefulWidget {
  const _BranchTriggerPill({required this.label, required this.onTap});

  final String label;
  final VoidCallback onTap;

  @override
  State<_BranchTriggerPill> createState() => _BranchTriggerPillState();
}

class _BranchTriggerPillState extends State<_BranchTriggerPill> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          height: 36,
          constraints: const BoxConstraints(minWidth: 220),
          padding: const EdgeInsets.symmetric(horizontal: 12),
          decoration: BoxDecoration(
            color:
                _hover ? AppTokens.overlayHover : const Color(0xFF1E2024),
            borderRadius: BorderRadius.circular(8),
            border: Border.all(color: AppTokens.border),
          ),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  widget.label,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: 12,
                    color: AppTokens.textPrimary,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              const AppIcon(
                'chevron-down',
                size: 11,
                color: AppTokens.textSecondary,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _BranchDropdown extends ConsumerWidget {
  const _BranchDropdown({
    required this.projectId,
    required this.field,
    required this.configuredValue,
    required this.availableBranches,
  });

  final String projectId;
  final BranchSettingField field;
  final String? configuredValue;
  final List<String> availableBranches;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Container(
      decoration: BoxDecoration(
        color: const Color(0xFF1E2024),
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: AppTokens.border),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _BranchOptionRow(
            label: 'Automatic',
            selected: configuredValue == null,
            onTap: () => _select(ref, null),
          ),
          for (final branch in availableBranches)
            _BranchOptionRow(
              label: branch,
              selected: configuredValue == branch,
              onTap: () => _select(ref, branch),
            ),
        ],
      ),
    );
  }

  Future<void> _select(WidgetRef ref, String? branchName) async {
    final connection = ref.read(localConnectionProvider);
    final fieldString = switch (field) {
      BranchSettingField.defaultBranch => 'default-branch',
      BranchSettingField.defaultTargetBranch => 'default-target-branch',
    };
    try {
      await connection.setBranchSetting(
        projectId: projectId,
        field: fieldString,
        branchName: branchName,
      );
    } catch (_) {
      // Best-effort — close dropdown and refetch so the rendered
      // state reflects whatever did persist (or didn't).
    }
    ref.read(branchSettingDropdownProvider.notifier).close();
    ref.invalidate(branchSettingsProvider(projectId));
  }
}

class _BranchOptionRow extends StatefulWidget {
  const _BranchOptionRow({
    required this.label,
    required this.selected,
    required this.onTap,
  });

  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  State<_BranchOptionRow> createState() => _BranchOptionRowState();
}

class _BranchOptionRowState extends State<_BranchOptionRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final restBg = widget.selected
        ? AppTokens.overlayHoverStrong // white @ 0.08
        : Colors.transparent;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          height: 36,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          color: _hover ? AppTokens.overlayHover : restBg,
          child: Row(
            children: [
              Expanded(
                child: Text(
                  widget.label,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: 12,
                    color: AppTokens.textPrimary,
                  ),
                ),
              ),
              if (widget.selected)
                const Text(
                  'Selected',
                  style: TextStyle(
                    fontSize: 10,
                    fontWeight: FontWeight.w600,
                    color: AppTokens.textSecondary,
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
