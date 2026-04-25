// Responsive breakpoint primitives for the unified Flutter app.
//
// One Flutter codebase ships to 5 platforms (Android, iOS, Linux,
// macOS, Windows) at any aspect ratio. The user's directive is "web
// dev style viewports" — pick a layout per breakpoint, not per
// platform. A wide-aspect tablet renders the same layout as a
// laptop; a phone in landscape reflows differently from portrait.
//
// Breakpoints are width-only (height is rarely the binding
// constraint). Names are descriptive of the *layout*, not the
// device — "tablet" doesn't mean iPad, it means the size class
// where a master-detail two-column layout fits.
//
//   ┌─────────────────────────────────────────────────────────┐
//   │ phoneCompact     <  360 dp  iPhone SE 1st gen, mini phones │
//   │ phoneRegular     ≥  360 dp  most phones in portrait        │
//   │ phoneLandscape   ≥  600 dp  phone in landscape, small tab  │
//   │ tablet           ≥  840 dp  iPad / Android tablet portrait │
//   │ desktop          ≥ 1200 dp  laptop, tablet landscape       │
//   │ wideDesktop      ≥ 1600 dp  external monitor               │
//   └─────────────────────────────────────────────────────────┘
//
// Numbers come from Material 3 window-size classes with two extra
// rungs (phoneCompact + wideDesktop) for the polar ends. Keep them
// here — every screen reads them via [Breakpoint.of(context)].

import 'package:flutter/widgets.dart';

/// Discrete window-size classes the UI branches on.
enum Breakpoint {
  phoneCompact,
  phoneRegular,
  phoneLandscape,
  tablet,
  desktop,
  wideDesktop;

  /// Pick a breakpoint from the current `MediaQuery`. Cheaper than
  /// `LayoutBuilder` when the whole subtree only cares about the
  /// window size, not its parent's constraints.
  static Breakpoint of(BuildContext context) =>
      forWidth(MediaQuery.sizeOf(context).width);

  /// Same logic, but for a precomputed width. Useful in
  /// `LayoutBuilder` builders.
  static Breakpoint forWidth(double width) {
    if (width < 360) return Breakpoint.phoneCompact;
    if (width < 600) return Breakpoint.phoneRegular;
    if (width < 840) return Breakpoint.phoneLandscape;
    if (width < 1200) return Breakpoint.tablet;
    if (width < 1600) return Breakpoint.desktop;
    return Breakpoint.wideDesktop;
  }

  /// True for breakpoints that should render a phone-style single-
  /// column layout (push-navigation, bottom sheets, full-screen
  /// modals).
  bool get isPhone =>
      this == phoneCompact ||
      this == phoneRegular ||
      this == phoneLandscape;

  /// True for breakpoints with enough width to host a persistent
  /// sidebar / master-detail split.
  bool get hasSidebar =>
      this == tablet || this == desktop || this == wideDesktop;
}

/// Builds a different widget per breakpoint. Required builders:
/// `phone` and `desktop`. The rest fall back to the closest larger
/// rung — phoneLandscape falls back to desktop, tablet to desktop —
/// so most screens only need to specify two builders to start.
///
/// Use this for top-level layout decisions; for fine-grained
/// `if (bp == ...)` branches inside a build method, just call
/// [Breakpoint.of] directly.
class Responsive extends StatelessWidget {
  const Responsive({
    super.key,
    required this.phone,
    required this.desktop,
    this.phoneCompact,
    this.phoneLandscape,
    this.tablet,
    this.wideDesktop,
  });

  final WidgetBuilder phone;
  final WidgetBuilder desktop;
  final WidgetBuilder? phoneCompact;
  final WidgetBuilder? phoneLandscape;
  final WidgetBuilder? tablet;
  final WidgetBuilder? wideDesktop;

  @override
  Widget build(BuildContext context) {
    final bp = Breakpoint.of(context);
    final builder = switch (bp) {
      Breakpoint.phoneCompact => phoneCompact ?? phone,
      Breakpoint.phoneRegular => phone,
      Breakpoint.phoneLandscape => phoneLandscape ?? desktop,
      Breakpoint.tablet => tablet ?? desktop,
      Breakpoint.desktop => desktop,
      Breakpoint.wideDesktop => wideDesktop ?? desktop,
    };
    return builder(context);
  }
}
