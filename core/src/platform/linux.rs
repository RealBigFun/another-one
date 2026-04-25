use super::HeadlessPlatform;

pub struct LinuxPlatform;

impl HeadlessPlatform for LinuxPlatform {
    fn name() -> &'static str {
        "linux"
    }
}
