//! Install-side helpers for the in-app updater.
//!
//! The updater downloads + verifies a payload to a per-user cache
//! directory. Installing means swapping the running binary with
//! the verified payload and relaunching, which has to happen
//! *outside* the running process — we can't replace our own
//! AppImage / `.app` bundle while it's executing on every
//! filesystem (Linux is permissive, macOS is much stricter).
//!
//! Per-platform strategy:
//!
//! * **Linux AppImage**: spawn a detached `/bin/sh` script that
//!   waits for the current PID to exit, replaces the file at
//!   `$APPIMAGE`, sets the executable bit, and re-execs the new
//!   AppImage.
//! * **macOS .app bundle**: spawn a detached `/bin/sh` script
//!   that waits for the current PID to exit, replaces the
//!   resolved `.app` bundle with the extracted payload, and
//!   relaunches via `open`.
//!
//! When the running app isn't in a recognizable installed
//! location, both helpers fall back to opening the downloaded
//! file in Finder/the file manager so the user can install
//! manually — the plan calls for "deliberate manual-install
//! fallback" rather than guessing.

use std::path::Path;

use crate::updater::UpdateAsset;

/// Spawn a detached helper that performs the install and
/// relaunches the app. Returns once the helper has been
/// successfully spawned; the helper itself runs after this
/// process exits.
pub fn launch_install(payload: &Path, asset: &UpdateAsset) -> Result<(), String> {
    if !payload.exists() {
        return Err(format!("payload missing at {}", payload.display()));
    }

    #[cfg(target_os = "linux")]
    {
        linux::install_appimage(payload, asset)
    }

    #[cfg(target_os = "macos")]
    {
        return macos::install_app_bundle(payload, asset);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (payload, asset);
        Err("in-app installs are only supported on macOS and Linux".into())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::UpdateAsset;

    pub fn install_appimage(payload: &Path, asset: &UpdateAsset) -> Result<(), String> {
        if asset.kind != "appimage" {
            return Err(format!(
                "expected AppImage asset, got `{}` — falling back to manual install",
                asset.kind
            ));
        }

        let appimage_target = match std::env::var_os("APPIMAGE").map(std::path::PathBuf::from) {
            Some(path) if path.is_absolute() => path,
            _ => {
                // Not running from an AppImage — open the
                // download folder so the user can install by
                // hand.
                open_in_file_manager(payload);
                return Err(
                    "App is not running from an AppImage; opened the download for manual install."
                        .into(),
                );
            }
        };

        let pid = std::process::id();
        let payload_str = payload.to_string_lossy();
        let target_str = appimage_target.to_string_lossy();
        // POSIX shell is everywhere; avoids spawning bash where
        // it might not exist (Alpine, NixOS).
        let script = format!(
            r#"
              set -eu
              # Wait for the running AnotherOne process to exit
              # before we replace its on-disk image.
              while kill -0 {pid} 2>/dev/null; do sleep 0.2; done
              # Atomically swap: rename to a backup, install the
              # new file, then remove the backup once we've
              # confirmed the install succeeded.
              backup="{target}.bak"
              cp -f -- "{payload}" "{target}.new"
              chmod +x "{target}.new"
              mv -f -- "{target}" "$backup"
              mv -f -- "{target}.new" "{target}"
              rm -f -- "$backup"
              # Relaunch — exec replaces this shell with the new
              # AppImage so the user sees the new build come up.
              exec "{target}"
            "#,
            pid = pid,
            payload = payload_str.replace('"', "\\\""),
            target = target_str.replace('"', "\\\""),
        );

        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Detach from the controlling terminal so the helper
        // outlives this process group.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("spawn install helper: {err}"))
    }

    fn open_in_file_manager(path: &Path) {
        // Best-effort: any failure is logged but doesn't block.
        let _ = Command::new("xdg-open")
            .arg(path.parent().unwrap_or(path))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::UpdateAsset;

    pub fn install_app_bundle(payload: &Path, asset: &UpdateAsset) -> Result<(), String> {
        let bundle = current_app_bundle();
        let bundle = match bundle {
            Some(path) => path,
            None => {
                open_in_finder(payload);
                return Err(
                    "App is not running from a .app bundle; opened the download for manual install."
                        .into(),
                );
            }
        };

        if asset.kind != "app-tar-gz" && asset.kind != "app-zip" {
            // DMGs require user mount + drag — open and let the
            // user finish manually.
            open_in_finder(payload);
            return Err(format!(
                "Asset kind `{}` requires manual install; opened it in Finder.",
                asset.kind
            ));
        }

        let pid = std::process::id();
        let bundle_str = bundle.to_string_lossy();
        let payload_str = payload.to_string_lossy();
        let extract_script = match asset.kind.as_str() {
            "app-tar-gz" => "tar -xzf \"$payload\" -C \"$work\"".to_string(),
            "app-zip" => "ditto -x -k \"$payload\" \"$work\"".to_string(),
            _ => unreachable!("kind already filtered"),
        };

        let script = format!(
            r#"
              set -eu
              payload="{payload}"
              bundle="{bundle}"
              work="$(mktemp -d -t another-one-update)"
              trap 'rm -rf "$work"' EXIT
              {extract_script}
              # The archive contains exactly one *.app at its
              # root. Locate it instead of guessing the name.
              new_bundle="$(find "$work" -maxdepth 2 -type d -name '*.app' | head -n1)"
              if [ -z "$new_bundle" ]; then
                  echo "no .app inside payload" >&2
                  exit 1
              fi
              while kill -0 {pid} 2>/dev/null; do sleep 0.2; done
              backup="$bundle.bak"
              rm -rf -- "$backup"
              if [ -d "$bundle" ]; then
                  mv -f -- "$bundle" "$backup"
              fi
              ditto "$new_bundle" "$bundle"
              rm -rf -- "$backup"
              open "$bundle"
            "#,
            pid = pid,
            payload = payload_str.replace('"', "\\\""),
            bundle = bundle_str.replace('"', "\\\""),
            extract_script = extract_script,
        );

        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("spawn install helper: {err}"))
    }

    /// Resolve the `.app` bundle that contains the current
    /// executable, if any. Returns `None` for plain `cargo run`
    /// builds and for any layout where the binary isn't nested
    /// inside an `*.app/Contents/MacOS/` path.
    fn current_app_bundle() -> Option<std::path::PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let mut current = exe.as_path();
        while let Some(parent) = current.parent() {
            if parent
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("app"))
                .unwrap_or(false)
            {
                return Some(parent.to_path_buf());
            }
            current = parent;
        }
        None
    }

    fn open_in_finder(path: &Path) {
        let _ = Command::new("open")
            .arg("-R")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}
