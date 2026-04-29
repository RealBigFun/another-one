use std::path::{Path, PathBuf};

use crate::AppWindow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppearancePreference {
    Light,
    Dark,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolvedAppearance {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AppliedAppearance {
    preference: AppearancePreference,
    resolved: ResolvedAppearance,
}

impl AppliedAppearance {
    pub(crate) fn preference_label(self) -> &'static str {
        self.preference.as_str()
    }

    pub(crate) fn resolved_label(self) -> &'static str {
        self.resolved.as_str()
    }
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
    sidebar_active_bg: slint::Color,
    sidebar_active_border: slint::Color,
    sidebar_icon_color: slint::Color,
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

pub(crate) fn apply_theme(app: &AppWindow) -> AppliedAppearance {
    let preference = load_appearance_preference();
    apply_theme_preference(app, preference)
}

pub(crate) fn cycle_and_persist_theme(app: &AppWindow) -> Result<AppliedAppearance, String> {
    let preference = AppearancePreference::parse(app.get_appearance_preference_label().as_str())
        .unwrap_or_else(load_appearance_preference)
        .next();
    persist_appearance_preference(preference)?;
    Ok(apply_theme_preference(app, preference))
}

fn apply_theme_preference(app: &AppWindow, preference: AppearancePreference) -> AppliedAppearance {
    let resolved = resolve_appearance::<HostAppearanceProfile>(preference);
    let theme = SlintTheme::for_appearance(resolved);

    app.set_chrome_bg(theme.chrome_bg);
    app.set_card_bg(theme.card_bg);
    app.set_sunken_bg(theme.sunken_bg);
    app.set_terminal_bg(theme.terminal_bg);
    app.set_overlay_hover(theme.overlay_hover);
    app.set_overlay_active(theme.overlay_active);
    app.set_sidebar_active_bg(theme.sidebar_active_bg);
    app.set_sidebar_active_border(theme.sidebar_active_border);
    app.set_sidebar_icon_color(theme.sidebar_icon_color);
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
    app.set_appearance_preference_label(preference.as_str().into());

    AppliedAppearance {
        preference,
        resolved,
    }
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

fn load_appearance_preference() -> AppearancePreference {
    appearance_preference_from_env("ANOTHERONE_SLINT_APPEARANCE")
        .or_else(|| {
            appearance_preference_path()
                .and_then(|path| read_persisted_appearance_preference(&path).ok().flatten())
        })
        .unwrap_or(AppearancePreference::System)
}

fn persist_appearance_preference(preference: AppearancePreference) -> Result<(), String> {
    let path = appearance_preference_path()
        .ok_or_else(|| "could not resolve a config directory for theme preference".to_string())?;
    write_persisted_appearance_preference(&path, preference)
}

fn appearance_preference_from_env(name: &str) -> Option<AppearancePreference> {
    AppearancePreference::parse(&std::env::var(name).ok()?)
}

fn appearance_from_env(name: &str) -> Option<ResolvedAppearance> {
    match AppearancePreference::parse(&std::env::var(name).ok()?)? {
        AppearancePreference::Light => Some(ResolvedAppearance::Light),
        AppearancePreference::Dark => Some(ResolvedAppearance::Dark),
        AppearancePreference::System => None,
    }
}

fn appearance_preference_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ANOTHERONE_SLINT_APPEARANCE_FILE") {
        return Some(PathBuf::from(path));
    }

    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;

    Some(config_home.join("another-one").join("slint-appearance"))
}

fn read_persisted_appearance_preference(
    path: &Path,
) -> Result<Option<AppearancePreference>, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(AppearancePreference::parse(&contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}

fn write_persisted_appearance_preference(
    path: &Path,
    preference: AppearancePreference,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    std::fs::write(path, format!("{}\n", preference.as_str()))
        .map_err(|error| format!("write {}: {error}", path.display()))
}

impl From<ResolvedAppearance> for AppearancePreference {
    fn from(appearance: ResolvedAppearance) -> Self {
        match appearance {
            ResolvedAppearance::Light => Self::Light,
            ResolvedAppearance::Dark => Self::Dark,
        }
    }
}

impl AppearancePreference {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "system" => Some(Self::System),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }
}

impl ResolvedAppearance {
    fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
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
            sidebar_active_bg: rgba(0x08, 0xff, 0xff, 0xff),
            sidebar_active_border: rgba(0x2e, 0xff, 0xff, 0xff),
            sidebar_icon_color: rgb(0x8c8c8c),
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
            sidebar_active_bg: rgba(0x08, 0x00, 0x00, 0x00),
            sidebar_active_border: rgba(0x2e, 0x00, 0x00, 0x00),
            sidebar_icon_color: rgb(0x6b7280),
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

    #[test]
    fn appearance_preference_parses_all_user_modes() {
        assert_eq!(
            AppearancePreference::parse("light"),
            Some(AppearancePreference::Light)
        );
        assert_eq!(
            AppearancePreference::parse("DARK"),
            Some(AppearancePreference::Dark)
        );
        assert_eq!(
            AppearancePreference::parse(" system\n"),
            Some(AppearancePreference::System)
        );
    }

    #[test]
    fn appearance_preference_cycles_through_user_modes() {
        assert_eq!(
            AppearancePreference::System.next(),
            AppearancePreference::Light
        );
        assert_eq!(
            AppearancePreference::Light.next(),
            AppearancePreference::Dark
        );
        assert_eq!(
            AppearancePreference::Dark.next(),
            AppearancePreference::System
        );
    }

    #[test]
    fn persisted_appearance_preference_round_trips() {
        let path = std::env::temp_dir().join(format!(
            "another-one-slint-appearance-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        assert_eq!(read_persisted_appearance_preference(&path).unwrap(), None);
        write_persisted_appearance_preference(&path, AppearancePreference::Light).unwrap();
        assert_eq!(
            read_persisted_appearance_preference(&path).unwrap(),
            Some(AppearancePreference::Light)
        );

        let _ = std::fs::remove_file(path);
    }
}
