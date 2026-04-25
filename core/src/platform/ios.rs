use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct IosPlatform;

impl HeadlessPlatform for IosPlatform {
    fn name() -> &'static str {
        "ios"
    }

    fn modifier_label() -> &'static str {
        // Hardware keyboards on iOS report the Cmd key as the
        // primary modifier (matching macOS), and the iPad's
        // Magic Keyboard glyph row mirrors the macOS layout.
        "Cmd"
    }

    fn open_external_url(_url: &str) -> Result<(), String> {
        // iOS sandboxes opening URLs behind `UIApplication.open(_:)`,
        // which is reachable from Swift/Objective-C only. The future
        // Flutter UI will route URL opens through a Dart platform
        // channel; this Rust-side implementation exists only so the
        // trait shape is the same on every target.
        Err("open_external_url not supported from Rust on iOS; use a Dart platform channel".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_ios() {
        assert_eq!(IosPlatform::name(), "ios");
    }

    #[test]
    fn modifier_label_returns_cmd() {
        assert_eq!(IosPlatform::modifier_label(), "Cmd");
    }

    #[test]
    fn open_external_url_returns_unsupported_error() {
        let result = IosPlatform::open_external_url("https://example.com");
        assert!(result.is_err());
        assert!(
            result.as_ref().unwrap_err().contains("Dart platform channel"),
            "expected the error to point at the Dart-side workaround, got: {:?}",
            result.unwrap_err()
        );
    }
}
