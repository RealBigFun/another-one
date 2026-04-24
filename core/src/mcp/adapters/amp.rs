//! Amp adapter.
//!
//! Config: `~/.config/amp/settings.json`, `mcpServers` at the top
//! level. Passthrough format, same shape as Claude Code.

use std::collections::HashSet;

use crate::mcp::adapters::{
    forward_passthrough, home, merge_owned, read_json, read_json_servers, write_json, JsonSpec,
    ServerMap,
};
use crate::mcp::McpServer;

fn spec() -> anyhow::Result<JsonSpec> {
    Ok(JsonSpec {
        config_path: home()?.join(".config").join("amp").join("settings.json"),
        servers_path: &["mcpServers"],
        template: r#"{"mcpServers":{}}"#,
    })
}

pub fn config_path() -> Option<std::path::PathBuf> {
    spec().ok().map(|s| s.config_path)
}

pub fn read() -> anyhow::Result<Vec<McpServer>> {
    read_json_servers(&spec()?)
}

pub fn write(
    registry_owned: &[&McpServer],
    previously_owned_ids: &HashSet<String>,
) -> anyhow::Result<()> {
    let spec = spec()?;
    let disk = read_json(&spec)?;
    let mut owned = ServerMap::new();
    for server in registry_owned {
        owned.insert(server.id.clone(), forward_passthrough(server));
    }
    let merged = merge_owned(&disk, &owned, previously_owned_ids);
    write_json(&spec, &merged)
}
