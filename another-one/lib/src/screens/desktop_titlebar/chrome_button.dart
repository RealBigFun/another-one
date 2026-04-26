// Shared base for the titlebar's "chrome icon button" shape.
//
// Used by the left + right sidebar toggles. Mirrors GPUI's
// `desktop/src/titlebar.rs` toggle layout exactly:
//
//   * `p(px(1.))` — 1px padding around the icon (no extra
//     padding around the SVG itself).
//   * `rounded_md` (8px) — soft squircle corners on the hit
//     target.
//   * No idle bg, no border. Hover only: `bg(white@0.06)`.
//   * Cursor pointer + tooltip.
//   * SVG color = `theme::toggle_icon_color(window)` =
//     `hsla(226/360, 0.42, 0.72, 0.95)` on dark windows
//     (the periwinkle accent token).
//
// Distinct from the bordered-chip shape used by the GitHub /
// PR / Pair Mobile buttons, which carry their own border + bg.

part of 'desktop_titlebar.dart';

class _TitlebarChromeButton extends StatefulWidget {
  const _TitlebarChromeButton({
    required this.assetPath,
    required this.tooltip,
    required this.onPressed,
  });

  /// Full asset path (e.g. `assets/icons/icons__sidebar-toggle.svg`)
  /// — these chrome SVGs aren't named with the standard
  /// `icons__<name>.svg` convention `AppIcon` assumes for some
  /// callers, so the button takes the full path directly.
  final String assetPath;
  final String tooltip;
  final VoidCallback onPressed;

  @override
  State<_TitlebarChromeButton> createState() => _TitlebarChromeButtonState();
}

class _TitlebarChromeButtonState extends State<_TitlebarChromeButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: Container(
            padding: const EdgeInsets.all(1),
            decoration: BoxDecoration(
              color: _hover
                  // GPUI: `hover.bg(white@0.06)`.
                  ? const Color(0x0FFFFFFF)
                  : Colors.transparent,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            ),
            child: SvgPicture.asset(
              widget.assetPath,
              width: 16,
              height: 16,
              colorFilter: const ColorFilter.mode(
                AppTokens.toggleIconColor,
                BlendMode.srcIn,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
