//! Built-in MCP catalog.
//!
//! A curated list of well-known MCP servers the user can add to the
//! registry with one click. Entries appear on the MCP page ahead of
//! user-added servers and carry `McpSource::Catalog`.
//!
//! Not exhaustive — the catalog lives here, in code, intentionally.
//! We want catalog changes to flow through code review; a remote
//! fetched list would blur the trust boundary on what the app pushes
//! into users' harness configs.

use std::collections::{BTreeMap, HashSet};

use crate::mcp::{McpServer, McpSource, McpTransport};

pub struct CatalogEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub docs_url: &'static str,
    /// Builds the default transport config. Catalog entries that
    /// need credentials surface placeholder values (e.g.
    /// `YOUR_API_KEY`) that the user replaces after adding.
    pub default_transport: fn() -> McpTransport,
}

pub fn entries() -> &'static [CatalogEntry] {
    CATALOG
}

pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

/// Materialise a catalog entry into an `McpServer` ready for the
/// registry. The returned server has `enabled_for` empty — toggles
/// are up to the user.
pub fn instantiate(entry: &CatalogEntry) -> McpServer {
    McpServer {
        id: entry.id.to_string(),
        label: entry.label.to_string(),
        transport: (entry.default_transport)(),
        enabled_for: HashSet::new(),
        source: McpSource::Catalog,
    }
}

fn playwright_transport() -> McpTransport {
    McpTransport::Stdio {
        command: "npx".into(),
        args: vec!["@playwright/mcp@latest".into()],
        env: BTreeMap::new(),
    }
}

fn context7_transport() -> McpTransport {
    let mut headers = BTreeMap::new();
    headers.insert("CONTEXT7_API_KEY".into(), "YOUR_API_KEY".into());
    McpTransport::Http {
        url: "https://mcp.context7.com/mcp".into(),
        headers,
    }
}

fn linear_transport() -> McpTransport {
    McpTransport::Http {
        url: "https://mcp.linear.app/mcp".into(),
        headers: BTreeMap::new(),
    }
}

fn sentry_transport() -> McpTransport {
    let mut headers = BTreeMap::new();
    headers.insert("SENTRY_ACCESS_TOKEN".into(), "YOUR_ACCESS_TOKEN".into());
    McpTransport::Http {
        url: "https://mcp.sentry.dev/mcp".into(),
        headers,
    }
}

fn figma_transport() -> McpTransport {
    McpTransport::Http {
        url: "https://mcp.figma.com/mcp".into(),
        headers: BTreeMap::new(),
    }
}

fn github_transport() -> McpTransport {
    McpTransport::Stdio {
        command: "npx".into(),
        args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
        env: BTreeMap::new(),
    }
}

/// Stable id for the daemon MCP entry. Used by
/// [`crate::mcp::registry::McpRegistry::ensure_builtin`] so
/// upgrading the app preserves the user's `enabled_for` set
/// across versions. Do not rename.
pub const DAEMON_MCP_ID: &str = "another-one-daemon";

/// Build the `McpServer` the app re-registers on every startup
/// to represent its own daemon MCP. `enabled_for` is empty on
/// first install; `ensure_builtin` preserves the user's choice
/// on subsequent runs.
///
/// The catalog entry uses stdio transport with `shim_bin` as
/// the command and the socket path exported via
/// `ANOTHERONE_MCP_SOCKET` — the shim prefers that env var so
/// the user's harness config can continue working even if we
/// later relocate the default per-user socket path.
pub fn daemon_catalog_entry(shim_bin: &std::path::Path, socket_path: &std::path::Path) -> McpServer {
    let mut env = BTreeMap::new();
    env.insert(
        "ANOTHERONE_MCP_SOCKET".to_string(),
        socket_path.to_string_lossy().into_owned(),
    );
    McpServer {
        id: DAEMON_MCP_ID.to_string(),
        label: "AnotherOne Daemon".to_string(),
        transport: McpTransport::Stdio {
            command: shim_bin.to_string_lossy().into_owned(),
            args: Vec::new(),
            env,
        },
        enabled_for: HashSet::new(),
        source: McpSource::BuiltInDaemon,
    }
}

const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "playwright",
        label: "Playwright",
        description: "Browser automation with Playwright.",
        docs_url: "https://github.com/microsoft/playwright-mcp",
        default_transport: playwright_transport,
    },
    CatalogEntry {
        id: "context7",
        label: "Context7",
        description: "Fetch up-to-date documentation and code examples.",
        docs_url: "https://github.com/upstash/context7",
        default_transport: context7_transport,
    },
    CatalogEntry {
        id: "linear",
        label: "Linear",
        description: "Manage issues, projects, and team workflows in Linear.",
        docs_url: "https://linear.app/docs/mcp",
        default_transport: linear_transport,
    },
    CatalogEntry {
        id: "sentry",
        label: "Sentry",
        description: "Search, query, and debug errors.",
        docs_url: "https://docs.sentry.io/product/sentry-mcp/",
        default_transport: sentry_transport,
    },
    CatalogEntry {
        id: "figma",
        label: "Figma",
        description: "Generate diagrams and code from Figma context.",
        docs_url: "https://help.figma.com/hc/en-us/articles/32132100833559",
        default_transport: figma_transport,
    },
    CatalogEntry {
        id: "github",
        label: "GitHub",
        description: "Read/write GitHub issues, PRs, and repository data.",
        docs_url: "https://github.com/modelcontextprotocol/servers/tree/main/src/github",
        default_transport: github_transport,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_entry_instantiates_without_panic() {
        for entry in entries() {
            let server = instantiate(entry);
            assert_eq!(server.id, entry.id);
            assert_eq!(server.source, McpSource::Catalog);
            assert!(server.enabled_for.is_empty());
        }
    }

    #[test]
    fn catalog_ids_are_unique() {
        let mut seen = HashSet::new();
        for entry in entries() {
            assert!(seen.insert(entry.id), "duplicate catalog id: {}", entry.id);
        }
    }
}
