//! MCP (Model Context Protocol) server registry.
//!
//! The registry is the source of truth for which MCP servers the user
//! has configured and which harnesses each one is enabled for. Harness
//! config files (`.claude/…`, `.cursor/mcp.json`, `.codex/…`, etc.)
//! are *sync targets*: the registry survives even when no harness has
//! a given entry enabled, and entries the user set up outside
//! AnotherOne are preserved in-place.
//!
//! The public surface:
//!   - [`McpServer`] — canonical entry.
//!   - [`McpTransport`] — stdio or http.
//!   - [`McpSource`] — where an entry came from.
//!
//! Per-harness adapters live under `mcp::adapters::*` (not yet
//! implemented; dispatched through `AgentHarness::{read,write}_mcp_config`).
//! The persistent registry (load/save + 2-phase sync) lives in
//! [`registry`].

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::agents::AgentProviderKind;

pub mod adapters;
pub mod catalog;
pub mod registry;

/// A single MCP server entry in the registry. Identity is `id`; the
/// same id is used as the partition key when writing to harness
/// config files (so the registry only ever replaces rows it owns).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServer {
    /// Stable, user-visible-safe id (e.g. `"context7"`, `"another-one-daemon"`,
    /// `"my-internal-tool-3f2a"`).
    pub id: String,
    /// Display label for the UI.
    pub label: String,
    pub transport: McpTransport,
    /// Providers this entry is enabled for. An empty set means
    /// visible in the registry but not written to any harness config.
    #[serde(default)]
    pub enabled_for: HashSet<AgentProviderKind>,
    #[serde(default)]
    pub source: McpSource,
}

/// How the MCP server is reached. Matches the two transports MCP
/// clients commonly support; per-harness adapters validate that the
/// target harness actually supports a given transport and surface an
/// error via toast if not (e.g. Codex is stdio-only today).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum McpTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

/// Where an entry came from. Catalog and BuiltInDaemon entries are
/// managed by AnotherOne; Custom entries are user-added.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpSource {
    /// A curated built-in entry (context7, github, filesystem, …).
    Catalog,
    /// User-added.
    #[default]
    Custom,
    /// The AnotherOne daemon itself (Phase B). Disabled by default.
    BuiltInDaemon,
}
