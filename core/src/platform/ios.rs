use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct IosPlatform;

impl HeadlessPlatform for IosPlatform {
    fn name() -> &'static str {
        "ios"
    }

    fn modifier_label() -> &'static str {
        // Hardware keyboards on iOS report the Cmd key as the
        // primary modifier (matching macOS), and the iPad's
        // Magic Keyboard glyph row mirrors the macOS layout.
        "Cmd"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_ios() {
        assert_eq!(IosPlatform::name(), "ios");
    }

    #[test]
    fn modifier_label_returns_cmd() {
        assert_eq!(IosPlatform::modifier_label(), "Cmd");
    }
}
