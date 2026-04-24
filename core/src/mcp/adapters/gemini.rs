//! Gemini adapter.
//!
//! Config: `~/.gemini/settings.json`, `mcpServers` keyed by server
//! name. HTTP entries use the `httpUrl` key instead of `url` and
//! expect an injected `Accept: application/json, text/event-stream`
//! header (Gemini's SSE transport). We inject that header on write
//! (only if absent) and strip it on read (only if it's the default
//! we inject) so round-tripping is a fixed point.

use std::collections::{BTreeMap, HashSet};

use serde_json::{Map, Value};

use crate::mcp::adapters::{
    forward_passthrough, home, merge_owned, read_json, write_json, JsonSpec, ServerMap,
};
use crate::mcp::{McpServer, McpSource, McpTransport};

const INJECTED_ACCEPT: &str = "application/json, text/event-stream";

fn spec() -> anyhow::Result<JsonSpec> {
    Ok(JsonSpec {
        config_path: home()?.join(".gemini").join("settings.json"),
        servers_path: &["mcpServers"],
        template: r#"{"mcpServers":{}}"#,
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
        // Gemini's HTTP shape uses `httpUrl` instead of `url`.
        let transport = if let Some(url) = obj.get("httpUrl").and_then(|v| v.as_str()) {
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
        } else if let Some(command) = obj.get("command").and_then(|v| v.as_str()) {
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
                        .collect()
                })
                .unwrap_or_default();
            McpTransport::Stdio {
                command: command.to_string(),
                args,
                env,
            }
        } else {
            continue;
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

fn forward_gemini(server: &McpServer) -> Value {
    match &server.transport {
        McpTransport::Stdio { .. } => forward_passthrough(server),
        McpTransport::Http { url, headers } => {
            let mut obj = Map::new();
            obj.insert("httpUrl".into(), Value::String(url.clone()));
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
        owned.insert(server.id.clone(), forward_gemini(server));
    }
    let merged = merge_owned(&disk, &owned, previously_owned_ids);
    write_json(&spec, &merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_writes_httpurl_and_injects_accept() {
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
        let value = forward_gemini(&server);
        let obj = value.as_object().unwrap();
        assert_eq!(obj["httpUrl"], "https://e.test");
        assert_eq!(obj["headers"]["Accept"], INJECTED_ACCEPT);
    }

    #[test]
    fn gemini_preserves_user_authored_accept() {
        let mut headers = BTreeMap::new();
        headers.insert("Accept".into(), "custom/one".into());
        let server = McpServer {
            id: "x".into(),
            label: "x".into(),
            transport: McpTransport::Http {
                url: "https://e.test".into(),
                headers,
            },
            enabled_for: HashSet::new(),
            source: McpSource::Custom,
        };
        let value = forward_gemini(&server);
        assert_eq!(value["headers"]["Accept"], "custom/one");
    }
}
