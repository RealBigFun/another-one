// Full-page Settings — visual-parity port of
// `desktop/src/settings_page.rs::render_settings_page`.
//
// Replaces the main pane (sidebar + center + right sidebar layout)
// when `settingsOpenProvider` is true. 180px-wide left rail with a
// "Back to app" chevron + 5 nav items; right side is a scrollable
// content pane that swaps based on `settingsSectionProvider`.
//
// Each sub-page is its own widget under
// `screens/settings_page/sections/`. The shell here wires up the
// nav + back button only.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../state/settings_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';
import 'sections/settings_agents_section.dart';
import 'sections/settings_git_actions_section.dart';
import 'sections/settings_keybindings_section.dart';
import 'sections/settings_mcp_section.dart';
import 'sections/settings_open_in_section.dart';

class SettingsPage extends ConsumerWidget {
  const SettingsPage({super.key});

  static const double _sidebarW = 180;
  // hsla(215/360, 0.60, 0.45, 1) — see desktop/src/settings_page.rs.
  static const Color _activeBg = Color(0xFF2E67B8);

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final section = ref.watch(settingsSectionProvider);
    return Container(
      color: AppTokens.terminalBg,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _Sidebar(
            width: _sidebarW,
            activeBg: _activeBg,
            current: section,
            onPick: (s) =>
                ref.read(settingsSectionProvider.notifier).state = s,
            onBack: () =>
                ref.read(settingsOpenProvider.notifier).state = false,
          ),
          Expanded(
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(32),
              child: switch (section) {
                SettingsSection.agents => const SettingsAgentsSection(),
                SettingsSection.openIn => const SettingsOpenInSection(),
                SettingsSection.gitActions =>
                  const SettingsGitActionsSection(),
                SettingsSection.keybindings =>
                  const SettingsKeybindingsSection(),
                SettingsSection.mcp => const SettingsMcpSection(),
              },
            ),
          ),
        ],
      ),
    );
  }
}

class _Sidebar extends StatelessWidget {
  const _Sidebar({
    required this.width,
    required this.activeBg,
    required this.current,
    required this.onPick,
    required this.onBack,
  });

  final double width;
  final Color activeBg;
  final SettingsSection current;
  final ValueChanged<SettingsSection> onPick;
  final VoidCallback onBack;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: width,
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(
          right: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 16),
            child: _BackButton(onTap: onBack),
          ),
          for (final section in SettingsSection.values)
            _NavItem(
              section: section,
              active: section == current,
              activeBg: activeBg,
              onTap: () => onPick(section),
            ),
        ],
      ),
    );
  }
}

class _BackButton extends StatefulWidget {
  const _BackButton({required this.onTap});
  final VoidCallback onTap;

  @override
  State<_BackButton> createState() => _BackButtonState();
}

class _BackButtonState extends State<_BackButton> {
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
          padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 4),
          decoration: BoxDecoration(
            color:
                _hover ? AppTokens.overlayHover : Colors.transparent,
            borderRadius: BorderRadius.circular(5),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: const [
              AppIcon(
                'chevron-left',
                size: 14,
                color: AppTokens.chevron,
              ),
              SizedBox(width: 4),
              Text(
                'Back to app',
                style: TextStyle(
                  fontSize: 12,
                  color: AppTokens.chevron,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _NavItem extends StatefulWidget {
  const _NavItem({
    required this.section,
    required this.active,
    required this.activeBg,
    required this.onTap,
  });

  final SettingsSection section;
  final bool active;
  final Color activeBg;
  final VoidCallback onTap;

  @override
  State<_NavItem> createState() => _NavItemState();
}

class _NavItemState extends State<_NavItem> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final bg = widget.active
        ? widget.activeBg
        : (_hover ? AppTokens.overlayHover : Colors.transparent);
    final textColor = widget.active ? Colors.white : AppTokens.textSecondary;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          height: 30,
          margin: const EdgeInsets.symmetric(horizontal: 8),
          padding: const EdgeInsets.symmetric(horizontal: 10),
          alignment: Alignment.centerLeft,
          decoration: BoxDecoration(
            color: bg,
            borderRadius: BorderRadius.circular(5),
          ),
          child: Text(
            widget.section.label,
            style: TextStyle(
              fontSize: 13,
              color: textColor,
            ),
          ),
        ),
      ),
    );
  }
}
