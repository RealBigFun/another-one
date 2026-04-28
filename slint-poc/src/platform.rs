use std::io::Write;
use std::process::{Command, Stdio};

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

pub(crate) fn open_uri(uri: &str) -> Result<(), String> {
    let uri = uri.trim();
    if uri.is_empty() {
        return Err("empty URI".to_string());
    }

    let Some(program) = open_uri_program_for_target(std::env::consts::OS) else {
        return Err(format!(
            "opening links is not supported on {}",
            std::env::consts::OS
        ));
    };

    Command::new(program)
        .arg(uri)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to run {program}: {error}"))
}

pub(crate) fn copy_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("empty selection".to_string());
    }

    let programs = copy_programs_for_target(std::env::consts::OS);
    if programs.is_empty() {
        return Err(format!(
            "copying terminal selections is not supported on {}",
            std::env::consts::OS
        ));
    }

    let mut errors = Vec::new();
    for &program in programs {
        match write_clipboard_program(program, text) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(error),
        }
    }

    Err(format!("clipboard command failed: {}", errors.join("; ")))
}

fn write_clipboard_program(program: ClipboardProgram, text: &str) -> Result<(), String> {
    let mut child = Command::new(program.name)
        .args(program.args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{}: {error}", program.name))?;

    let Some(mut stdin) = child.stdin.take() else {
        return Err(format!("{}: stdin unavailable", program.name));
    };
    stdin
        .write_all(text.as_bytes())
        .map_err(|error| format!("{}: {error}", program.name))?;
    drop(stdin);

    let status = child
        .wait()
        .map_err(|error| format!("{}: {error}", program.name))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} exited with {status}", program.name))
    }
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

fn open_uri_program_for_target(target_os: &str) -> Option<&'static str> {
    match target_os {
        "linux" => Some("xdg-open"),
        "macos" => Some("open"),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClipboardProgram {
    name: &'static str,
    args: &'static [&'static str],
}

fn copy_programs_for_target(target_os: &str) -> &'static [ClipboardProgram] {
    match target_os {
        "linux" => &[
            ClipboardProgram {
                name: "wl-copy",
                args: &[],
            },
            ClipboardProgram {
                name: "xclip",
                args: &["-selection", "clipboard"],
            },
        ],
        "macos" => &[ClipboardProgram {
            name: "pbcopy",
            args: &[],
        }],
        _ => &[],
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

    #[test]
    fn open_uri_program_uses_desktop_platform_tools() {
        assert_eq!(open_uri_program_for_target("linux"), Some("xdg-open"));
        assert_eq!(open_uri_program_for_target("macos"), Some("open"));
    }

    #[test]
    fn open_uri_program_is_absent_on_mobile_targets() {
        assert_eq!(open_uri_program_for_target("android"), None);
        assert_eq!(open_uri_program_for_target("ios"), None);
    }

    #[test]
    fn copy_programs_use_desktop_clipboard_tools() {
        assert_eq!(
            copy_programs_for_target("linux"),
            &[
                ClipboardProgram {
                    name: "wl-copy",
                    args: &[],
                },
                ClipboardProgram {
                    name: "xclip",
                    args: &["-selection", "clipboard"],
                },
            ]
        );
        assert_eq!(
            copy_programs_for_target("macos"),
            &[ClipboardProgram {
                name: "pbcopy",
                args: &[],
            }]
        );
    }

    #[test]
    fn copy_programs_are_absent_on_mobile_targets() {
        assert!(copy_programs_for_target("android").is_empty());
        assert!(copy_programs_for_target("ios").is_empty());
    }
}
