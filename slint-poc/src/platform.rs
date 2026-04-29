use std::io::Write;
use std::path::PathBuf;
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
    pub(crate) folder_picker: bool,
}

impl SlintPlatformProfile {
    pub(crate) fn label(self) -> String {
        format!("{} / {}", self.target, self.input_policy.label())
    }
}

pub(crate) fn current_platform_profile() -> SlintPlatformProfile {
    let mut profile = current_platform_profile_for_target(std::env::consts::OS);
    profile.folder_picker = effective_folder_picker_available_for_target(profile.target);
    profile
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

pub(crate) fn choose_project_folder() -> Result<Option<PathBuf>, String> {
    choose_project_folder_for_target(std::env::consts::OS)
}

#[cfg(target_os = "linux")]
fn choose_project_folder_for_target(target_os: &str) -> Result<Option<PathBuf>, String> {
    if target_os != "linux" || !effective_folder_picker_available_for_target(target_os) {
        return Err(unsupported_folder_picker_message(target_os));
    }

    choose_project_folder_with_xdg_portal()
}

#[cfg(target_os = "macos")]
fn choose_project_folder_for_target(target_os: &str) -> Result<Option<PathBuf>, String> {
    if target_os != "macos" || !effective_folder_picker_available_for_target(target_os) {
        return Err(unsupported_folder_picker_message(target_os));
    }

    Ok(rfd::FileDialog::new()
        .set_title("Add Project Folder")
        .pick_folder())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn choose_project_folder_for_target(target_os: &str) -> Result<Option<PathBuf>, String> {
    Err(unsupported_folder_picker_message(target_os))
}

#[cfg(target_os = "linux")]
fn choose_project_folder_with_xdg_portal() -> Result<Option<PathBuf>, String> {
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    let connection = zbus::blocking::Connection::session()
        .map_err(|error| format!("platform folder picker session bus failed: {error}"))?;
    let unique_name = connection
        .unique_name()
        .ok_or_else(|| "platform folder picker session bus has no unique name".to_string())?;
    let handle_token = xdg_portal_handle_token();
    let sender = unique_name
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    let request_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{handle_token}");

    let request_proxy = xdg_portal_request_proxy(&connection, &request_path)?;
    let mut response_stream = request_proxy
        .receive_signal("Response")
        .map_err(|error| format!("platform folder picker response listener failed: {error}"))?;
    let file_chooser = xdg_portal_file_chooser_proxy(&connection)?;

    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("handle_token", Value::from(handle_token.as_str()));
    options.insert("directory", Value::from(true));
    options.insert("modal", Value::from(true));
    options.insert("multiple", Value::from(false));
    options.insert("accept_label", Value::from("Add"));

    let returned_path: OwnedObjectPath = file_chooser
        .call("OpenFile", &("", "Add Project Folder", options))
        .map_err(|error| format!("platform folder picker failed: {error}"))?;

    let returned_request_proxy;
    if returned_path.as_str() != request_path {
        returned_request_proxy = Some(xdg_portal_request_proxy(
            &connection,
            returned_path.as_str(),
        )?);
        response_stream = returned_request_proxy
            .as_ref()
            .expect("returned request proxy was just set")
            .receive_signal("Response")
            .map_err(|error| format!("platform folder picker response listener failed: {error}"))?;
    }

    let response = response_stream
        .next()
        .ok_or_else(|| "platform folder picker closed without a response".to_string())?;
    let (code, mut results): (u32, HashMap<String, OwnedValue>) = response
        .body()
        .deserialize()
        .map_err(|error| format!("platform folder picker response was invalid: {error}"))?;

    match code {
        0 => {}
        1 => return Ok(None),
        _ => return Err("platform folder picker failed".to_string()),
    }

    let Some(uris_value) = results.remove("uris") else {
        return Ok(None);
    };
    let uris = Vec::<String>::try_from(uris_value).map_err(|error| {
        format!("platform folder picker returned invalid folder URI list: {error}")
    })?;
    let Some(uri) = uris.first() else {
        return Ok(None);
    };

    file_uri_to_path(uri)
}

#[cfg(target_os = "linux")]
fn xdg_portal_request_proxy<'a>(
    connection: &'a zbus::blocking::Connection,
    request_path: &'a str,
) -> Result<zbus::blocking::Proxy<'a>, String> {
    zbus::blocking::Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        request_path,
        "org.freedesktop.portal.Request",
    )
    .map_err(|error| format!("platform folder picker request failed: {error}"))
}

