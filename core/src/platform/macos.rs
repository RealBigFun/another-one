use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct MacosPlatform;

impl HeadlessPlatform for MacosPlatform {
    fn name() -> &'static str {
        "macos"
    }

    fn modifier_label() -> &'static str {
        "Cmd"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_macos() {
        assert_eq!(MacosPlatform::name(), "macos");
    }

    #[test]
    fn modifier_label_returns_cmd() {
        assert_eq!(MacosPlatform::modifier_label(), "Cmd");
    }
}
