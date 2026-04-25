// Thin wrapper around `flutter_svg` for the GPUI-shipped icon set.
//
// All sidebar + titlebar glyphs come from `assets/icons/icons__*.svg`,
// the same files the GPUI desktop ships under `desktop/assets/icons/`.
// Bundling them keeps the two UIs visually identical at the icon
// level — no "Material approximation" drift.

import 'package:flutter/widgets.dart';
import 'package:flutter_svg/flutter_svg.dart';

class AppIcon extends StatelessWidget {
  const AppIcon(
    this.name, {
    super.key,
    required this.size,
    required this.color,
  });

  /// File name without the `icons__` prefix or `.svg` suffix —
  /// e.g. `chevron-down`, `pin-off`, `qr-code`.
  final String name;
  final double size;
  final Color color;

  @override
  Widget build(BuildContext context) {
    return SvgPicture.asset(
      'assets/icons/icons__$name.svg',
      width: size,
      height: size,
      colorFilter: ColorFilter.mode(color, BlendMode.srcIn),
    );
  }
}
