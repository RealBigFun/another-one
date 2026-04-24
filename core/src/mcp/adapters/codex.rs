//! Codex adapter.
//!
//! Config: `~/.codex/config.toml`, tables under `[mcp_servers.*]`.
//! Codex is **stdio-only** — HTTP registry entries are filtered
//! out on write (the caller gets no error; the page's per-cell
//! state is how this is surfaced to the user).
//!
//! Uses `toml_edit` so user-authored comments, blank lines, and
//! unrelated top-level keys survive a round-trip. Only the tables
//! we own (by id) are replaced.

use std::collections::HashSet;

use anyhow::Context;
use toml_edit::{Array, DocumentMut, Item, Table, Value as TomlValue};

use crate::mcp::adapters::home;
use crate::mcp::{McpServer, McpSource, McpTransport};

const SERVERS_KEY: &str = "mcp_servers";

fn config_path_inner() -> anyhow::Result<std::path::PathBuf> {
    Ok(home()?.join(".codex").join("config.toml"))
}

pub fn config_path() -> Option<std::path::PathBuf> {
    config_path_inner().ok()
}

fn read_document() -> anyhow::Result<DocumentMut> {
    let path = config_path_inner()?;
    let contents = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(anyhow::Error::from(err)
                .context(format!("failed to read Codex config at {}", path.display())))
        }
    };
    contents
        .parse::<DocumentMut>()
        .with_context(|| format!("failed to parse {}", path.display()))
}

pub fn read() -> anyhow::Result<Vec<McpServer>> {
    let doc = match read_document() {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(servers) = doc.get(SERVERS_KEY).and_then(Item::as_table) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (id, item) in servers.iter() {
        let Some(table) = item.as_table() else { continue };
        let Some(command) = table
            .get("command")
            .and_then(Item::as_value)
            .and_then(TomlValue::as_str)
        else {
            continue;
        };
        let args = table
            .get("args")
            .and_then(Item::as_value)
            .and_then(TomlValue::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let env = table
            .get("env")
            .and_then(Item::as_table)
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| {
                        v.as_value()
                            .and_then(TomlValue::as_str)
                            .map(|s| (k.to_string(), s.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(McpServer {
            id: id.to_string(),
            label: id.to_string(),
            transport: McpTransport::Stdio {
                command: command.to_string(),
                args,
                env,
            },
            enabled_for: HashSet::new(),
            source: McpSource::Custom,
        });
    }
    Ok(out)
}

fn server_to_table(server: &McpServer) -> Option<Table> {
    let McpTransport::Stdio { command, args, env } = &server.transport else {
        // HTTP entries are silently dropped (Codex doesn't support them).
        return None;
    };
    let mut table = Table::new();
    table.insert("command", Item::Value(command.clone().into()));
    if !args.is_empty() {
        let mut arr = Array::new();
        for a in args {
            arr.push(a.clone());
        }
        table.insert("args", Item::Value(TomlValue::Array(arr)));
    }
    if !env.is_empty() {
        let mut env_table = Table::new();
        for (k, v) in env {
            env_table.insert(k, Item::Value(v.clone().into()));
        }
        env_table.set_implicit(false);
        table.insert("env", Item::Table(env_table));
    }
    Some(table)
}

pub fn write(
    registry_owned: &[&McpServer],
    previously_owned_ids: &HashSet<String>,
) -> anyhow::Result<()> {
    let path = config_path_inner()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut doc = read_document().unwrap_or_else(|_| DocumentMut::new());

    // Ensure `[mcp_servers]` exists as an implicit table parent so
    // child tables render as `[mcp_servers.name]`.
    if doc.get(SERVERS_KEY).is_none() {
        let mut t = Table::new();
        t.set_implicit(true);
        doc.insert(SERVERS_KEY, Item::Table(t));
    } else if let Some(existing) = doc.get_mut(SERVERS_KEY).and_then(Item::as_table_mut) {
        existing.set_implicit(true);
    }

    let servers_tbl = doc
        .get_mut(SERVERS_KEY)
        .and_then(Item::as_table_mut)
        .expect("just inserted or upgraded");

    // Remove previously-owned ids that aren't in the current owned set.
    let owned_ids: HashSet<String> = registry_owned.iter().map(|s| s.id.clone()).collect();
    let to_remove: Vec<String> = previously_owned_ids
        .iter()
        .filter(|id| !owned_ids.contains(id.as_str()))
        .cloned()
        .collect();
    for id in to_remove {
        servers_tbl.remove(&id);
    }

    // Replace each owned row.
    for server in registry_owned {
        let Some(table) = server_to_table(server) else {
            // HTTP entry on Codex — make sure we haven't left a stale copy.
            servers_tbl.remove(&server.id);
            continue;
        };
        servers_tbl.insert(&server.id, Item::Table(table));
    }

    std::fs::write(&path, doc.to_string())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn server_to_table_drops_http_entries() {
        let http = McpServer {
            id: "x".into(),
            label: "x".into(),
            transport: McpTransport::Http {
                url: "https://e.test".into(),
                headers: BTreeMap::new(),
            },
            enabled_for: HashSet::new(),
            source: McpSource::Custom,
        };
        assert!(server_to_table(&http).is_none());
    }

    #[test]
    fn server_to_table_round_trips_stdio() {
        let mut env = BTreeMap::new();
        env.insert("FOO".into(), "bar".into());
        let server = McpServer {
            id: "x".into(),
            label: "x".into(),
            transport: McpTransport::Stdio {
                command: "node".into(),
                args: vec!["server.js".into()],
                env,
            },
            enabled_for: HashSet::new(),
            source: McpSource::Custom,
        };
        let table = server_to_table(&server).unwrap();
        assert_eq!(
            table.get("command").and_then(|v| v.as_str()),
            Some("node")
        );
        let args = table.get("args").and_then(Item::as_value).and_then(TomlValue::as_array).unwrap();
        assert_eq!(args.len(), 1);
        assert_eq!(args.get(0).and_then(TomlValue::as_str), Some("server.js"));
        let env_tbl = table.get("env").and_then(Item::as_table).unwrap();
        assert_eq!(
            env_tbl
                .get("FOO")
                .and_then(Item::as_value)
                .and_then(TomlValue::as_str),
            Some("bar")
        );
    }
}
