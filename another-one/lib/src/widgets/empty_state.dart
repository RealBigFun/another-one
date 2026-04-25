// Centred placeholder text widget used for empty/loading/error
// states. Replaces three near-identical privates that had spread
// across the desktop shell:
//
//   - `_PaneEmptyState` — right sidebar's "Working tree clean" /
//     "No commits to show" / "No checks to show".
//   - `_WelcomePlaceholder` — main pane "No project selected".
//   - `_SidebarMessage` — sidebar "Project list error: …".
//
// All three were `Center → Text(muted, body-lg)` with optional
// padding. This widget keeps that shape but collapses the variants
// into one call site so the screens stop carrying their own copies.

import 'package:flutter/material.dart';

import '../tokens.dart';

class EmptyState extends StatelessWidget {
  const EmptyState({
    super.key,
    required this.text,
    this.padding,
    this.color = AppTokens.textMuted,
    this.fontSize = AppTokens.fontBodyLg,
  });

  final String text;

  /// Outer padding. Centring isn't always enough — the welcome
  /// placeholder, for instance, leans a bit off-centre with a
  /// `space10` symmetric padding to leave room for the right
  /// sidebar.
  final EdgeInsets? padding;

  final Color color;
  final double fontSize;

  @override
  Widget build(BuildContext context) {
    final body = Center(
      child: Text(
        text,
        style: TextStyle(fontSize: fontSize, color: color),
      ),
    );
    return padding == null ? body : Padding(padding: padding!, child: body);
  }
}
