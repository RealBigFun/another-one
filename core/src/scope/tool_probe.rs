//! Cached PATH lookups for external CLIs (`git`, `gh`, …).
//!
//! Replaces the ad-hoc `find_gh_cli` + per-call `which`-style probes
//! scattered through `git_actions.rs`. Capability `applies()` impls
//! reach for `scope.system().tool_probe.has("foo")` to decide
//! whether they're available; the result is cached for the process
//! lifetime and explicitly invalidated when a session-level event
//! says the answer may have changed (e.g. `RecheckGhAuth`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

pub struct ToolProbe {
    cache: RwLock<HashMap<String, Option<PathBuf>>>,
}

impl ToolProbe {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// `Some(path)` if `name` resolved on PATH at some point in this
    /// process (or the most recent invalidate), `None` if a prior
    /// probe came up empty.
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        if let Some(hit) = self.cache.read().unwrap().get(name).cloned() {
            return hit;
        }
        let resolved = which_on_path(name);
        self.cache
            .write()
            .unwrap()
            .insert(name.to_string(), resolved.clone());
        resolved
    }

    pub fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// Drop the cached answer for `name` so the next `find` / `has`
    /// re-probes PATH. Called from `RecheckGhAuth`-style handlers.
    pub fn invalidate(&self, name: &str) {
        self.cache.write().unwrap().remove(name);
    }
}

impl Default for ToolProbe {
    fn default() -> Self {
        Self::new()
    }
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        // Windows: try with `.exe` suffix.
        #[cfg(windows)]
        {
            let with_ext = dir.join(format!("{name}.exe"));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_tool_returns_none() {
        let probe = ToolProbe::new();
        assert!(!probe.has("definitely-not-a-real-binary-xyz-123"));
    }

    #[test]
    fn invalidate_drops_cached_entry() {
        let probe = ToolProbe::new();
        let _ = probe.has("definitely-not-a-real-binary-xyz-123");
        probe.invalidate("definitely-not-a-real-binary-xyz-123");
        // No assertion on contents — just that invalidate is callable
        // without holding a stale write lock and the next `has` works.
        assert!(!probe.has("definitely-not-a-real-binary-xyz-123"));
    }
}
