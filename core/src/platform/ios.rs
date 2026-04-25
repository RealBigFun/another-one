use super::HeadlessPlatform;

pub struct IosPlatform;

impl HeadlessPlatform for IosPlatform {
    fn name() -> &'static str {
        "ios"
    }
}
