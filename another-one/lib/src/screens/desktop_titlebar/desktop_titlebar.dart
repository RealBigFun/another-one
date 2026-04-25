// Top chrome titlebar for the desktop shell — visual-parity port of
// `desktop/src/titlebar.rs`.
//
// Layout (left → right):
//   - sidebar toggle (hides/shows the left sidebar — toggle wire-up
//     comes when the shell supports collapsible chrome)
//   - draggable empty region
//   - right cluster: pair-mobile (QR), resource indicator stub
//     (CPU% / mem MB), right-sidebar toggle
//
// Buttons that need verbs not yet on the bridge (Open In, Push,
// custom actions, build chip) are intentionally absent — they
// surface as placeholder buttons in subsequent commits when the
// underlying verbs land.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../tokens.dart';
import '../../widgets/app_icon.dart';
import '../pair_mobile/pair_mobile_modal.dart';

class DesktopTitlebar extends ConsumerWidget {
  const DesktopTitlebar({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Container(
      height: AppTokens.titlebarHeight,
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(
          bottom: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.space2),
      child: Row(
        children: [
          _TitlebarIconButton(
            icon: 'layout-split',
            tooltip: 'Toggle sidebar',
            onPressed: () {
              ScaffoldMessenger.of(context).showSnackBar(
                const SnackBar(
                  content: Text('Sidebar toggle is not yet wired'),
                  duration: Duration(seconds: 2),
                ),
              );
            },
          ),
          const SizedBox(width: AppTokens.space2),
          // Draggable region — Flutter doesn't expose a native
          // window-drag handle on Linux without `bitsdojo_window`,
          // which lands in Phase 4. Empty Spacer keeps the layout
          // stable until then.
          const Spacer(),
          const _PairMobileButton(),
          const SizedBox(width: AppTokens.space2),
          const _ResourceIndicator(),
          const SizedBox(width: AppTokens.space2),
          _TitlebarIconButton(
            icon: 'layout-split',
            tooltip: 'Toggle right sidebar',
            onPressed: () {
              ScaffoldMessenger.of(context).showSnackBar(
                const SnackBar(
                  content: Text('Right sidebar is not yet ported'),
                  duration: Duration(seconds: 2),
                ),
              );
            },
          ),
        ],
      ),
    );
  }
}

/// Resource-usage indicator: CPU% + memory MB. The GPUI version
/// reads from `core::platform::HeadlessPlatform::read_process_samples`
/// via a poll timer; wiring that through FRB is a follow-up. For
/// now this renders an em-dash placeholder so the layout footprint
/// matches the GPUI titlebar.
class _ResourceIndicator extends StatelessWidget {
  const _ResourceIndicator();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.space2),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          AppIcon(
            'big-plus',
            size: 12,
            color: AppTokens.textPlaceholder.withValues(alpha: 0.4),
          ),
          const SizedBox(width: AppTokens.space1),
          const Text(
            '— %  |  — MB',
            style: TextStyle(
              fontFamily: AppTokens.fontFamilyMono,
              fontSize: AppTokens.fontCaption,
              color: AppTokens.textPlaceholder,
            ),
          ),
        ],
      ),
    );
  }
}

class _PairMobileButton extends StatelessWidget {
  const _PairMobileButton();

  @override
  Widget build(BuildContext context) {
    return _TitlebarIconButton(
      icon: 'qr-code',
      tooltip: 'Pair a mobile device with the embedded daemon',
      onPressed: () => showPairMobileModal(context),
    );
  }
}

class _TitlebarIconButton extends StatefulWidget {
  const _TitlebarIconButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
  });

  final String icon;
  final String tooltip;
  final VoidCallback onPressed;

  @override
  State<_TitlebarIconButton> createState() => _TitlebarIconButtonState();
}

class _TitlebarIconButtonState extends State<_TitlebarIconButton> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: Container(
            width: 28,
            height: 28,
            decoration: BoxDecoration(
              color: _hovered
                  ? AppTokens.overlayHoverStrong
                  : AppTokens.overlayRest,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              border: Border.all(color: AppTokens.border),
            ),
            alignment: Alignment.center,
            child: AppIcon(
              widget.icon,
              size: 15,
              color: AppTokens.textPrimary,
            ),
          ),
        ),
      ),
    );
  }
}
