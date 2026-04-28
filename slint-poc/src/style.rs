use crate::AppWindow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppearancePreference {
    Light,
    Dark,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolvedAppearance {
    Light,
    Dark,
}

/// Platform seam for appearance resolution.
///
/// The production platform layer should replace `HostAppearanceProfile`
/// with target-specific implementations. Until those APIs are wired, the
/// profile supports deterministic env overrides and otherwise preserves the
/// GPUI dark baseline.
trait SlintAppearanceProfile {
    fn system_appearance() -> Option<ResolvedAppearance>;
}

struct HostAppearanceProfile;

impl SlintAppearanceProfile for HostAppearanceProfile {
    fn system_appearance() -> Option<ResolvedAppearance> {
        appearance_from_env("ANOTHERONE_SLINT_SYSTEM_APPEARANCE")
    }
}

#[derive(Clone, Copy, Debug)]
struct SlintTheme {
    label: &'static str,
    chrome_bg: slint::Color,
    card_bg: slint::Color,
    sunken_bg: slint::Color,
    terminal_bg: slint::Color,
    overlay_hover: slint::Color,
    overlay_active: slint::Color,
    border_color: slint::Color,
    divider_color: slint::Color,
    focus_ring: slint::Color,
    text_primary: slint::Color,
    text_secondary: slint::Color,
    text_muted: slint::Color,
    success_color: slint::Color,
    warning_color: slint::Color,
    danger_color: slint::Color,
}

pub(crate) fn apply_theme(app: &AppWindow) {
    let preference = appearance_from_env("ANOTHERONE_SLINT_APPEARANCE")
        .map(AppearancePreference::from)
        .unwrap_or(AppearancePreference::System);
    let resolved = resolve_appearance::<HostAppearanceProfile>(preference);
    let theme = SlintTheme::for_appearance(resolved);

    app.set_chrome_bg(theme.chrome_bg);
    app.set_card_bg(theme.card_bg);
    app.set_sunken_bg(theme.sunken_bg);
    app.set_terminal_bg(theme.terminal_bg);
    app.set_overlay_hover(theme.overlay_hover);
    app.set_overlay_active(theme.overlay_active);
    app.set_border_color(theme.border_color);
    app.set_divider_color(theme.divider_color);
    app.set_focus_ring(theme.focus_ring);
    app.set_text_primary(theme.text_primary);
    app.set_text_secondary(theme.text_secondary);
    app.set_text_muted(theme.text_muted);
    app.set_success_color(theme.success_color);
    app.set_warning_color(theme.warning_color);
    app.set_danger_color(theme.danger_color);
    app.set_appearance_label(theme.label.into());
}

fn resolve_appearance<T: SlintAppearanceProfile>(
    preference: AppearancePreference,
) -> ResolvedAppearance {
    match preference {
        AppearancePreference::Light => ResolvedAppearance::Light,
        AppearancePreference::Dark => ResolvedAppearance::Dark,
        AppearancePreference::System => T::system_appearance().unwrap_or(ResolvedAppearance::Dark),
    }
}

fn appearance_from_env(name: &str) -> Option<ResolvedAppearance> {
    match std::env::var(name).ok()?.to_ascii_lowercase().as_str() {
        "light" => Some(ResolvedAppearance::Light),
        "dark" => Some(ResolvedAppearance::Dark),
        _ => None,
    }
}

impl From<ResolvedAppearance> for AppearancePreference {
    fn from(appearance: ResolvedAppearance) -> Self {
        match appearance {
            ResolvedAppearance::Light => Self::Light,
            ResolvedAppearance::Dark => Self::Dark,
        }
    }
}

impl SlintTheme {
    fn for_appearance(appearance: ResolvedAppearance) -> Self {
        match appearance {
            ResolvedAppearance::Dark => Self::dark(),
            ResolvedAppearance::Light => Self::light(),
        }
    }

    fn dark() -> Self {
        Self {
            label: "dark",
            chrome_bg: rgb(0x27292e),
            card_bg: rgb(0x2b2d31),
            sunken_bg: rgb(0x202329),
            terminal_bg: rgb(0x17191d),
            overlay_hover: rgba(0x0f, 0xff, 0xff, 0xff),
            overlay_active: rgba(0x1a, 0xff, 0xff, 0xff),
            border_color: rgba(0x16, 0xff, 0xff, 0xff),
            divider_color: rgba(0x10, 0xff, 0xff, 0xff),
            focus_ring: rgb(0x5d7ad5),
            text_primary: rgb(0xebebeb),
            text_secondary: rgb(0xc7c7c7),
            text_muted: rgb(0x949494),
            success_color: rgb(0x7ad591),
            warning_color: rgb(0xe5c07b),
            danger_color: rgb(0xe06c75),
        }
    }

    fn light() -> Self {
        Self {
            label: "light",
            chrome_bg: rgb(0xf1f2f4),
            card_bg: rgb(0xffffff),
            sunken_bg: rgb(0xe8eaee),
            terminal_bg: rgb(0xf8f7f2),
            overlay_hover: rgba(0x10, 0x00, 0x00, 0x00),
            overlay_active: rgba(0x1c, 0x00, 0x00, 0x00),
            border_color: rgba(0x22, 0x00, 0x00, 0x00),
            divider_color: rgba(0x18, 0x00, 0x00, 0x00),
            focus_ring: rgb(0x4f68bd),
            text_primary: rgb(0x17191d),
            text_secondary: rgb(0x41454d),
            text_muted: rgb(0x68707b),
            success_color: rgb(0x237a42),
            warning_color: rgb(0xa36a00),
            danger_color: rgb(0xb3343e),
        }
    }
}

fn rgb(value: u32) -> slint::Color {
    slint::Color::from_argb_encoded(0xff000000 | value)
}

fn rgba(a: u8, r: u8, g: u8, b: u8) -> slint::Color {
    slint::Color::from_argb_u8(a, r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoSystemAppearance;

    impl SlintAppearanceProfile for NoSystemAppearance {
        fn system_appearance() -> Option<ResolvedAppearance> {
            None
        }
    }

    struct LightSystemAppearance;

    impl SlintAppearanceProfile for LightSystemAppearance {
        fn system_appearance() -> Option<ResolvedAppearance> {
            Some(ResolvedAppearance::Light)
        }
    }

    #[test]
    fn system_appearance_falls_back_to_dark_baseline() {
        assert_eq!(
            resolve_appearance::<NoSystemAppearance>(AppearancePreference::System),
            ResolvedAppearance::Dark
        );
    }

    #[test]
    fn system_appearance_uses_platform_profile_when_available() {
        assert_eq!(
            resolve_appearance::<LightSystemAppearance>(AppearancePreference::System),
            ResolvedAppearance::Light
        );
    }

    #[test]
    fn explicit_appearance_overrides_system_profile() {
        assert_eq!(
            resolve_appearance::<LightSystemAppearance>(AppearancePreference::Dark),
            ResolvedAppearance::Dark
        );
    }
}
