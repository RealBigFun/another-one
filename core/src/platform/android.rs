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

    fn open_external_url(_url: &str) -> Result<(), String> {
        // Android opens URLs via `Intent.ACTION_VIEW`, which is
        // reachable from Java/Kotlin only. The future Flutter UI
        // will route URL opens through a Dart platform channel;
        // this Rust-side implementation exists only so the trait
        // shape is the same on every target.
        Err("open_external_url not supported from Rust on Android; use a Dart platform channel".into())
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

    #[test]
    fn open_external_url_returns_unsupported_error() {
        let result = AndroidPlatform::open_external_url("https://example.com");
        assert!(result.is_err());
        assert!(
            result.as_ref().unwrap_err().contains("Dart platform channel"),
            "expected the error to point at the Dart-side workaround, got: {:?}",
            result.unwrap_err()
        );
    }
}
