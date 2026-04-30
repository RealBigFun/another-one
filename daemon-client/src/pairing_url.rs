//! Inverse of `daemon/src/transport_iroh.rs::build_pairing_url_with_token`.
//!
//! Pairing URLs look like
//! `iroh://<endpoint_id>?direct=<addr>,<addr>&relay=<urlencoded>&pair=<hex>`.
//! `direct` and `relay` may both be present; at least one is required for
//! the client to dial. `pair` is the TOFU nonce — `Some` until the daemon
//! has accepted this client.

use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct PairingUrl {
    pub endpoint_id: String,
    pub direct_addrs: Vec<String>,
    pub relay_urls: Vec<String>,
    pub pair_token: Option<String>,
}

pub fn parse_pairing_url(url: &str) -> Result<PairingUrl> {
    let trimmed = url.trim();
    let body = match trimmed.strip_prefix("iroh://") {
        Some(rest) => rest,
        None => bail!("pairing URL must start with `iroh://`: {trimmed:?}"),
    };

    let (endpoint_id, query) = body.split_once('?').unwrap_or((body, ""));
    if endpoint_id.is_empty() {
        bail!("pairing URL has empty endpoint id: {trimmed:?}");
    }

    let mut direct_addrs: Vec<String> = Vec::new();
    let mut relay_urls: Vec<String> = Vec::new();
    let mut pair_token: Option<String> = None;

    if !query.is_empty() {
        for part in query.split('&') {
            if let Some(directs) = part.strip_prefix("direct=") {
                for a in directs.split(',') {
                    if !a.is_empty() {
                        direct_addrs.push(a.to_string());
                    }
                }
            } else if let Some(relay) = part.strip_prefix("relay=") {
                let decoded: String = urlencoding::decode(relay)
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| relay.to_string());
                if !decoded.is_empty() {
                    relay_urls.push(decoded);
                }
            } else if let Some(token) = part.strip_prefix("pair=") {
                if !token.is_empty() {
                    pair_token = Some(token.to_string());
                }
            }
            // Unknown keys are ignored for forward-compat.
        }
    }

    Ok(PairingUrl {
        endpoint_id: endpoint_id.to_string(),
        direct_addrs,
        relay_urls,
        pair_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iroh_url_with_direct_relay_pair() {
        let input = "iroh://abc123?direct=192.168.1.42:11204,10.0.0.1:11204&relay=https%3A%2F%2Frelay.example%2F&pair=deadbeef";
        let parsed = parse_pairing_url(input).expect("should parse");
        assert_eq!(parsed.endpoint_id, "abc123");
        assert_eq!(
            parsed.direct_addrs,
            vec!["192.168.1.42:11204".to_string(), "10.0.0.1:11204".to_string()]
        );
        assert_eq!(parsed.relay_urls, vec!["https://relay.example/".to_string()]);
        assert_eq!(parsed.pair_token.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn rejects_missing_scheme() {
        let err = parse_pairing_url("https://abc123?direct=1.2.3.4:5678").unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("iroh://"),
            "error should mention required scheme, got: {msg}"
        );
    }

    #[test]
    fn accepts_empty_query() {
        let parsed = parse_pairing_url("iroh://abc123").expect("should parse");
        assert_eq!(parsed.endpoint_id, "abc123");
        assert!(parsed.direct_addrs.is_empty());
        assert!(parsed.relay_urls.is_empty());
        assert!(parsed.pair_token.is_none());
    }

    #[test]
    fn forward_compat_unknown_keys() {
        let input = "iroh://abc123?direct=1.2.3.4:5678&foo=bar&pair=cafef00d";
        let parsed = parse_pairing_url(input).expect("should parse");
        assert_eq!(parsed.endpoint_id, "abc123");
        assert_eq!(parsed.direct_addrs, vec!["1.2.3.4:5678".to_string()]);
        assert_eq!(parsed.pair_token.as_deref(), Some("cafef00d"));
        assert!(parsed.relay_urls.is_empty());
    }
}
