# GPUI Platform Integration Inventory

This inventory records the GPUI platform behavior Slint must preserve or replace with explicit platform profiles.

## GPUI / Shared Rust Sources

| Facet | Current source | Slint owner |
| --- | --- | --- |
| App/window identity | `desktop/src/platform/*`, GPUI window setup | `SlintPlatformProfile.app_id`, package metadata |
| Linux/macOS custom chrome | `desktop/src/titlebar.rs`, `desktop/src/layout.rs` | Slint shell profile and app layout |
| Resource sampling | `core/src/platform/*`, `desktop/src/resource_usage.rs` | shared core platform/process APIs |
| Open-In app detection | `core/src/platform/*`, `desktop/src/open_in.rs` | shared core platform APIs plus Slint UI actions |
| Keyboard modifier labels | `core/src/platform/mod.rs`, shortcuts/settings UI | Slint input profile |
| Terminal input policy | GPUI event handling plus terminal runtime | Slint input profile and daemon `Control::TabInput` |
| Toast/error surfaces | `desktop/src/app.rs` | Slint toast component and app-level toast route |
| Project folder picker | GPUI platform services | Slint platform profile `folder_picker`; Linux uses XDG Desktop Portal FileChooser, macOS uses native panel |
| Android/iOS packaging | not GPUI-owned | Slint build/profile metadata |
| System appearance | GPUI `WindowAppearance` | Slint appearance profile |

## Current Slint Implementation

`slint-poc/src/platform.rs` defines:

- `SlintPlatformProfile`
- `SlintInputPolicy`
- `current_platform_profile()`

Profiles currently cover Linux, macOS, Android, iOS, and explicit unsupported fallback. The Slint app consumes the profile for XDG app id, platform label, and input policy documentation.

## Production Rules

- View files must not branch directly on target OS.
- Build scripts may select packages/targets, but product behavior should come from profiles.
- Android/iOS profile gaps must stay visible in this document until tested on-device.
- Windows is unsupported for Slint productization unless a future decision changes that stance.
