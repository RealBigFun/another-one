use super::HeadlessPlatform;

pub struct WindowsPlatform;

impl HeadlessPlatform for WindowsPlatform {
    fn name() -> &'static str {
        "windows"
    }
}
