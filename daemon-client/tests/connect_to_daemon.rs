//! End-to-end smoke: spin up a real `daemon::run_endpoint` in the
//! same process, fish the pairing URL out of the resulting
//! [`daemon::EndpointHandle`], and dial it with
//! [`daemon_client::connect`]. Validates the full wire path —
//! ALPN handshake, `ControlEnvelope`-wrapped `Hello`, TOFU
//! pair-token check — against the live daemon implementation
//! without any phone in the loop.

use std::sync::Arc;
use std::time::Duration;

use daemon::sandbox::SandboxRegistry;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connect_with_pair_token_handshake_succeeds() {
    // Logging is opt-in (`RUST_LOG=daemon=debug,daemon_client=debug`).
    // We don't initialise a default subscriber here so test output stays
    // tidy when run from `cargo test` without env override.
    let _ = tracing_subscriber::fmt::try_init();

    let temp = tempfile::tempdir().expect("tempdir");
    let secret_key_path = temp.path().join("secret_key");
    let paired_peers_path = temp.path().join("paired_peers.json");

    // The sandbox registry returns a synthetic single-project tree —
    // good enough for verifying the transport layer.
    let registry: Arc<dyn daemon::DaemonRegistry> = Arc::new(SandboxRegistry::new());

    let handle = daemon::run_endpoint(registry, secret_key_path, paired_peers_path)
        .await
        .expect("daemon::run_endpoint");

    let pairing_url = handle.pairing_url();
    assert!(
        pairing_url.starts_with("iroh://"),
        "pairing URL shape: {pairing_url}"
    );
    assert!(
        pairing_url.contains("pair="),
        "pairing URL must include a TOFU token: {pairing_url}"
    );

    // Dial. The client also runs on its own internal tokio runtime so
    // timing out here through `tokio::time::timeout` is still robust
    // against an iroh handshake that takes longer than expected.
    let session_result = tokio::time::timeout(
        Duration::from_secs(20),
        daemon_client::connect(&pairing_url),
    )
    .await;

    let session = match session_result {
        Err(_) => panic!("daemon_client::connect timed out after 20s"),
        Ok(Err(e)) => panic!("daemon_client::connect failed: {e:#}"),
        Ok(Ok(s)) => s,
    };

    // Round-trip a `ListProjects` so we exercise the worker-reply
    // path too (the daemon's `WorkerReplyEnvelope` decode in
    // `session.rs`'s recv loop, plus the `ControlEnvelope` send for
    // the call itself). Sandbox registry returns a synthetic
    // single-project tree, so we just need the variant to match.
    session
        .list_projects()
        .await
        .expect("Session::list_projects send");

    let reply = tokio::time::timeout(Duration::from_secs(5), session.next_worker_reply())
        .await
        .expect("ProjectList timed out after 5s")
        .expect("recv channel closed before ProjectList arrived");

    match reply {
        daemon_client::WorkerReply::ProjectList { projects } => {
            assert!(
                !projects.is_empty(),
                "sandbox registry should return at least one synthetic project"
            );
        }
        other => panic!("expected ProjectList, got {other:?}"),
    }

    drop(session);
    // `handle` drops here, which aborts the daemon's iroh tasks —
    // sandbox tempdir cleans up after both halves are gone.
    drop(handle);
}
