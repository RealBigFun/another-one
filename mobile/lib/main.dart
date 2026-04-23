// Mobile client for the AnotherOne companion daemon. Entry point
// only — all screens live under `lib/src/`:
//
//   - `app_root.dart` — routes between Pair and Drawer based on
//     whether an endpoint is saved.
//   - `pair_device_page.dart` — onboarding / QR pairing.
//   - `projects_drawer_page.dart` — home: expandable project list.
//   - `task_page.dart` — per-task terminal with tab strip.
//   - `settings_page.dart` — unlink / rescan.
//   - `qr_scan_page.dart` — pairing QR scanner, pushed from either.

import 'package:flutter/material.dart';

import 'src/app_root.dart';
import 'src/rust/frb_generated.dart';
import 'src/theme.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  runApp(const AnotherOneApp());
}

class AnotherOneApp extends StatelessWidget {
  const AnotherOneApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: 'AnotherOne',
        theme: buildAppTheme(),
        home: const AppRoot(),
      );
}
