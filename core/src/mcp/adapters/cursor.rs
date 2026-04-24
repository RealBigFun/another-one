//! Cursor adapter.
//!
//! Config: `~/.cursor/mcp.json`, with `mcpServers` keyed by server
//! name. HTTP entries carry `url` directly with no `type` tag —
//! we drop the `"type": "http"` tag on write and re-infer it on
//! read (an entry with `url` and no `command` is HTTP).

use std::collections::HashSet;

use serde_json::Value;

use crate::mcp::adapters::{
    forward_passthrough, home, merge_owned, read_json, read_json_servers, write_json, JsonSpec,
    ServerMap,
};
use crate::mcp::McpServer;

fn spec() -> anyhow::Result<JsonSpec> {
    Ok(JsonSpec {
        config_path: home()?.join(".cursor").join("mcp.json"),
        servers_path: &["mcpServers"],
        template: r#"{"mcpServers":{}}"#,
    })
}

pub fn config_path() -> Option<std::path::PathBuf> {
    spec().ok().map(|s| s.config_path)
}

pub fn read() -> anyhow::Result<Vec<McpServer>> {
    // Cursor omits the `type` tag on HTTP entries; the shared
    // passthrough reverser already infers from `url`/`command`
    // key presence, so a plain passthrough read works here.
    read_json_servers(&spec()?)
}

fn forward_cursor(server: &McpServer) -> Value {
    let value = forward_passthrough(server);
    let Value::Object(mut obj) = value else {
        return value;
    };
    // HTTP entries: strip the `type: "http"` tag for Cursor's taste.
    if obj.get("type").and_then(|t| t.as_str()) == Some("http") {
        obj.remove("type");
    }
    Value::Object(obj)
}

pub fn write(
    registry_owned: &[&McpServer],
    previously_owned_ids: &HashSet<String>,
) -> anyhow::Result<()> {
    let spec = spec()?;
    let disk = read_json(&spec)?;
    let mut owned = ServerMap::new();
    for server in registry_owned {
        owned.insert(server.id.clone(), forward_cursor(server));
    }
    let merged = merge_owned(&disk, &owned, previously_owned_ids);
    write_json(&spec, &merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{McpSource, McpTransport};
    use std::collections::BTreeMap;

    #[test]
    fn cursor_strips_http_type_tag_on_write() {
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
        let value = forward_cursor(&server);
        let obj = value.as_object().unwrap();
        assert_eq!(obj.get("url").and_then(|v| v.as_str()), Some("https://e.test"));
        assert!(!obj.contains_key("type"));
    }

    #[test]
    fn cursor_leaves_stdio_type_tag_intact() {
        let server = McpServer {
            id: "x".into(),
            label: "x".into(),
            transport: McpTransport::Stdio {
                command: "node".into(),
                args: vec![],
                env: BTreeMap::new(),
            },
            enabled_for: HashSet::new(),
            source: McpSource::Custom,
        };
        let value = forward_cursor(&server);
        let obj = value.as_object().unwrap();
        assert_eq!(obj.get("type").and_then(|v| v.as_str()), Some("stdio"));
    }

}
