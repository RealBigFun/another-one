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
}