#[cfg(target_os = "linux")]
fn xdg_portal_file_chooser_proxy(
    connection: &zbus::blocking::Connection,
) -> Result<zbus::blocking::Proxy<'_>, String> {
    zbus::blocking::Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.FileChooser",
    )
    .map_err(|error| format!("platform folder picker is unavailable: {error}"))
}

#[cfg(target_os = "linux")]
fn xdg_portal_handle_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("anotherone_{nanos}")
}

#[cfg(target_os = "linux")]
fn file_uri_to_path(uri: &str) -> Result<Option<PathBuf>, String> {
    let Some(encoded_path) = uri.strip_prefix("file://") else {
        return Err(format!(
            "platform folder picker returned a non-file URI: {uri}"
        ));
    };
    let decoded = percent_encoding::percent_decode_str(encoded_path)
        .decode_utf8()
        .map_err(|error| format!("platform folder picker returned invalid URI text: {error}"))?;
    Ok(Some(PathBuf::from(decoded.as_ref())))
}

fn unsupported_folder_picker_message(target_os: &str) -> String {
    format!("the platform folder picker is not available on {target_os}")
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
            folder_picker: native_folder_picker_available_for_target("android"),
        },
        "ios" => SlintPlatformProfile {
            target: "ios",
            app_id: "com.anotherone.slint",
            mobile: true,
            input_policy: SlintInputPolicy::TouchIme,
            window_label: "ios-scene",
            folder_picker: native_folder_picker_available_for_target("ios"),
        },
        "macos" => SlintPlatformProfile {
            target: "macos",
            app_id: "com.anotherone.Slint",
            mobile: false,
            input_policy: SlintInputPolicy::DesktopKeyboard,
            window_label: "desktop-window",
            folder_picker: native_folder_picker_available_for_target("macos"),
        },
        "linux" => SlintPlatformProfile {
            target: "linux",
            app_id: "com.anotherone.Slint",
            mobile: false,
            input_policy: SlintInputPolicy::DesktopKeyboard,
            window_label: "desktop-window",
            folder_picker: native_folder_picker_available_for_target("linux"),
        },
        _ => SlintPlatformProfile {
            target: "unsupported",
            app_id: "com.anotherone.Slint",
            mobile: false,
            input_policy: SlintInputPolicy::DesktopKeyboard,
            window_label: "unsupported-window",
            folder_picker: native_folder_picker_available_for_target("unsupported"),
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

fn native_folder_picker_available_for_target(target_os: &str) -> bool {
    matches!(target_os, "linux" | "macos")
}

fn effective_folder_picker_available_for_target(target_os: &str) -> bool {
    match target_os {
        "linux" => linux_file_chooser_portal_available(),
        "macos" => true,
        _ => false,
    }
}

#[cfg(target_os = "linux")]
fn linux_file_chooser_portal_available() -> bool {
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let Ok(proxy) = xdg_portal_file_chooser_proxy(&connection) else {
        return false;
    };
    proxy.get_property::<u32>("version").is_ok()
}

#[cfg(not(target_os = "linux"))]
fn linux_file_chooser_portal_available() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_profile_uses_desktop_keyboard_policy() {
        let profile = current_platform_profile_for_target("linux");
        assert_eq!(profile.input_policy, SlintInputPolicy::DesktopKeyboard);
        assert!(!profile.mobile);
        assert!(profile.folder_picker);
    }

    #[test]
    fn android_profile_uses_touch_ime_policy() {
        let profile = current_platform_profile_for_target("android");
        assert_eq!(profile.input_policy, SlintInputPolicy::TouchIme);
        assert!(profile.mobile);
        assert!(!profile.folder_picker);
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

    #[test]
    fn folder_picker_uses_native_desktop_platform_picker() {
        assert!(native_folder_picker_available_for_target("linux"));
        assert!(native_folder_picker_available_for_target("macos"));
    }

    #[test]
    fn folder_picker_is_unavailable_on_mobile_targets() {
        assert!(!native_folder_picker_available_for_target("android"));
        assert!(!native_folder_picker_available_for_target("ios"));
        assert!(!effective_folder_picker_available_for_target("android"));
        assert!(!effective_folder_picker_available_for_target("ios"));
        assert_eq!(
            unsupported_folder_picker_message("android"),
            "the platform folder picker is not available on android"
        );
    }
}
