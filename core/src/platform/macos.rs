use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct MacosPlatform;

impl HeadlessPlatform for MacosPlatform {
    fn name() -> &'static str {
        "macos"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_macos() {
        assert_eq!(MacosPlatform::name(), "macos");
    }
}
