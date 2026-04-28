#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlintInputPolicy {
    DesktopKeyboard,
    TouchIme,
}

impl SlintInputPolicy {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::DesktopKeyboard => "keyboard",
            Self::TouchIme => "touch-ime",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlintPlatformProfile {
    pub(crate) target: &'static str,
    pub(crate) app_id: &'static str,
    pub(crate) mobile: bool,
    pub(crate) input_policy: SlintInputPolicy,
    pub(crate) window_label: &'static str,
}

impl SlintPlatformProfile {
    pub(crate) fn label(self) -> String {
        format!("{} / {}", self.target, self.input_policy.label())
    }
}

pub(crate) fn current_platform_profile() -> SlintPlatformProfile {
    current_platform_profile_for_target(std::env::consts::OS)
}

fn current_platform_profile_for_target(target_os: &str) -> SlintPlatformProfile {
    match target_os {
        "android" => SlintPlatformProfile {
            target: "android",
            app_id: "com.anotherone.slint",
            mobile: true,
            input_policy: SlintInputPolicy::TouchIme,
            window_label: "android-activity",
        },
        "ios" => SlintPlatformProfile {
            target: "ios",
            app_id: "com.anotherone.slint",
            mobile: true,
            input_policy: SlintInputPolicy::TouchIme,
            window_label: "ios-scene",
        },
        "macos" => SlintPlatformProfile {
            target: "macos",
            app_id: "com.anotherone.Slint",
            mobile: false,
            input_policy: SlintInputPolicy::DesktopKeyboard,
            window_label: "desktop-window",
        },
        "linux" => SlintPlatformProfile {
            target: "linux",
            app_id: "com.anotherone.Slint",
            mobile: false,
            input_policy: SlintInputPolicy::DesktopKeyboard,
            window_label: "desktop-window",
        },
        _ => SlintPlatformProfile {
            target: "unsupported",
            app_id: "com.anotherone.Slint",
            mobile: false,
            input_policy: SlintInputPolicy::DesktopKeyboard,
            window_label: "unsupported-window",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_profile_uses_desktop_keyboard_policy() {
        let profile = current_platform_profile_for_target("linux");
        assert_eq!(profile.input_policy, SlintInputPolicy::DesktopKeyboard);
        assert!(!profile.mobile);
    }

    #[test]
    fn android_profile_uses_touch_ime_policy() {
        let profile = current_platform_profile_for_target("android");
        assert_eq!(profile.input_policy, SlintInputPolicy::TouchIme);
        assert!(profile.mobile);
    }

    #[test]
    fn unsupported_profile_is_explicit() {
        let profile = current_platform_profile_for_target("windows");
        assert_eq!(profile.target, "unsupported");
        assert_eq!(profile.window_label, "unsupported-window");
    }
}
