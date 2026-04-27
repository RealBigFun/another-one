// ThemeData factory mirroring the AnotherOne desktop look.
//
// Desktop is dark-only; we follow suit. Palette sourced from
// `desktop/src/tokens.rs` via `tokens.dart`. Material 3 is on so the
// newer component tokens (surfaceContainerHigh, etc.) are addressable
// by the few places that still use them.

import 'package:flutter/material.dart';

import 'tokens.dart';

ThemeData buildAppTheme() {
  const scheme = ColorScheme.dark(
    surface: AppTokens.chromeBg,
    onSurface: AppTokens.textPrimary,
    onSurfaceVariant: AppTokens.textSecondary,
    primary: Color(0xFFA5B4FC), // periwinkle accent (focus ring lightness).
    onPrimary: Color(0xFF111318),
    secondary: Color(0xFF8B9BBC),
    onSecondary: AppTokens.textPrimary,
    error: Color(0xFFF0A5A5),
    onError: Color(0xFF111318),
    outline: AppTokens.border,
    surfaceContainerHighest: AppTokens.cardBg,
    surfaceContainerHigh: AppTokens.cardBg,
    surfaceContainer: AppTokens.sunkenBg,
  );

  final base = ThemeData(
    useMaterial3: true,
    brightness: Brightness.dark,
    colorScheme: scheme,
    scaffoldBackgroundColor: AppTokens.chromeBg,
    canvasColor: AppTokens.chromeBg,
    dividerColor: AppTokens.divider,
    hintColor: AppTokens.textMuted,
    // GPUI desktop is monospace-only — Lilex Nerd Font Mono for
    // every label. Wire it on the root ThemeData so every Text /
    // Material widget inherits without per-callsite fontFamily.
    fontFamily: AppTokens.fontFamily,
  );

  return base.copyWith(
    appBarTheme: const AppBarTheme(
      backgroundColor: AppTokens.chromeBg,
      foregroundColor: AppTokens.textPrimary,
      elevation: 0,
      scrolledUnderElevation: 0,
      centerTitle: false,
      titleTextStyle: TextStyle(
        color: AppTokens.textPrimary,
        fontSize: AppTokens.fontHeadingSm,
        fontWeight: FontWeight.w600,
      ),
    ),
    textTheme: base.textTheme.apply(
      bodyColor: AppTokens.textPrimary,
      displayColor: AppTokens.textPrimary,
    ),
    iconTheme: const IconThemeData(
      color: AppTokens.textSecondary,
      size: AppTokens.iconSizeDefault,
    ),
    listTileTheme: const ListTileThemeData(
      iconColor: AppTokens.textSecondary,
      textColor: AppTokens.textPrimary,
      dense: true,
      contentPadding: EdgeInsets.symmetric(
        horizontal: AppTokens.space5,
        vertical: AppTokens.space1,
      ),
    ),
    dividerTheme: const DividerThemeData(
      color: AppTokens.divider,
      thickness: 1,
      space: 1,
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        backgroundColor: scheme.primary,
        foregroundColor: scheme.onPrimary,
        padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.space7,
          vertical: AppTokens.space4,
        ),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        ),
        textStyle: const TextStyle(
          fontSize: AppTokens.fontBodyLg,
          fontWeight: FontWeight.w600,
        ),
      ),
    ),
    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(
        foregroundColor: AppTokens.textSecondary,
        textStyle: const TextStyle(fontSize: AppTokens.fontBody),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        foregroundColor: AppTokens.textPrimary,
        side: const BorderSide(color: AppTokens.border),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppTokens.radiusSm),
        ),
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: AppTokens.sunkenBg,
      isDense: true,
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        borderSide: const BorderSide(color: AppTokens.border),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        borderSide: const BorderSide(color: AppTokens.border),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(AppTokens.radiusMd),
        borderSide: BorderSide(color: AppTokens.focusRing),
      ),
      hintStyle: const TextStyle(color: AppTokens.textPlaceholder),
      labelStyle: const TextStyle(color: AppTokens.textSecondary),
    ),
    progressIndicatorTheme: ProgressIndicatorThemeData(color: scheme.primary),
  );
}
