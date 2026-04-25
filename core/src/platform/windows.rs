use std::process::Command;

use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsPlatform;

impl HeadlessPlatform for WindowsPlatform {
    fn name() -> &'static str {
        "windows"
    }

    fn modifier_label() -> &'static str {
        "Win"
    }

    fn open_external_url(url: &str) -> Result<(), String> {
        // The empty string after `start` is the window-title
        // placeholder — without it `start` would interpret a
        // quoted URL itself as the title.
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("Could not open URL externally: {err}"))
    }

    fn total_system_memory_bytes() -> Option<u64> {
        // Matches the existing GPUI desktop behaviour: no Windows
        // implementation yet. Wiring `GlobalMemoryStatusEx` from
        // `windows-sys` is straightforward but pulls in the
        // `windows-sys` dep; defer until someone needs the value
        // on Windows. The resource indicator UI hides the total
        // when this is `None`.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_windows() {
        assert_eq!(WindowsPlatform::name(), "windows");
    }

    #[test]
    fn modifier_label_returns_win() {
        assert_eq!(WindowsPlatform::modifier_label(), "Win");
    }

    #[test]
    fn total_system_memory_bytes_returns_none() {
        assert!(WindowsPlatform::total_system_memory_bytes().is_none());
    }
}
