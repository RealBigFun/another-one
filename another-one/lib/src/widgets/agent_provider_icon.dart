// Maps a daemon-side `AgentProvider` enum value to its SVG glyph
// in the shared icon set. Mirrors `desktop/src/terminal_tab_bar.rs`'s
// `tab_icon_element` so the same task in the two UIs renders with
// the same provider mark.

import 'package:flutter/widgets.dart';

import '../rust/api/iroh_client.dart' show AgentProvider;
import 'app_icon.dart';

class AgentProviderIcon extends StatelessWidget {
  const AgentProviderIcon({
    super.key,
    required this.provider,
    required this.size,
    required this.color,
  });

  /// `null` falls back to the generic shell glyph — matches GPUI's
  /// behaviour for tabs that don't carry a launch_config.
  final AgentProvider? provider;
  final double size;
  final Color color;

  @override
  Widget build(BuildContext context) {
    final iconName = _iconNameFor(provider);
    return AppIcon(iconName, size: size, color: color);
  }

  static String _iconNameFor(AgentProvider? provider) {
    return switch (provider) {
      AgentProvider.claudeCode => 'claude-ai',
      AgentProvider.cursorAgent => 'cursor-ai',
      AgentProvider.codex => 'codex-ai',
      AgentProvider.gemini => 'gemini-ai',
      AgentProvider.pi => 'brain',
      // Fallbacks for providers without a dedicated bundled glyph
      // — `brain` is the GPUI default for non-specific agent kinds
      // and matches the visual the desktop currently ships.
      AgentProvider.openCode || AgentProvider.amp ||
      AgentProvider.rovoDev || AgentProvider.forge ||
      AgentProvider.shell => 'brain',
      null => 'brain',
    };
  }
}
