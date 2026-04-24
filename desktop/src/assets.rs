//! Custom asset source that loads from the project or app bundle at runtime.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use gpui::SharedString;

pub struct ProjectAssets {
    pub root: PathBuf,
}

pub fn asset_root() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(root) = macos_bundle_resource_root() {
        return root;
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

impl gpui::AssetSource for ProjectAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
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
