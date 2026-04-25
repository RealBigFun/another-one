use super::HeadlessPlatform;

pub struct MacosPlatform;

impl HeadlessPlatform for MacosPlatform {
    fn name() -> &'static str {
        "macos"
    }
}
