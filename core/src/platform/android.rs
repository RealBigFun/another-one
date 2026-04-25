use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct AndroidPlatform;

impl HeadlessPlatform for AndroidPlatform {
    fn name() -> &'static str {
        "android"
    }

    fn modifier_label() -> &'static str {
        // Android external keyboards (Bluetooth, USB) almost
        // universally ship Ctrl as the primary modifier — Search
        // is technically the Android-flavoured super-key but few
        // physical keyboards expose it. Pragmatically: "Ctrl"
        // matches what users see in app shortcut hints.
        "Ctrl"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_android() {
        assert_eq!(AndroidPlatform::name(), "android");
    }

    #[test]
    fn modifier_label_returns_ctrl() {
        assert_eq!(AndroidPlatform::modifier_label(), "Ctrl");
    }
}
