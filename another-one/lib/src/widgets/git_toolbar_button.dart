// Pixel-precise port of `desktop/src/right_sidebar.rs::git_toolbar_button`.
// Tab pills + the future "Load more" / "Open PR" bar buttons all use
// this exact shape:
//
//   h(30) px(7) rounded(7), 1px border (transparent unless active)
//   active bg rgb(0x262a30), active border white @ 0.14
//   hover bg white @ 0.06 (only when interactive)
//   visually_enabled = enabled || active
//     text   white @ 0.94 / 0.48 disabled
//     icon   white @ 0.82 / 0.42 disabled
//   leading_icon 12px svg, label 11px semibold
//   trailing_icon 11px svg
//   opacity 0.55 when !visually_enabled
//
// Active and "enabled-but-not-active" share text/icon colors —
// the active state shows up purely through bg + border. Same as
// GPUI; do not collapse the two states into different palettes.

import 'package:flutter/material.dart';

import 'app_icon.dart';

class GitToolbarButton extends StatefulWidget {
  const GitToolbarButton({
    super.key,
    required this.label,
    this.leadingIcon,
    this.trailingIcon,
    this.tooltip,
    this.active = false,
    this.enabled = true,
    this.onPressed,
  });

  /// Label text shown next to the icon. 11px semibold.
  final String label;

  /// Leading SVG glyph name (without `icons__` prefix or `.svg`),
  /// rendered at 12px. Null = label-only.
  final String? leadingIcon;

  /// Trailing SVG glyph (chevron, etc.) rendered at 11px.
  final String? trailingIcon;

  final String? tooltip;

  /// Active state — the button reflects current selection. GPUI
  /// gates `bg(rgb(0x262a30))` + `white@0.14 border` on this flag.
  final bool active;

  /// Disabled state. When false (and not active), the button greys
  /// to opacity 0.55, drops hover bg, and ignores taps.
  final bool enabled;

  /// Click handler. Required when [enabled] is true; ignored when
  /// [enabled] is false.
  final VoidCallback? onPressed;

  @override
  State<GitToolbarButton> createState() => _GitToolbarButtonState();
}

class _GitToolbarButtonState extends State<GitToolbarButton> {
  bool _hover = false;

  static const Color _enabledText = Color(0xF0FFFFFF); // white @ 0.94
  static const Color _disabledText = Color(0x7AFFFFFF); // white @ 0.48
  static const Color _enabledIcon = Color(0xD1FFFFFF); // white @ 0.82
  static const Color _disabledIcon = Color(0x6BFFFFFF); // white @ 0.42
  static const Color _activeBg = Color(0xFF262A30);
  static const Color _activeBorder = Color(0x24FFFFFF); // white @ 0.14
  static const Color _hoverBg = Color(0x0FFFFFFF); // white @ 0.06

  @override
  Widget build(BuildContext context) {
    final visuallyEnabled = widget.enabled || widget.active;
    final textColor = visuallyEnabled ? _enabledText : _disabledText;
    final iconColor = visuallyEnabled ? _enabledIcon : _disabledIcon;
    final canHover = widget.enabled && !widget.active;
    final bg = widget.active
        ? _activeBg
        : (canHover && _hover ? _hoverBg : Colors.transparent);
    final borderColor = widget.active ? _activeBorder : Colors.transparent;

    final container = Opacity(
      opacity: visuallyEnabled ? 1.0 : 0.55,
      child: Container(
        height: 30,
        padding: const EdgeInsets.symmetric(horizontal: 7),
        alignment: Alignment.center,
        decoration: BoxDecoration(
          color: bg,
          borderRadius: BorderRadius.circular(7),
          border: Border.all(color: borderColor),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (widget.leadingIcon != null) ...[
              AppIcon(widget.leadingIcon!, size: 12, color: iconColor),
              const SizedBox(width: 5),
            ],
            Text(
              widget.label,
              style: TextStyle(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: textColor,
              ),
            ),
            if (widget.trailingIcon != null) ...[
              const SizedBox(width: 5),
              AppIcon(widget.trailingIcon!, size: 11, color: iconColor),
            ],
          ],
        ),
      ),
    );

    if (!widget.enabled) return container;
    final wrapped = MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: canHover ? (_) => setState(() => _hover = true) : null,
      onExit: canHover ? (_) => setState(() => _hover = false) : null,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onPressed,
        child: container,
      ),
    );
    return widget.tooltip == null
        ? wrapped
        : Tooltip(message: widget.tooltip!, child: wrapped);
  }
}
