# Slint Platform Trait/Profile API

Slint platform behavior is selected through narrow Rust profiles rather than scattered view conditionals.

## Implemented Profile Surface

`slint-poc/src/platform.rs`:

```rust
pub(crate) enum SlintInputPolicy {
    DesktopKeyboard,
    TouchIme,
}

pub(crate) struct SlintPlatformProfile {
    pub(crate) target: &'static str,
    pub(crate) app_id: &'static str,
    pub(crate) mobile: bool,
    pub(crate) input_policy: SlintInputPolicy,
    pub(crate) window_label: &'static str,
}
```

## Appearance Seam

`slint-poc/src/style.rs` defines `SlintAppearanceProfile`, which resolves system light/dark mode separately from view/layout code.

## Target Profiles

| Target | App id | Mobile | Input policy | Window label |
| --- | --- | --- | --- | --- |
| Linux | `com.anotherone.Slint` | false | `DesktopKeyboard` | `desktop-window` |
| macOS | `com.anotherone.Slint` | false | `DesktopKeyboard` | `desktop-window` |
| Android | `com.anotherone.slint` | true | `TouchIme` | `android-activity` |
| iOS | `com.anotherone.slint` | true | `TouchIme` | `ios-scene` |
| Unsupported | `com.anotherone.Slint` | false | `DesktopKeyboard` | `unsupported-window` |

## Required Follow-Up

- Move shared profile definitions to the final production Slint crate once `slint-poc` is renamed.
- Add real system appearance APIs per target.
- Add platform file/open-in hooks for Slint UI actions.
- Add Android orientation/runtime proof once an `adb` device is available.
- Replace the iOS simulator library proof with an app bundle/install proof once
  the iOS shell exists.

## Build Profile Evidence

The active profile scripts are:

- Linux dev: `scripts/slint/linux-dev.sh`
- Linux release: `scripts/slint/linux-release.sh`
- macOS: `scripts/slint/macos-build.sh`
- Android APK/install: `scripts/slint/android-apk.sh`
- iOS simulator: `scripts/slint/ios-simulator-build.sh`

These scripts select Cargo targets and packaging/install tools without adding
platform branches to `slint-poc/ui/app.slint` or component/layout code.
