# Slint Style Artifact Model

Slint style resolution lives in `slint-poc/src/style.rs` and feeds semantic properties on `AppWindow`. Views and components consume those properties through Slint bindings.

## Runtime Model

`style::apply_theme(&AppWindow)` resolves:

1. `ANOTHERONE_SLINT_APPEARANCE=light|dark`, when explicitly set.
2. `ANOTHERONE_SLINT_APPEARANCE=system` or unset, using the active `SlintAppearanceProfile`.
3. `ANOTHERONE_SLINT_SYSTEM_APPEARANCE=light|dark` as the temporary host-profile implementation.
4. GPUI dark baseline fallback when no platform value is available.

The current platform seam is:

- `SlintAppearanceProfile::system_appearance() -> Option<ResolvedAppearance>`
- `HostAppearanceProfile`, which reads the deterministic env override.

This keeps platform nuance out of view/layout/component code and gives Linux/macOS/Android/iOS implementations a narrow integration point later.

## Slint AppWindow Roles

The style module writes these semantic roles:

- `chrome_bg`
- `card_bg`
- `sunken_bg`
- `terminal_bg`
- `overlay_hover`
- `overlay_active`
- `border_color`
- `divider_color`
- `focus_ring`
- `text_primary`
- `text_secondary`
- `text_muted`
- `success_color`
- `warning_color`
- `danger_color`
- `appearance_label`

## Appearance Bundles

Dark mode mirrors the GPUI baseline. Light mode is intentionally conservative: it inverts surface hierarchy, preserves the blue focus ring family, and uses darker semantic foreground colors for success/warning/danger.

## Verification

Current automated coverage:

- `style::tests::system_appearance_falls_back_to_dark_baseline`
- `style::tests::system_appearance_uses_platform_profile_when_available`
- `style::tests::explicit_appearance_overrides_system_profile`
- `cargo check -p slint-poc`

Remaining evidence:

- GPUI vs Slint dark screenshot crops.
- Light-mode review screenshots.
- System appearance hooks backed by real platform APIs instead of env-only host profile.
- Font packaging and typography capture.
