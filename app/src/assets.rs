//! Custom asset source that loads from the project or app bundle at runtime.

use std::borrow::Cow;
#[cfg(target_os = "macos")]
use std::path::Path;
use std::path::PathBuf;

use gpui::SharedString;

pub struct ProjectAssets {
    pub root: PathBuf,
}

pub fn asset_root() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(root) = macos_bundle_resource_root() {
        return root;
    }

    #[cfg(target_os = "linux")]
    if let Some(root) = linux_appimage_resource_root() {
        return root;
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Icons that have to render on Android too. Android APKs don't
/// expose `app/assets/` through the filesystem, so the disk lookup
/// in `load` returns `NotFound` and `svg()` paints empty. Embedding
/// the bytes mirrors what `lib.rs` already does for the bundled
/// Lilex font — small binary cost, zero runtime asset wiring.
///
/// Add a path here only when its widget needs to render on every
/// platform; the rest still resolve via the on-disk asset root.
const EMBEDDED_ICONS: &[(&str, &[u8])] = &[
    (
        "assets/sidebar_toggle.svg",
        include_bytes!("../assets/sidebar_toggle.svg"),
    ),
    (
        "assets/right_sidebar_toggle.svg",
        include_bytes!("../assets/right_sidebar_toggle.svg"),
    ),
];

impl gpui::AssetSource for ProjectAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = EMBEDDED_ICONS
            .iter()
            .find_map(|(p, b)| (*p == path).then_some(*b))
        {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        let full = self.root.join(path);
        match std::fs::read(&full) {
            Ok(bytes) => Ok(Some(Cow::Owned(bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(anyhow::anyhow!(
                "failed to read asset {}: {}",
                full.display(),
                e
            )),
        }
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

#[cfg(target_os = "macos")]
fn macos_bundle_resource_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let contents_dir = exe.parent()?.parent()?;
    if contents_dir.file_name()? != "Contents" {
        return None;
    }

    let resources_dir = contents_dir.join("Resources");
    has_bundled_assets(&resources_dir).then_some(resources_dir)
}

#[cfg(target_os = "macos")]
fn has_bundled_assets(resources_dir: &Path) -> bool {
    resources_dir.join("assets").is_dir()
}

/// AppImage runtime: `linuxdeploy`'s `AppRun` exports `APPDIR`
/// pointing at the squashfs mount root. The packaging script lays
/// assets under `$APPDIR/usr/share/another-one/assets/`, matching
/// the XDG-ish convention `linuxdeploy` already uses for icons and
/// .desktop files.
#[cfg(target_os = "linux")]
fn linux_appimage_resource_root() -> Option<PathBuf> {
    let appdir = std::env::var_os("APPDIR")?;
    let resources = PathBuf::from(appdir).join("usr/share/another-one");
    resources.join("assets").is_dir().then_some(resources)
}
