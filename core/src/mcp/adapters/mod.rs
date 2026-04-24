//! Per-harness MCP config adapters.
//!
//! The registry owns a canonical [`McpServer`] set. Each harness
//! stores MCP entries in its own native format — different paths,
//! different JSON keys, TOML for Codex, stdio-only for Codex, etc.
//! Adapters bridge the canonical shape to the on-disk shape:
//!
//!   registry ──forward──► native file
//!   registry ◄──reverse── native file
//!
//! ## Ownership partition
//!
//! Harness config files may contain entries the user configured
//! outside AnotherOne. Adapters must preserve those untouched.
//! The registry's `McpServer::id` is the partition key — adapters
//! only add/remove/update rows whose ids are in the registry's
//! owned set.
//!
//! ## Format translation nuances (ported from emdash)
//!
//! - **passthrough** (ClaudeCode, Amp): canonical JSON shape, no
//!   translation. `{ "type": "http", "url": "…" }` or
//!   `{ "command": "…", "args": [...] }`.
//! - **cursor**: HTTP entries carry `url` directly (no `type: http`
//!   tag on write; the reverse pass re-tags so the registry sees
//!   them as HTTP).
//! - **codex**: stdio-only; HTTP entries in the registry are
//!   filtered out on write. TOML, with `toml_edit` preserving
//!   user-authored comments and row order.
//! - **gemini**: HTTP entries use key `httpUrl` instead of `url`
//!   and auto-inject an `Accept` header.
//! - **opencode**: wraps entries in `{ "type": "local" | "remote",
//!   "enabled": true, ... }`; stdio command is an array rather
//!   than `command` + `args`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Context};
use serde_json::{Map, Value};

use crate::mcp::{McpServer, McpSource, McpTransport};

pub mod amp;
pub mod claude_code;
pub mod codex;
pub mod cursor;
pub mod gemini;
pub mod opencode;

/// A raw on-disk entry keyed by server name. Values are opaque
/// `serde_json::Value`s so format quirks per-adapter (e.g. Gemini's
/// `httpUrl`) round-trip unchanged.
pub type ServerMap = BTreeMap<String, Value>;

/// Description of how to read/write a JSON config file that stores
/// MCP servers as a map at a nested path.
pub struct JsonSpec {
    /// Absolute path to the config file.
    pub config_path: std::path::PathBuf,
    /// JSON path segments down to the servers map, e.g. `&["mcpServers"]`
    /// for Claude / Cursor / Gemini / Amp, `&["mcp"]` for OpenCode.
    pub servers_path: &'static [&'static str],
    /// Template for a freshly-created config file (only used when
    /// the file doesn't exist yet). Must contain a map at
    /// `servers_path` (empty is fine).
    pub template: &'static str,
}

/// Read servers from a JSON config file. Missing file ⇒ empty map.
/// Malformed file ⇒ empty map (the adapter surfaces no error; the
/// file is treated as a clean slate on write). Non-object values
/// at the servers path are filtered out.
pub fn read_json(spec: &JsonSpec) -> anyhow::Result<ServerMap> {
    let contents = match std::fs::read_to_string(&spec.config_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(ServerMap::new()),
        Err(err) => {
            return Err(anyhow::Error::from(err).context(format!(
                "failed to read MCP config at {}",
                spec.config_path.display()
            )))
        }
    };
    if contents.trim().is_empty() {
        return Ok(ServerMap::new());
    }
    let parsed: Value = match serde_json::from_str(&contents) {
        Ok(v) => v,
        Err(_) => return Ok(ServerMap::new()),
    };
    Ok(extract_at_path(&parsed, spec.servers_path))
}

/// Write the servers map back into the JSON config at `servers_path`,
/// preserving sibling keys in the file. Creates parent directories
/// and falls back to [`JsonSpec::template`] when the file doesn't exist.
///
/// Rows in the file at `servers_path` that are not in `servers` are
/// **dropped**. Callers are responsible for passing a merged map
/// (registry-owned rows + non-registry rows read back from disk).
pub fn write_json(spec: &JsonSpec, servers: &ServerMap) -> anyhow::Result<()> {
    if let Some(parent) = spec.config_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create config directory {}",
                parent.display()
            )
        })?;
    }
    let existing = std::fs::read_to_string(&spec.config_path).ok();
    let mut root: Value = existing
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::from_str(spec.template).expect("valid template"));

    set_at_path(&mut root, spec.servers_path, server_map_to_value(servers));

    let pretty = serde_json::to_string_pretty(&root)
        .with_context(|| format!("failed to serialise {}", spec.config_path.display()))?;
    std::fs::write(&spec.config_path, pretty).with_context(|| {
        format!(
            "failed to write MCP config at {}",
            spec.config_path.display()
        )
    })?;
    Ok(())
}

