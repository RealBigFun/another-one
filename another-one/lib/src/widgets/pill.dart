// Reusable pill widget for tab/chip patterns. Replaces the
// `_TabPill` (right sidebar Changes/Commits/Checks) and
// `_AgentChip` (new-task modal) variants — both were the same
// rounded clickable container with hover/active state, an icon
// (sometimes), and a label.
//
// `_TabChip` (the terminal tab strip) stays separate: it has a
// trailing close × button and an agent-provider icon resolved
// from FRB-typed enums, both of which are heavier than this
// widget's slot abstraction warrants.
//
// Hover state is local — `StatefulWidget`. The `active` flag is
// caller-driven (the parent watches the source of truth and
// passes it in).

import 'package:flutter/material.dart';

import '../tokens.dart';
import 'app_icon.dart';

class Pill extends StatefulWidget {
  const Pill({
    super.key,
    required this.label,
    required this.active,
    this.onTap,
    this.icon,
    this.iconWidget,
    this.tooltip,
    this.height = 26,
    this.activeBg = AppTokens.overlayActive,
    this.hoverBg = AppTokens.overlayHover,
    this.restBg = Colors.transparent,
    this.activeBorderColor,
    this.borderColor,
    this.activeColor = AppTokens.textPrimary,
    this.inactiveColor = AppTokens.textSecondary,
    this.fontSize = AppTokens.fontBodyLg,
    this.fontWeight = FontWeight.w500,
    this.iconSize = 12,
    this.horizontalPadding = AppTokens.space3,
  });

  final String label;
  final bool active;

  /// `null` disables the tap handler — the pill stays rendered
  /// but the cursor stays default and hover bg doesn't flip.
  final VoidCallback? onTap;

  /// SVG glyph name passed through [`AppIcon`]. Mutually exclusive
  /// with [iconWidget]; if both are null, the pill renders label-
  /// only (matches `_AgentChip`'s no-icon "Shell" entry).
  final String? icon;

  /// Pre-built icon widget. Used when the icon source isn't an
  /// `AppIcon` SVG — e.g. agent-provider icons that resolve via
  /// `AgentProviderIcon`. Mutually exclusive with [icon].
  final Widget? iconWidget;

  final String? tooltip;
  final double height;
  final Color activeBg;
  final Color hoverBg;
  final Color restBg;

  /// Border drawn when [active] is true. `null` means no border —
  /// the right-sidebar tabs use this; the new-task agent chips set
  /// it to [`AppTokens.focusRing`].
  final Color? activeBorderColor;

  /// Border drawn when not active. `null` means no border. Set to
  /// [`AppTokens.border`] for filled chips that need an outline at
  /// rest (agent chips); leave null for tab strips that lean on
  /// hover bg only.
  final Color? borderColor;

  final Color activeColor;
  final Color inactiveColor;
  final double fontSize;
  final FontWeight fontWeight;
  final double iconSize;
  final double horizontalPadding;

  @override
  State<Pill> createState() => _PillState();
}

class _PillState extends State<Pill> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final enabled = widget.onTap != null;
    final bg = widget.active
        ? widget.activeBg
        : (enabled && _hovered ? widget.hoverBg : widget.restBg);
    final fg = widget.active ? widget.activeColor : widget.inactiveColor;
    final borderColor = widget.active
        ? widget.activeBorderColor
        : widget.borderColor;
    final container = Container(
      height: widget.height,
      padding: EdgeInsets.symmetric(horizontal: widget.horizontalPadding),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        border:
            borderColor != null ? Border.all(color: borderColor) : null,
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (widget.iconWidget != null) ...[
            widget.iconWidget!,
            const SizedBox(width: AppTokens.space2),
          ] else if (widget.icon != null) ...[
            AppIcon(widget.icon!, size: widget.iconSize, color: fg),
            const SizedBox(width: AppTokens.space2),
          ],
          Text(
            widget.label,
            style: TextStyle(
              fontSize: widget.fontSize,
              fontWeight: widget.fontWeight,
              color: fg,
            ),
          ),
        ],
      ),
    );
    final hoverable = MouseRegion(
      cursor: enabled
          ? SystemMouseCursors.click
          : SystemMouseCursors.basic,
      onEnter: enabled ? (_) => setState(() => _hovered = true) : null,
      onExit: enabled ? (_) => setState(() => _hovered = false) : null,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: container,
      ),
    );
    return widget.tooltip == null
        ? hoverable
        : Tooltip(message: widget.tooltip!, child: hoverable);
  }
}
