//! Cached PATH lookups for external CLIs (`git`, `gh`, …).
//!
//! Delegates discovery to `crate::command_env::find_executable` — the
//! same lookup the actual shell-out helpers (`find_gh_cli`,
//! `git_command`) use. Caches results for the process lifetime and
//! exposes `invalidate` for session-level revalidation events (e.g.
//! `Control::RecheckGhAuth` after the user installs `gh`).
//!
//! Why delegate instead of walking `$PATH` ourselves: on macOS GUI
//! launches inherit a minimal launchd PATH that omits Homebrew, so a
//! naïve `which`-style probe would miss `gh` even when the actual
//! shell-out succeeds via the login-shell-initialized PATH + the
//! `/opt/homebrew/bin` fallback list. Capability `applies()` MUST
//! agree with the shell-out's discovery or the registry will hide
//! providers that would otherwise work.

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

    /// `Some(path)` if `name` resolved on PATH (or one of its known
    /// fallbacks) at some point in this process; `None` if a prior
    /// probe came up empty. Delegates to
    /// `command_env::find_executable` so the answer matches what
    /// the eventual shell-out will see — see module docs.
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        if let Some(hit) = self.cache.read().unwrap().get(name).cloned() {
            return hit;
        }
        let cwd = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let resolved =
            crate::command_env::find_executable(name, &cwd, &fallbacks_for(name));
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

/// Per-tool fallback paths matched against the canonical install
/// locations the existing `find_*_cli` helpers in `git_actions.rs`
/// use. Kept here (not on the impls) so a `ToolProbe.find("gh")`
/// call surfaces the same answer regardless of caller.
fn fallbacks_for(name: &str) -> Vec<PathBuf> {
    match name {
        "gh" => vec![
            PathBuf::from("/opt/homebrew/bin/gh"),
            PathBuf::from("/usr/local/bin/gh"),
        ],
        _ => Vec::new(),
    }
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