/// Merge `registry_owned` into `disk` such that:
///   - every registry-owned id appears with the registry's version;
///   - every non-registry id from disk is preserved untouched;
///   - ids previously owned by the registry but no longer provided
///     are removed (callers must pass *all* currently-owned rows,
///     not just deltas).
///
/// `previously_owned_ids` is the set of ids the registry previously
/// wrote for this harness — rows with those ids are stripped from
/// disk before the new set is layered on top. Pass an empty set
/// on first write (nothing was owned yet).
pub fn merge_owned(
    disk: &ServerMap,
    registry_owned: &ServerMap,
    previously_owned_ids: &std::collections::HashSet<String>,
) -> ServerMap {
    let mut out = ServerMap::new();
    for (id, value) in disk {
        if registry_owned.contains_key(id) || previously_owned_ids.contains(id) {
            continue;
        }
        out.insert(id.clone(), value.clone());
    }
    for (id, value) in registry_owned {
        out.insert(id.clone(), value.clone());
    }
    out
}

fn extract_at_path(root: &Value, segments: &[&str]) -> ServerMap {
    let mut current = root;
    for seg in segments {
        match current {
            Value::Object(map) => match map.get(*seg) {
                Some(next) => current = next,
                None => return ServerMap::new(),
            },
            _ => return ServerMap::new(),
        }
    }
    let Value::Object(map) = current else {
        return ServerMap::new();
    };
    let mut out = ServerMap::new();
    for (k, v) in map {
        if v.is_object() {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

fn set_at_path(root: &mut Value, segments: &[&str], value: Value) {
    let (last, parents) = match segments.split_last() {
        Some(split) => split,
        None => return,
    };
    if !root.is_object() {
        *root = Value::Object(Map::new());
    }
    let mut cursor = root.as_object_mut().expect("just set to object");
    for seg in parents {
        let entry = cursor
            .entry((*seg).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        cursor = entry.as_object_mut().expect("just set to object");
    }
    cursor.insert((*last).to_string(), value);
}

fn server_map_to_value(map: &ServerMap) -> Value {
    let mut out = Map::new();
    for (k, v) in map {
        out.insert(k.clone(), v.clone());
    }
    Value::Object(out)
}

/// Canonical `McpServer` → `serde_json::Value` in its on-disk shape
/// for a "passthrough" adapter (ClaudeCode, Amp). Other adapters
/// start from this and tweak as needed.
pub fn forward_passthrough(server: &McpServer) -> Value {
    match &server.transport {
        McpTransport::Stdio { command, args, env } => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::String("stdio".into()));
            obj.insert("command".into(), Value::String(command.clone()));
            if !args.is_empty() {
                obj.insert(
                    "args".into(),
                    Value::Array(args.iter().map(|a| Value::String(a.clone())).collect()),
                );
            }
            if !env.is_empty() {
                let mut env_map = Map::new();
                for (k, v) in env {
                    env_map.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("env".into(), Value::Object(env_map));
            }
            Value::Object(obj)
        }
        McpTransport::Http { url, headers } => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::String("http".into()));
            obj.insert("url".into(), Value::String(url.clone()));
            if !headers.is_empty() {
                let mut hmap = Map::new();
                for (k, v) in headers {
                    hmap.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("headers".into(), Value::Object(hmap));
            }
            Value::Object(obj)
        }
    }
}

/// Best-effort `Value` → canonical `McpTransport`, recognising both
/// the passthrough shape (type tag) and the older untagged shape
/// (`command`/`url` inferred from which key is present). Used by
/// `read_mcp_config` to classify on-disk entries.
pub fn reverse_passthrough_transport(value: &Value) -> Option<McpTransport> {
    let obj = value.as_object()?;
    let kind = obj.get("type").and_then(|v| v.as_str());
    if kind == Some("http") || (kind.is_none() && obj.contains_key("url")) {
        let url = obj.get("url")?.as_str()?.to_string();
        let headers = obj
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        return Some(McpTransport::Http { url, headers });
    }
    // stdio
    let command = obj.get("command")?.as_str()?.to_string();
    let args = obj
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let env = obj
        .get("env")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    Some(McpTransport::Stdio { command, args, env })
}

/// Build an [`McpServer`] from an on-disk `(id, value)` pair,
/// marking it as `McpSource::Custom` — the registry reconciles
/// these with its own owned entries on load.
pub fn reverse_passthrough_server(id: String, value: &Value) -> Option<McpServer> {
    let transport = reverse_passthrough_transport(value)?;
    Some(McpServer {
        label: id.clone(),
        id,
        transport,
        enabled_for: std::collections::HashSet::new(),
        source: McpSource::Custom,
    })
}

/// Read a harness's config file and surface every entry (both
/// registry-owned and user-authored) as `McpServer`s with
/// `McpSource::Custom`. The registry is responsible for promoting
/// rows whose ids it owns back to their true `source`.
pub fn read_json_servers(spec: &JsonSpec) -> anyhow::Result<Vec<McpServer>> {
    let disk = read_json(spec)?;
    let mut out = Vec::new();
    for (id, value) in disk {
        if let Some(server) = reverse_passthrough_server(id, &value) {
            out.push(server);
        }
    }
    Ok(out)
}

/// Resolve `$HOME` for adapter path building. Returns an error
/// when the OS can't produce a home directory rather than silently
/// falling through to a surprising default.
pub(crate) fn home() -> anyhow::Result<std::path::PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow!("could not locate user home directory"))
}

