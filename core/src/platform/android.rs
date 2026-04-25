use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct AndroidPlatform;

impl HeadlessPlatform for AndroidPlatform {
    fn name() -> &'static str {
        "android"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_android() {
        assert_eq!(AndroidPlatform::name(), "android");
    }
}
