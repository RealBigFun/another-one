use super::HeadlessPlatform;

pub struct AndroidPlatform;

impl HeadlessPlatform for AndroidPlatform {
    fn name() -> &'static str {
        "android"
    }
}