#[allow(dead_code)]
pub(crate) fn join_under_home(segments: &[&str]) -> anyhow::Result<std::path::PathBuf> {
    let mut p = home()?;
    for seg in segments {
        p = p.join(seg);
    }
    Ok(p)
}

#[allow(dead_code)]
pub(crate) fn exists(path: &Path) -> bool {
    std::fs::metadata(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn stdio_server(id: &str, command: &str) -> McpServer {
        McpServer {
            id: id.into(),
            label: id.into(),
            transport: McpTransport::Stdio {
                command: command.into(),
                args: vec![],
                env: BTreeMap::new(),
            },
            enabled_for: HashSet::new(),
            source: McpSource::Custom,
        }
    }

    fn json_roundtrip(server: &McpServer) -> Option<McpTransport> {
        let value = forward_passthrough(server);
        reverse_passthrough_transport(&value)
    }

    #[test]
    fn passthrough_roundtrips_stdio() {
        let s = stdio_server("x", "node");
        assert_eq!(json_roundtrip(&s), Some(s.transport));
    }

    #[test]
    fn passthrough_roundtrips_http_with_headers() {
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".into(), "Bearer abc".into());
        let s = McpServer {
            id: "y".into(),
            label: "y".into(),
            transport: McpTransport::Http {
                url: "https://example.test/mcp".into(),
                headers: headers.clone(),
            },
            enabled_for: HashSet::new(),
            source: McpSource::Custom,
        };
        assert_eq!(
            json_roundtrip(&s),
            Some(McpTransport::Http {
                url: "https://example.test/mcp".into(),
                headers,
            })
        );
    }

    #[test]
    fn merge_owned_preserves_third_party_rows() {
        let mut disk = ServerMap::new();
        disk.insert("user-added".into(), serde_json::json!({"command": "foo"}));
        disk.insert("ours".into(), serde_json::json!({"command": "old"}));

        let mut owned = ServerMap::new();
        owned.insert("ours".into(), serde_json::json!({"command": "new"}));

        let mut prev: HashSet<String> = HashSet::new();
        prev.insert("ours".into());

        let out = merge_owned(&disk, &owned, &prev);
        assert_eq!(out.get("user-added"), Some(&serde_json::json!({"command": "foo"})));
        assert_eq!(out.get("ours"), Some(&serde_json::json!({"command": "new"})));
    }

    #[test]
    fn merge_owned_removes_rows_toggled_off() {
        let mut disk = ServerMap::new();
        disk.insert("user-added".into(), serde_json::json!({"command": "foo"}));
        disk.insert("retired".into(), serde_json::json!({"command": "gone"}));

        let owned = ServerMap::new();
        let mut prev: HashSet<String> = HashSet::new();
        prev.insert("retired".into());

        let out = merge_owned(&disk, &owned, &prev);
        assert!(out.contains_key("user-added"));
        assert!(!out.contains_key("retired"));
    }

    #[test]
    fn json_round_trip_preserves_unknown_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("claude.json");
        std::fs::write(
            &cfg,
            r#"{ "theme": "dark", "mcpServers": { "user": { "command": "node" } } }"#,
        )
        .unwrap();
        let spec = JsonSpec {
            config_path: cfg.clone(),
            servers_path: &["mcpServers"],
            template: r#"{"mcpServers":{}}"#,
        };

        let disk = read_json(&spec).unwrap();
        assert!(disk.contains_key("user"));

        let mut new_map = ServerMap::new();
        new_map.insert("ours".into(), serde_json::json!({"command": "x"}));
        write_json(&spec, &new_map).unwrap();

        let rewritten = std::fs::read_to_string(&cfg).unwrap();
        let parsed: Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(parsed["theme"], "dark");
        assert!(parsed["mcpServers"]["ours"].is_object());
        // NB: caller passes already-merged map; this test asserts
        // write_json writes what it's given, preserving only
        // unrelated siblings (theme) — not the "user" key.
    }
}
