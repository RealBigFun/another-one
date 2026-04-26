// Single hover-icon-button widget consolidating five near-identical
// private variants that had spread across titlebar / sidebar / footer
// surfaces (`_TitlebarIconButton`, `_RowIconButton`, `_FooterIconButton`,
// `_ProjectGithubButton`, `_ActiveProjectGithubButton`).
//
// The differences across those variants reduced cleanly to four
// tokens: button size, presence of a border, rest/hover background
// intensity, and icon colour. Plus the github-slot pattern, where
// the row reserves its slot but renders the icon transparent until
// a URL resolves — exposed here as `iconOpacity` so the call site
// stays one widget.
//
// Hover state is local-only and frame-by-frame, so this stays a
// `StatefulWidget` rather than a `ConsumerWidget`. Riverpod-aware
// callers wrap this with their own watch (see the github buttons).

import 'package:flutter/material.dart';

import '../tokens.dart';
import 'app_icon.dart';

class HoverIconButton extends StatefulWidget {
  const HoverIconButton({
    super.key,
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    this.size = 24,
    this.iconSize,
    this.iconColor = AppTokens.textMuted,
    this.restBg = Colors.transparent,
    this.hoverBg = AppTokens.overlayHover,
    this.showBorder = false,
    this.borderColor = AppTokens.border,
    this.iconOpacity = 1.0,
  });

  /// SVG glyph name passed through [`AppIcon`]. The asset must live
  /// in `assets/icons/` as `icons__<name>.svg`.
  final String icon;
  final String tooltip;

  /// `null` disables the click handler and skips the hover bg flip,
  /// matching the GPUI desktop's "invisible disabled" treatment for
  /// gated buttons (e.g. the github slot when the URL hasn't
  /// resolved).
  final VoidCallback? onPressed;

  /// Outer square edge length. Common values are 24 (in-row) and
  /// 28 (titlebar / footer chrome).
  final double size;

  /// Inner SVG size. Defaults to a sensible fraction of [size]
  /// matching the rest of the GPUI desktop's tokens (~13 for size
  /// 24, ~15 for 28).
  final double? iconSize;
  final Color iconColor;

  /// Background when the cursor isn't over the button. Most variants
  /// are transparent at rest; titlebar buttons use [`AppTokens.overlayRest`]
  /// to match the bordered chrome shape.
  final Color restBg;

  /// Background when hovered. Use [`AppTokens.overlayHoverStrong`]
  /// for chrome buttons, [`AppTokens.overlayHover`] for in-row.
  final Color hoverBg;

  /// Whether to draw a 1px border around the button. Titlebar
  /// chrome buttons set this true; in-row buttons keep it false
  /// so the hover bg is the only visual affordance.
  final bool showBorder;
  final Color borderColor;

  /// Icon transparency. The sidebar's github slot renders the icon
  /// at 0.0 when the project hasn't resolved a URL yet — keeps row
  /// widths stable as the cache populates.
  final double iconOpacity;

  @override
  State<HoverIconButton> createState() => _HoverIconButtonState();
}

class _HoverIconButtonState extends State<HoverIconButton> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final enabled = widget.onPressed != null;
    final innerSize = widget.iconSize ?? (widget.size >= 28 ? 15.0 : 13.0);
    final container = Container(
      width: widget.size,
      height: widget.size,
      decoration: BoxDecoration(
        color: enabled && _hovered ? widget.hoverBg : widget.restBg,
        borderRadius: BorderRadius.circular(AppTokens.radiusSm),
        border: widget.showBorder
            ? Border.all(color: widget.borderColor)
            : null,
      ),
      alignment: Alignment.center,
      child: Opacity(
        opacity: widget.iconOpacity,
        child: AppIcon(
          widget.icon,
          size: innerSize,
          color: widget.iconColor,
        ),
      ),
    );
    if (!enabled) return container;
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: container,
        ),
      ),
    );
  }
}
