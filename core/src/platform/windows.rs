use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsPlatform;

impl HeadlessPlatform for WindowsPlatform {
    fn name() -> &'static str {
        "windows"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_windows() {
        assert_eq!(WindowsPlatform::name(), "windows");
    }
}
