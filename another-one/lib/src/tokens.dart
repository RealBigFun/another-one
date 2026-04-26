// Design tokens — port of `desktop/src/tokens.rs` (+ selected layout
// constants from `desktop/src/layout.rs` and the project-color palette
// from `desktop/src/theme.rs`).
//
// Values mirror the desktop dark-only aesthetic exactly; when editing
// here, update the Rust side too so the two apps keep visual parity.
//
// Spacing / radius / font-size constants use plain `double`s because
// Flutter's `EdgeInsets` etc. all take doubles directly.

import 'package:flutter/material.dart';

/// Design tokens mirroring `desktop/src/tokens.rs`.
class AppTokens {
  AppTokens._();

  // ── Surface colours ────────────────────────────────────────────────
  /// Titlebar + sidebar chrome; darkest neutral surface.
  static const Color chromeBg = Color(0xFF27292E);

  /// Modal + dropdown card surface; one step lighter than chrome.
  static const Color cardBg = Color(0xFF2B2D31);

  /// Subtle sunken background for list-row backgrounds and footer.
  static const Color sunkenBg = Color(0xFF202329);

  /// Terminal / editor surface; darker than chrome.
  static const Color terminalBg = Color(0xFF17191D);

  /// Modal scrim — translucent black.
  static const Color scrimBg = Color(0x80000000);

  // ── Text colours (four brightness rungs) ──────────────────────────
  static const Color textPrimary = Color(0xEBFFFFFF); // 0.92 alpha white
  static const Color textSecondary = Color(0xC7FFFFFF); // 0.78
  static const Color textMuted = Color(0x94FFFFFF); // 0.58
  static const Color textPlaceholder = Color(0x61FFFFFF); // 0.38

  // ── Borders & dividers ─────────────────────────────────────────────
  static const Color border = Color(0x14FFFFFF); // 0.08
  static const Color divider = Color(0x0FFFFFFF); // 0.06

  // ── Interactive overlays ───────────────────────────────────────────
  static const Color overlayRest = Color(0x0AFFFFFF); // 0.04
  static const Color overlayHover = Color(0x0FFFFFFF); // 0.06
  static const Color overlayHoverStrong = Color(0x14FFFFFF); // 0.08
  static const Color overlayActive = Color(0x1AFFFFFF); // 0.10

  // ── Focus / accents ────────────────────────────────────────────────
  /// Cool periwinkle focus ring.
  static Color focusRing = HSLColor.fromAHSL(1.0, 220, 0.55, 0.60).toColor();

  /// Primary accent — used by pin glyphs, selected-chip outlines, etc.
  /// Mirrors the periwinkle used on the desktop sidebar's active row.
  /// `const` so widgets can inline it.
  static const Color accent = Color(0xFF7B8FD9);

  /// Diff-stat colours — match the GPUI sidebar's `+N -N` badges
  /// (`hsla(138/360, 0.50, 0.74)` green and `hsla(352/360, 0.52,
  /// 0.76)` red, computed once into the equivalent sRGB constants).
  static const Color diffAdded = Color(0xFFA0D9B4);
  static const Color diffRemoved = Color(0xFFEBA8B0);

  // ── Semantic chrome (subset used by mobile) ────────────────────────
  static Color successIcon =
      HSLColor.fromAHSL(1.0, 138, 0.52, 0.66).toColor();
  static Color errorIcon = HSLColor.fromAHSL(1.0, 0, 0.68, 0.72).toColor();
  /// Background + text for the TaskPage error banner. Muted so it
  /// doesn't clash with terminal output below but distinct enough
  /// that a dropped connection is obvious at a glance.
  static const Color errorBg = Color(0xFF5A2A2E);
  static const Color errorText = Color(0xFFFFD6DC);
  static Color warningIcon =
      HSLColor.fromAHSL(1.0, 42, 0.70, 0.68).toColor();
  static Color infoIcon = HSLColor.fromAHSL(1.0, 208, 0.62, 0.72).toColor();

  // ── Chevron / icon grey used by sidebar rows ──────────────────────
  // `const` so call sites can use `AppTokens.chevron` inside const
  // Icon() constructors (Flutter requires the color arg be const).
  static const Color chevron = Color(0xFF8C8C8C);

  // ── Typography ─────────────────────────────────────────────────────
  /// Primary app font. GPUI desktop bundled Lilex NerdFont Mono and
  /// used it for every label + glyph + modal — we mirror that
  /// (registered as `Lilex` in pubspec.yaml fonts section). Mobile
  /// falls back gracefully when the bundled TTFs aren't shipped.
  static const String fontFamily = 'Lilex';

