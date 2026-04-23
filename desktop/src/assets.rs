//! Custom asset source that loads from the project root at runtime.

use std::borrow::Cow;
use std::path::PathBuf;

use gpui::SharedString;

pub struct ProjectAssets {
    pub root: PathBuf,
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
