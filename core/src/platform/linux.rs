use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxPlatform;

impl HeadlessPlatform for LinuxPlatform {
    fn name() -> &'static str {
        "linux"
    }

    fn modifier_label() -> &'static str {
        "Super"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_linux() {
        assert_eq!(LinuxPlatform::name(), "linux");
    }

    #[test]
    fn modifier_label_returns_super() {
        assert_eq!(LinuxPlatform::modifier_label(), "Super");
    }
}
