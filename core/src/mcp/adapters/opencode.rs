//! OpenCode adapter.
//!
//! Config: `~/.config/opencode/opencode.json`, `mcp` (not `mcpServers`)
//! keyed by server name. Entries are wrapped:
//!
//!   - stdio → `{ "type": "local", "command": [cmd, ...args],
//!     "enabled": true, "env": {...} }` (note: command is an array,
//!     not a `command` + `args` split).
//!   - http  → `{ "type": "remote", "url": "...", "enabled": true,
//!     "headers": {..., "Accept": "application/json, text/event-stream"} }`.
//!
//! The file is JSONC in the wild (comments + trailing commas). We
//! fall back to plain JSON parsing; malformed-with-comments files
//! round-trip as empty on read (adapter returns no entries) and
//! are rewritten as plain JSON on write. This is a known loss —
//! tracked as a follow-up.

use std::collections::{BTreeMap, HashSet};

use serde_json::{Map, Value};

use crate::mcp::adapters::{
    home, merge_owned, read_json, write_json, JsonSpec, ServerMap,
};
use crate::mcp::{McpServer, McpSource, McpTransport};

const INJECTED_ACCEPT: &str = "application/json, text/event-stream";

fn spec() -> anyhow::Result<JsonSpec> {
    Ok(JsonSpec {
        config_path: home()?
            .join(".config")
            .join("opencode")
            .join("opencode.json"),
        servers_path: &["mcp"],
        template: r#"{"mcp":{}}"#,
    })
}

pub fn config_path() -> Option<std::path::PathBuf> {
    spec().ok().map(|s| s.config_path)
}

pub fn read() -> anyhow::Result<Vec<McpServer>> {
    let disk = read_json(&spec()?)?;
    let mut out = Vec::new();
    for (id, value) in disk {
        let Some(obj) = value.as_object() else {
            continue;
        };
        let entry_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let transport = match entry_type {
            "remote" => {
                let Some(url) = obj.get("url").and_then(|v| v.as_str()) else {
                    continue;
                };
                let headers: BTreeMap<String, String> = obj
                    .get("headers")
                    .and_then(|v| v.as_object())
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .filter(|(k, v)| !(k == "Accept" && v == INJECTED_ACCEPT))
                            .collect()
                    })
                    .unwrap_or_default();
                McpTransport::Http {
                    url: url.to_string(),
                    headers,
                }
            }
            "local" => {
                let Some(cmd_arr) = obj.get("command").and_then(|v| v.as_array()) else {
                    continue;
                };
                let mut it = cmd_arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()));
                let Some(command) = it.next() else {
                    continue;
                };
                let args: Vec<String> = it.collect();
                let env = obj
                    .get("env")
                    .and_then(|v| v.as_object())
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();
                McpTransport::Stdio {
                    command,
                    args,
                    env,
                }
            }
            _ => continue,
        };
        out.push(McpServer {
            id: id.clone(),
            label: id,
            transport,
            enabled_for: HashSet::new(),
            source: McpSource::Custom,
        });
    }
    Ok(out)
}

fn forward_opencode(server: &McpServer) -> Value {
    match &server.transport {
        McpTransport::Stdio { command, args, env } => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::String("local".into()));
            let mut cmd_arr = Vec::with_capacity(1 + args.len());
            cmd_arr.push(Value::String(command.clone()));
            for a in args {
                cmd_arr.push(Value::String(a.clone()));
            }
            obj.insert("command".into(), Value::Array(cmd_arr));
            obj.insert("enabled".into(), Value::Bool(true));
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
            obj.insert("type".into(), Value::String("remote".into()));
            obj.insert("url".into(), Value::String(url.clone()));
            obj.insert("enabled".into(), Value::Bool(true));
            let mut hmap = Map::new();
            for (k, v) in headers {
                hmap.insert(k.clone(), Value::String(v.clone()));
            }
            hmap.entry("Accept".to_string())
                .or_insert_with(|| Value::String(INJECTED_ACCEPT.to_string()));
            obj.insert("headers".into(), Value::Object(hmap));
            Value::Object(obj)
        }
    }
}

pub fn write(
    registry_owned: &[&McpServer],
    previously_owned_ids: &HashSet<String>,
) -> anyhow::Result<()> {
    let spec = spec()?;
    let disk = read_json(&spec)?;
    let mut owned = ServerMap::new();
    for server in registry_owned {
        owned.insert(server.id.clone(), forward_opencode(server));
    }
    let merged = merge_owned(&disk, &owned, previously_owned_ids);
    write_json(&spec, &merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_wraps_stdio_in_local_with_command_array() {
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
        let value = forward_opencode(&server);
        assert_eq!(value["type"], "local");
        assert_eq!(value["command"], serde_json::json!(["node", "server.js"]));
        assert_eq!(value["enabled"], true);
        assert_eq!(value["env"]["FOO"], "bar");
    }

    #[test]
    fn opencode_wraps_http_in_remote() {
        let server = McpServer {
            id: "x".into(),
            label: "x".into(),
            transport: McpTransport::Http {
                url: "https://e.test".into(),
                headers: BTreeMap::new(),
            },
            enabled_for: HashSet::new(),
            source: McpSource::Custom,
        };
        let value = forward_opencode(&server);
        assert_eq!(value["type"], "remote");
        assert_eq!(value["url"], "https://e.test");
        assert_eq!(value["enabled"], true);
        assert_eq!(value["headers"]["Accept"], INJECTED_ACCEPT);
    }
}
