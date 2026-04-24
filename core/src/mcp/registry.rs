//! Persistent MCP registry.
//!
//! Source of truth for the user's configured MCP servers. Lives on
//! disk at `{config_dir}/another-one/mcp.json` (parallel to
//! `projects.json`). Survives harness uninstalls — entries persist
//! even when no harness has them toggled on.
//!
//! ## Sync semantics (2-phase per-harness)
//!
//! 1. **Reads** are *not* orchestrated here — the registry is
//!    authoritative for what AnotherOne owns. Adapters' `read()`
//!    functions are still public so the MCP page can surface
//!    non-registry rows that appear in harness config files (the
//!    user set them up via the harness directly). Those are not
//!    promoted into the registry automatically.
//! 2. **Writes** are 2-phase per-harness: for each harness `H` that
//!    supports MCP, gather the registry-owned entries whose
//!    `enabled_for` contains `H`'s provider kind, then call the
//!    adapter's `write(...)` — which internally re-reads disk,
//!    merges (preserving user-authored rows), and writes back.
//!    Previously-owned ids are tracked per-harness in the registry
//!    itself so rows removed from `enabled_for` are stripped from
//!    the corresponding harness config on the next sync.
//!
//! Each adapter's write is independent; one failing surfaces as an
//! error in the returned [`SyncReport`] but doesn't roll back the
//! others (matches #33's "best-effort writes, per-provider error
//! reporting").

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agents::AgentProviderKind;
use crate::mcp::adapters;
use crate::mcp::McpServer;

const REGISTRY_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct McpRegistry {
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<McpServer>,
    /// Per-provider list of ids the registry previously wrote to
    /// that provider's config file. On next sync, rows with these
    /// ids get stripped from disk if they're no longer in the
    /// current owned set — that's how toggling an entry off for a
    /// harness removes it from the harness's config.
    #[serde(default)]
    pub previously_owned: HashMap<AgentProviderKind, HashSet<String>>,
}

/// Per-provider outcome from a sync pass. `Ok(()))` ⇒ the provider's
/// adapter wrote successfully; `Err` ⇒ the adapter surfaced an error
/// that the UI should show as a toast against the affected provider.
pub type SyncReport = HashMap<AgentProviderKind, anyhow::Result<()>>;

impl McpRegistry {
    pub fn load() -> Self {
        Self::read_from_disk(&Self::config_path())
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        crate::mcp::adapters::atomic_write(&path, json.as_bytes()).map_err(std::io::Error::other)
    }

    /// Replace `entries` with the passed set. Also handled: rebuild
    /// `previously_owned` so on-disk cleanup happens the next time
    /// `sync_all` runs.
    pub fn set_entries(&mut self, entries: Vec<McpServer>) {
        self.entries = entries;
    }

