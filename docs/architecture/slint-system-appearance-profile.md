# Slint System Appearance Profile

System appearance is resolved behind the style/profile seam instead of in view code.

## Current Behavior

`ANOTHERONE_SLINT_APPEARANCE` accepts:

- `light`
- `dark`
- unset or any other value: system mode

System mode currently checks `ANOTHERONE_SLINT_SYSTEM_APPEARANCE=light|dark` through `HostAppearanceProfile`. If unavailable, it falls back to dark because GPUI dark is the product baseline.

## Required Production Behavior

- Linux: detect portal or desktop preference when available; otherwise dark fallback.
- macOS: use native appearance API.
- Android: use activity/system night mode.
- iOS: use trait collection style.
- Changes at runtime should trigger `style::apply_theme` again.
