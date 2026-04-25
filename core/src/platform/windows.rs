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