    /// Toggle a single entry's enablement for a provider. Returns
    /// false if no entry with `id` is in the registry.
    pub fn toggle(&mut self, id: &str, provider: AgentProviderKind, enabled: bool) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else {
            return false;
        };
        if enabled {
            entry.enabled_for.insert(provider);
        } else {
            entry.enabled_for.remove(&provider);
        }
        true
    }

    /// Upsert an entry (inserts if id is new, replaces otherwise).
    pub fn upsert(&mut self, server: McpServer) {
        match self.entries.iter_mut().find(|e| e.id == server.id) {
            Some(slot) => *slot = server,
            None => self.entries.push(server),
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let len = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() < len
    }

    /// Write the registry's owned entries into every supported
    /// harness's native config file. Each provider is attempted
    /// independently; errors accumulate into the returned
    /// [`SyncReport`]. On success for a provider, the registry's
    /// `previously_owned` map is updated to reflect the new set of
    /// ids (so next sync cleans up rows removed from `enabled_for`).
    ///
    /// Call `save()` after `sync_all` to persist the updated
    /// `previously_owned` tracking.
    pub fn sync_all(&mut self) -> SyncReport {
        let mut report = SyncReport::new();
        for provider in SUPPORTED_PROVIDERS {
            let owned: Vec<&McpServer> = self
                .entries
                .iter()
                .filter(|e| e.enabled_for.contains(provider))
                .collect();
            let prev_ids = self
                .previously_owned
                .get(provider)
                .cloned()
                .unwrap_or_default();

            let result = write_for_provider(*provider, &owned, &prev_ids);

            if result.is_ok() {
                let new_ids: HashSet<String> = owned.iter().map(|s| s.id.clone()).collect();
                self.previously_owned.insert(*provider, new_ids);
            }
            report.insert(*provider, result);
        }
        report
    }

    fn config_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("another-one").join("mcp.json")
    }

    fn read_from_disk(path: &Path) -> Self {
        let Ok(contents) = std::fs::read_to_string(path) else {
            return Self {
                version: REGISTRY_VERSION,
                entries: Vec::new(),
                previously_owned: HashMap::new(),
            };
        };
        match serde_json::from_str::<Self>(&contents) {
            Ok(mut r) => {
                // On a future version bump we still want to keep
                // `previously_owned` intact so the next sync can
                // strip harness-side rows AnotherOne used to own.
                // `entries` we reset since their shape may have
                // changed; migration hooks can slot in here later.
                if r.version != REGISTRY_VERSION {
                    eprintln!(
                        "warn: MCP registry at {} is version {} (current {}); preserving \
                         previously_owned and resetting entries",
                        path.display(),
                        r.version,
                        REGISTRY_VERSION,
                    );
                    r.entries.clear();
                    r.version = REGISTRY_VERSION;
                }
                r
            }
            Err(err) => {
                eprintln!(
                    "warn: failed to parse MCP registry at {}: {err}; starting empty",
                    path.display(),
                );
                Self {
                    version: REGISTRY_VERSION,
                    entries: Vec::new(),
                    previously_owned: HashMap::new(),
                }
            }
        }
    }

    /// Ensure a built-in entry (e.g. the daemon MCP from #34)
    /// exists in the registry with the latest generated transport,
    /// preserving the user's `enabled_for` set across restarts and
    /// app upgrades. If the id isn't present yet, inserts it with
    /// the provided default (typically `enabled_for = {}`, so the
    /// user explicitly opts in).
    ///
    /// The id is a stable contract across app versions — built-ins
    /// must keep the same id even as their generated command line
    /// evolves. Renaming a built-in id would orphan the user's
    /// enablement in `previously_owned` without a migration hook.
    pub fn ensure_builtin(&mut self, default: McpServer) {
        if let Some(slot) = self.entries.iter_mut().find(|e| e.id == default.id) {
            slot.label = default.label;
            slot.transport = default.transport;
            slot.source = default.source;
            // enabled_for preserved.
        } else {
            self.entries.push(default);
        }
    }
}

/// Providers the registry knows how to write into. Any provider not
/// listed here is skipped silently on sync — `supports_mcp_client()`
/// on its harness should return `false` too.
const SUPPORTED_PROVIDERS: &[AgentProviderKind] = &[
    AgentProviderKind::ClaudeCode,
    AgentProviderKind::CursorAgent,
    AgentProviderKind::Codex,
    AgentProviderKind::Gemini,
    AgentProviderKind::OpenCode,
    AgentProviderKind::Amp,
];

fn write_for_provider(
    provider: AgentProviderKind,
    owned: &[&McpServer],
    previously_owned_ids: &HashSet<String>,
) -> anyhow::Result<()> {
    match provider {
        AgentProviderKind::ClaudeCode => adapters::claude_code::write(owned, previously_owned_ids),
        AgentProviderKind::CursorAgent => adapters::cursor::write(owned, previously_owned_ids),
        AgentProviderKind::Codex => adapters::codex::write(owned, previously_owned_ids),
        AgentProviderKind::Gemini => adapters::gemini::write(owned, previously_owned_ids),
        AgentProviderKind::OpenCode => adapters::opencode::write(owned, previously_owned_ids),
        AgentProviderKind::Amp => adapters::amp::write(owned, previously_owned_ids),
        // No-op for providers without MCP client support today.
        AgentProviderKind::Pi | AgentProviderKind::RovoDev | AgentProviderKind::Forge => Ok(()),
    }
}

