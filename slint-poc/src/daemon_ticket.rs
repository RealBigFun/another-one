//! Daemon ticket discovery and Slint-side pre-authorization.
//!
//! GPUI source of truth: `desktop/src/daemon_host.rs` is the writer that publishes
//! `endpoint.ticket` and `paired_peers` under
//! `${XDG_CONFIG_HOME:-$HOME/.config}/another-one/daemon/`. This module is the
//! Slint-side reader for the same paths plus the legacy sandbox ticket under the
//! system temp dir.
//!
//! Path contract (mirrored from `desktop/src/daemon_host.rs`):
//! - primary writer constructs `dir.join("endpoint.ticket")` and
//!   `dir.join("paired_peers")` (`daemon_host.rs:1407-1408`);
//! - the directory resolves to `${XDG_CONFIG_HOME or $HOME/.config}/another-one/daemon`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Context;
use iroh::EndpointId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DaemonTicketSource {
    Primary,
    Sandbox,
}

pub(crate) struct DaemonTicket {
    pub(crate) endpoint_id: EndpointId,
    pub(crate) direct_addrs: Vec<SocketAddr>,
    pub(crate) source: DaemonTicketSource,
}

pub(crate) fn load_ticket() -> anyhow::Result<Option<DaemonTicket>> {
    for (source, path) in daemon_ticket_candidates() {
        let Some((endpoint_id, direct_addrs)) = load_ticket_from_path(&path)? else {
            continue;
        };
        return Ok(Some(DaemonTicket {
            endpoint_id,
            direct_addrs,
            source,
        }));
    }
    Ok(None)
}

fn load_ticket_from_path(path: &Path) -> anyhow::Result<Option<(EndpointId, Vec<SocketAddr>)>> {
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };

    parse_ticket(&content)
}

pub(crate) fn parse_ticket(content: &str) -> anyhow::Result<Option<(EndpointId, Vec<SocketAddr>)>> {
    let mut id = None;
    let mut addrs = Vec::new();
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("id=") {
            id = Some(rest.trim().parse().context("parse EndpointId in ticket")?);
        } else if let Some(rest) = line.strip_prefix("addr=") {
            addrs.push(rest.trim().parse().context("parse addr in ticket")?);
        }
    }

    Ok(id.map(|id| (id, addrs)))
}

pub(crate) fn daemon_ticket_candidates() -> Vec<(DaemonTicketSource, PathBuf)> {
    vec![
        (DaemonTicketSource::Primary, primary_daemon_ticket_path()),
        (
            DaemonTicketSource::Sandbox,
            std::env::temp_dir().join("daemon-sandbox.ticket"),
        ),
    ]
}

pub(crate) fn pre_authorize_local_client(
    endpoint_id: EndpointId,
    source: DaemonTicketSource,
) -> anyhow::Result<()> {
    daemon_sandbox::persist_pairing(
        &endpoint_id.to_string(),
        &paired_peers_path_for_ticket_source(source),
    )
}

fn paired_peers_path_for_ticket_source(source: DaemonTicketSource) -> PathBuf {
    match source {
        DaemonTicketSource::Primary => primary_daemon_paired_peers_path(),
        DaemonTicketSource::Sandbox => sandbox_paired_peers_path(),
    }
}

fn primary_daemon_ticket_path() -> PathBuf {
    primary_daemon_config_dir().join("endpoint.ticket")
}

fn primary_daemon_paired_peers_path() -> PathBuf {
    primary_daemon_config_dir().join("paired_peers")
}

fn primary_daemon_config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("another-one").join("daemon")
}

fn sandbox_paired_peers_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("another-one-sandbox").join("paired_peers")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slint_daemon_ticket_prefers_primary_daemon_before_sandbox() {
        let candidates = daemon_ticket_candidates();

        assert_eq!(candidates[0].0, DaemonTicketSource::Primary);
        assert!(candidates[0]
            .1
            .ends_with("another-one/daemon/endpoint.ticket"));
        assert_eq!(candidates[1].0, DaemonTicketSource::Sandbox);
        assert!(candidates[1].1.ends_with("daemon-sandbox.ticket"));
    }

    #[test]
    fn slint_daemon_ticket_parses_endpoint_and_direct_addrs() {
        let endpoint_id = iroh::SecretKey::generate().public().to_string();
        let ticket = format!("id={endpoint_id}\naddr=127.0.0.1:55123\nrelay=https://relay.test\n");

        let (parsed_id, addrs) = parse_ticket(&ticket)
            .expect("valid ticket parses")
            .expect("ticket includes endpoint id");

        assert_eq!(parsed_id.to_string(), endpoint_id);
        assert_eq!(addrs, vec!["127.0.0.1:55123".parse().unwrap()]);
    }

    /// Pin the Slint-side ticket reader to the GPUI writer's path conventions in
    /// `desktop/src/daemon_host.rs`. If GPUI ever renames `endpoint.ticket` or
    /// `paired_peers`, this test fails so Slint isn't silently looking at stale
    /// paths.
    #[test]
    fn slint_daemon_ticket_paths_match_gpui_writer_contract() {
        let gpui = include_str!("../../desktop/src/daemon_host.rs");
        assert!(
            gpui.contains("dir.join(\"endpoint.ticket\")"),
            "GPUI writer no longer constructs endpoint.ticket path; update Slint reader."
        );
        assert!(
            gpui.contains("dir.join(\"paired_peers\")"),
            "GPUI writer no longer constructs paired_peers path; update Slint reader."
        );
        let primary = primary_daemon_ticket_path();
        assert!(primary.ends_with("another-one/daemon/endpoint.ticket"));
        let paired = primary_daemon_paired_peers_path();
        assert!(paired.ends_with("another-one/daemon/paired_peers"));
    }
}
