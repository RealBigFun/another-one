//! Asset source backed by the crate's `assets/` directory.
//!
//! Every asset is embedded at compile time via `include_dir!` so the
//! same bytes are available on every platform without depending on
//! OS-specific bundle layouts (macOS `Contents/Resources`, Linux
//! AppImage `$APPDIR`, Android APK internals, etc.). The asset
//! resolver is now pure — no filesystem probing, no per-OS shims,
//! no per-icon allowlist that has to be updated every time a new
//! SVG is referenced from the UI. Add a file under `app/assets/`
//! and it's immediately loadable from every target.
//!

use std::borrow::Cow;
#[cfg(target_os = "macos")]
use std::path::PathBuf;

use gpui::SharedString;
use include_dir::{include_dir, Dir};

static EMBEDDED_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets");

pub struct ProjectAssets;

#[cfg(target_os = "macos")]
pub fn asset_root() -> PathBuf {
    // Reports the source-of-truth path the embed snapshotted. Kept
    // for logs / diagnostics; the runtime loader itself never
    // consults the filesystem.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

impl gpui::AssetSource for ProjectAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        // Callers pass paths like `"assets/icons/icons__close.svg"`;
        // the embedded dir is rooted at `assets/`, so strip the
        // leading segment if present and look the rest up.
        let relative = path.strip_prefix("assets/").unwrap_or(path);
        Ok(EMBEDDED_ASSETS
            .get_file(relative)
            .map(|f| Cow::Borrowed(f.contents())))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let relative = path.strip_prefix("assets/").unwrap_or(path);
        let dir = if relative.is_empty() {
            Some(&EMBEDDED_ASSETS)
        } else {
            EMBEDDED_ASSETS.get_dir(relative)
        };
        Ok(dir
            .map(|d| {
                d.files()
                    .map(|f| SharedString::from(format!("assets/{}", f.path().display())))
                    .collect()
            })
            .unwrap_or_default())
    }
}