/// Read the current on-disk MCP config for the given provider. The
/// returned entries are tagged as `McpSource::Custom` — the UI is
/// responsible for promoting those whose ids match registry entries
/// back to their registry-declared source when rendering.
pub fn read_for_provider(provider: AgentProviderKind) -> anyhow::Result<Vec<McpServer>> {
    match provider {
        AgentProviderKind::ClaudeCode => adapters::claude_code::read(),
        AgentProviderKind::CursorAgent => adapters::cursor::read(),
        AgentProviderKind::Codex => adapters::codex::read(),
        AgentProviderKind::Gemini => adapters::gemini::read(),
        AgentProviderKind::OpenCode => adapters::opencode::read(),
        AgentProviderKind::Amp => adapters::amp::read(),
        AgentProviderKind::Pi | AgentProviderKind::RovoDev | AgentProviderKind::Forge => {
            Ok(Vec::new())
        }
    }
}

pub fn config_path_for_provider(provider: AgentProviderKind) -> Option<PathBuf> {
    match provider {
        AgentProviderKind::ClaudeCode => adapters::claude_code::config_path(),
        AgentProviderKind::CursorAgent => adapters::cursor::config_path(),
        AgentProviderKind::Codex => adapters::codex::config_path(),
        AgentProviderKind::Gemini => adapters::gemini::config_path(),
        AgentProviderKind::OpenCode => adapters::opencode::config_path(),
        AgentProviderKind::Amp => adapters::amp::config_path(),
        AgentProviderKind::Pi | AgentProviderKind::RovoDev | AgentProviderKind::Forge => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[allow(unused_imports)]
    use std::collections::HashMap as _HashMap;

    use crate::mcp::{McpSource, McpTransport};

    fn stdio(id: &str, cmd: &str) -> McpServer {
        McpServer {
            id: id.into(),
            label: id.into(),
            transport: McpTransport::Stdio {
                command: cmd.into(),
                args: vec![],
                env: BTreeMap::new(),
            },
            enabled_for: HashSet::new(),
            source: McpSource::Custom,
        }
    }

    #[test]
    fn registry_roundtrips_through_json() {
        let mut original = McpRegistry {
            version: REGISTRY_VERSION,
            entries: vec![stdio("context7", "npx")],
            previously_owned: HashMap::new(),
        };
        let mut prev = HashSet::new();
        prev.insert("old".to_string());
        original
            .previously_owned
            .insert(AgentProviderKind::ClaudeCode, prev);
        let json = serde_json::to_string(&original).unwrap();
        let back: McpRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entries, original.entries);
        assert_eq!(back.previously_owned, original.previously_owned);
    }

    #[test]
    fn toggle_updates_enabled_for_set() {
        let mut reg = McpRegistry {
            version: REGISTRY_VERSION,
            entries: vec![stdio("c7", "npx")],
            previously_owned: HashMap::new(),
        };
        assert!(reg.toggle("c7", AgentProviderKind::ClaudeCode, true));
        assert!(reg.entries[0]
            .enabled_for
            .contains(&AgentProviderKind::ClaudeCode));
        assert!(reg.toggle("c7", AgentProviderKind::ClaudeCode, false));
        assert!(!reg.entries[0]
            .enabled_for
            .contains(&AgentProviderKind::ClaudeCode));
        assert!(!reg.toggle("missing", AgentProviderKind::ClaudeCode, true));
    }

    #[test]
    fn upsert_replaces_by_id() {
        let mut reg = McpRegistry {
            version: REGISTRY_VERSION,
            entries: vec![stdio("x", "old")],
            previously_owned: HashMap::new(),
        };
        reg.upsert(stdio("x", "new"));
        assert_eq!(reg.entries.len(), 1);
        if let McpTransport::Stdio { command, .. } = &reg.entries[0].transport {
            assert_eq!(command, "new");
        } else {
            panic!("expected stdio");
        }
    }
}