  /// Backwards-compatible alias for callers that explicitly want a
  /// monospace font. Same family as the primary one — Lilex IS
  /// monospace; the desktop has no proportional secondary.
  static const String fontFamilyMono = 'Lilex';

  static const double fontCaption = 10;
  static const double fontSmall = 11;
  static const double fontBody = 12;
  static const double fontBodyLg = 13;
  static const double fontHeadingSm = 14;
  static const double fontHeading = 18;
  static const double fontHeadingLg = 20;

  // ── Spacing scale ──────────────────────────────────────────────────
  static const double space1 = 4;
  static const double space2 = 6;
  static const double space3 = 8;
  static const double space4 = 10;
  static const double space5 = 12;
  static const double space6 = 14;
  static const double space7 = 16;
  static const double space8 = 18;
  static const double space9 = 20;
  static const double space10 = 24;

  // ── Border radii ───────────────────────────────────────────────────
  static const double radiusXs = 4;
  static const double radiusSm = 6;
  static const double radiusMd = 8;
  static const double radiusLg = 10;
  static const double radiusXl = 12;
  static const double radius2xl = 14;
  static const double radiusPill = 999;

  // ── Component scales ───────────────────────────────────────────────
  static const double iconSizeXs = 9;
  static const double iconSizeSm = 11;
  static const double iconSizeDefault = 16;
  static const double iconSizeLg = 26;

  /// Height of the tab-strip bar (mirrors desktop `TERMINAL_TAB_BAR_H`).
  static const double tabStripHeight = 36;

  /// Padding inside the terminal view (mirrors `TERMINAL_VIEW_PADDING`).
  static const double terminalViewPadding = 12;

  // ── Desktop layout constants (mirror desktop/src/layout.rs) ────────
  /// Titlebar height — `TITLEBAR_CHROME_H`.
  static const double titlebarHeight = 38;

  /// Project row height — `PROJECT_ROW_H` (left_sidebar.rs).
  static const double projectRowHeight = 34;

  /// Task ("branch") row height — `BRANCH_ROW_H` (left_sidebar.rs).
  static const double taskRowHeight = 44;

  /// Top padding inside the sidebar list — `LIST_TOP_PAD`.
  static const double sidebarListTopPad = 4;

  /// Gap between sidebar rows — `LIST_GAP`.
  static const double sidebarListGap = 2;

  /// Project context-menu width — `MENU_W`.
  static const double projectMenuWidth = 316;

  /// Task context-menu width — `TASK_MENU_W`.
  static const double taskMenuWidth = 248;

  /// Default sidebar width.
  static const double sidebarWidth = 280;

  /// Project avatar size + corner radius (left_sidebar.rs).
  static const double avatarSize = 24;
  static const double avatarRadius = 5;

  // ── Active row chrome (left_sidebar.rs) ────────────────────────────
  /// Active row background — white().opacity(0.03).
  static const Color rowActiveBg = Color(0x08FFFFFF);

  /// Active row border — white().opacity(0.18).
  static const Color rowActiveBorder = Color(0x2EFFFFFF);

  // ── Destructive button colours (matches GPUI delete buttons) ────────
  static const Color dangerBg = Color(0xFF8A2A2A);
  static const Color dangerHover = Color(0xFFA03A3A);

  // ── Project-avatar palette (from `desktop/src/theme.rs`) ───────────
  static const List<Color> projectColors = [
    Color(0xFF5B4A9E), // purple
    Color(0xFF2E7D6F), // teal
    Color(0xFFB85C38), // burnt orange
    Color(0xFF3A6EA5), // blue
    Color(0xFF8B5E3C), // brown
    Color(0xFF7B2D5F), // magenta
    Color(0xFF4A7C4B), // green
    Color(0xFF9C5151), // rose
  ];

  /// Deterministic project-avatar colour. Matches
  /// `desktop/src/theme.rs::project_color` (byte-wise FNV-ish hash
  /// with u32 wrapping arithmetic). Mask is `0xFFFFFFFF`, NOT
  /// `0x7fffffff` — GPUI's hash uses `u32::wrapping_mul/wrapping_add`
  /// which wraps mod 2^32; the prior 31-bit mask diverged for any
  /// project id long enough that an intermediate hash overflowed
  /// 31 bits (e.g. `aws-vpn-client` rendered purple here vs the
  /// rose tone GPUI showed).
  static Color projectColor(String id) {
    var hash = 0;
    for (final b in id.codeUnits) {
      hash = ((hash * 31) + b) & 0xFFFFFFFF;
    }
    return projectColors[hash % projectColors.length];
  }
}
