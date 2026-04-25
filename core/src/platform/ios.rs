use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct IosPlatform;

impl HeadlessPlatform for IosPlatform {
    fn name() -> &'static str {
        "ios"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_ios() {
        assert_eq!(IosPlatform::name(), "ios");
    }
}
