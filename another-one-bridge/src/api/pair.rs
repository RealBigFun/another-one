//! FRB-exposed pairing surface for the embedded daemon.
//!
//! Reads from a host-registered [`crate::local_pair::LocalPairInfo`]
//! (see that module for boot-order semantics). Boot-order forgiving
//! — if the host hasn't registered a source yet (e.g. embedded
//! daemon still starting), [`pairing_info`] returns `None` and the
//! UI shows a "daemon not ready" empty state.

use std::io::Write;
use std::path::Path;

use crate::local_pair;

/// Snapshot of the embedded daemon's current pairing material.
/// Stable for one render of the pair-mobile modal; refetch after
/// [`regenerate_local_pairing`] to pick up the rotated nonce.
pub struct PairingInfo {
    pub url: String,
    pub qr_png_bytes: Vec<u8>,
}

/// Current pairing material, or `None` if the host hasn't registered
/// the embedded daemon yet (boot race, or the binary was built
/// without daemon-sandbox).
pub fn pairing_info() -> Option<PairingInfo> {
    let handle = local_pair::local_pair_info()?;
    Some(PairingInfo {
        url: handle.pairing_url(),
        qr_png_bytes: handle.qr_png_bytes(),
    })
}

/// Reset paired mobile access by revoking persisted peers, while
/// preserving the desktop's own loopback self-trust entry, then roll
/// a fresh TOFU nonce and rebuild the pairing URL + QR. Errors
/// surface as a string so the bridge doesn't need to expose a
/// project-wide error type to Dart.
pub fn regenerate_local_pairing() -> Result<(), String> {
    let Some(handle) = local_pair::local_pair_info() else {
        return Err("embedded daemon not registered".to_string());
    };

    reset_persisted_pairings()?;
    handle.regenerate_pairing()
}

fn reset_persisted_pairings() -> Result<(), String> {
    let paired_peers_path = another_one_core::daemon_embed::paired_peers_path()
        .map_err(|e| format!("resolve paired peers path: {e:#}"))?;
    let local_device_node_id = crate::api::iroh_client::load_or_create_device_secret_key()
        .map_err(|e| format!("load local device identity: {e:#}"))?
        .public()
        .to_string();

    rewrite_paired_peers_with_local_device(&paired_peers_path, &local_device_node_id)
}

fn rewrite_paired_peers_with_local_device(
    path: &Path,
    local_device_node_id: &str,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create paired peers dir {}: {e}", parent.display()))?;
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("open paired peers {}: {e}", path.display()))?;
    file.write_all(format!("{local_device_node_id}\n").as_bytes())
        .map_err(|e| format!("write paired peers {}: {e}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("set paired peers permissions {}: {e}", path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::rewrite_paired_peers_with_local_device;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "another_one_bridge_pair_tests_{}_{}",
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn rewrite_paired_peers_with_local_device_should_replace_existing_entries() {
        let dir = unique_test_dir();
        let path = dir.join("paired_peers");
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(&path, "phone-a\nphone-b\n").expect("seed paired peers");

        rewrite_paired_peers_with_local_device(&path, "desktop-node-id")
            .expect("rewrite paired peers");

        let content = std::fs::read_to_string(&path).expect("read paired peers");
        assert_eq!(content, "desktop-node-id\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewrite_paired_peers_with_local_device_should_create_missing_parent_dirs() {
        let dir = unique_test_dir();
        let path = dir.join("nested").join("paired_peers");

        rewrite_paired_peers_with_local_device(&path, "desktop-node-id")
            .expect("rewrite paired peers");

        let content = std::fs::read_to_string(&path).expect("read paired peers");
        assert_eq!(content, "desktop-node-id\n");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
