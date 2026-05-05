//! Integration tests that exercise `daemon::dispatch::serve_session`
//! against a real `DesktopTerminalRegistry` over a real
//! `RegistryState` + seeded `ProjectStore`, via the in-memory
//! transport pair. No GPUI, no Android, no iroh.
//!
//! Run with:
//! ```text
//! cargo test -p another-one --features test-harness --test daemon_dispatch_harness
//! ```
//!
//! The harness lives here (in `app/tests/`) rather than in the
//! `daemon` crate because `DesktopTerminalRegistry` and
//! `RegistryState` are defined in `app/src/daemon_host.rs`, and the
//! `daemon` crate doesn't depend on `app`. The `test-harness` feature
//! flag exposes a tiny `__test_harness` re-export from `app/src/lib.rs`
//! so we don't widen crate-wide visibility on `daemon_host` itself.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use another_one::__test_harness::{DesktopTerminalRegistry, RegistryState};
use another_one_core::project_store::{Project, ProjectStore, Task};
use daemon_proto::{Control, WorkerReply};
use daemon_transport::{
    in_memory::pair, DialTarget, Session as ClientSession, SessionEvent,
};
use futures_core::Stream;
use futures_util::StreamExt;

/// Test fixture wiring: `RegistryState` + `DesktopTerminalRegistry` +
/// `daemon_transport::in_memory::pair` + a tokio task running
/// `daemon::dispatch::serve_session`. Each `Harness` is independent.
pub struct Harness {
    pub registry_state: Arc<Mutex<RegistryState>>,
    pub registry: Arc<dyn daemon::registry::DaemonRegistry>,
    pub client: Box<dyn ClientSession>,
    pub server_join: tokio::task::JoinHandle<Result<(), daemon_transport::TransportError>>,
}

impl Harness {
    /// Wire a fresh harness with the given seeded projects + tasks.
    pub async fn new_seeded(projects: Vec<Project>, tasks: Vec<Task>) -> Self {
        let store = ProjectStore::from_projects_for_test(projects, tasks);
        let registry_state = Arc::new(Mutex::new(RegistryState::new(store)));
        let registry: Arc<dyn daemon::registry::DaemonRegistry> = Arc::new(
            DesktopTerminalRegistry::new(Arc::downgrade(&registry_state)),
        );

        let (server, client) = pair("test-peer");
        let server_arc: Arc<dyn daemon_transport::ServerSession> = Arc::from(server);
        let registry_for_serve = Arc::clone(&registry);
        let server_join = tokio::spawn(async move {
            daemon::dispatch::serve_session(server_arc, registry_for_serve).await
        });

        Self {
            registry_state,
            registry,
            client,
            server_join,
        }
    }

    /// Read the next `SessionEvent::Push(WorkerReply)` from the
    /// client's events stream, with timeout. Skips other event
    /// variants. Returns `None` on timeout.
    pub async fn expect_push(&mut self, timeout: Duration) -> Option<WorkerReply> {
        let mut stream = self.client.events();
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline - now;
            match tokio::time::timeout(remaining, futures_util::StreamExt::next(&mut stream)).await
            {
                Ok(Some(SessionEvent::Push(reply))) => return Some(reply),
                Ok(Some(_)) => continue,
                Ok(None) => return None,
                Err(_) => return None,
            }
        }
    }

    pub fn shutdown(self) {
        self.server_join.abort();
    }
}

/// Fixture: one root project with one task, no tabs. Cheap to build,
/// covers most read-side projection tests.
fn seed_one_project_one_task() -> (Vec<Project>, Vec<Task>) {
    use another_one_core::project_store::{ProjectKind, TaskKind};
    let project = Project {
        id: "p1".into(),
        repo_id: "p1".into(),
        name: "p1".into(),
        path: std::path::PathBuf::from("/tmp/p1"),
        kind: ProjectKind::Root,
        checkout: Default::default(),
        branch_settings: Default::default(),
        actions: Vec::new(),
        worktree_name: None,
        repo_common_dir: None,
    };
    let task = Task {
        id: "t1".into(),
        name: "task one".into(),
        kind: TaskKind::Direct,
        root_project_id: "p1".into(),
        target_project_id: "p1".into(),
        branch_name: "main".into(),
        section_id: "p1::main::t1".into(),
        worktree_project_id: None,
        tabs: Vec::new(),
        active_tab_id: String::new(),
        next_tab_id: 0,
        cwd: None,
    };
    (vec![project], vec![task])
}

#[tokio::test]
async fn harness_health_ok() {
    let (projects, tasks) = seed_one_project_one_task();
    let harness = Harness::new_seeded(projects, tasks).await;
    assert!(harness.registry.health().is_ok(), "registry health failed");
    harness.shutdown();
}

#[tokio::test]
async fn harness_round_trips_list_projects() {
    let (projects, tasks) = seed_one_project_one_task();
    let mut harness = Harness::new_seeded(projects, tasks).await;
    let reply = harness
        .client
        .call(Control::ListProjects)
        .await
        .expect("ListProjects call");
    match reply {
        WorkerReply::ProjectList { projects, ui: _ } => {
            assert_eq!(projects.len(), 1);
            assert_eq!(projects[0].id, "p1");
            assert_eq!(projects[0].tasks.len(), 1);
            assert_eq!(projects[0].tasks[0].id, "t1");
        }
        other => panic!("expected ProjectList, got {other:?}"),
    }
    harness.shutdown();
}

#[tokio::test]
#[ignore = "fails until commit 3 wires the initial-push at session connect"]
async fn boot_session_receives_initial_project_list_push() {
    let (projects, tasks) = seed_one_project_one_task();
    let mut harness = Harness::new_seeded(projects, tasks).await;
    let push = harness
        .expect_push(Duration::from_millis(250))
        .await
        .expect("initial push within 250ms of session connect");
    match push {
        WorkerReply::ProjectList { projects, .. } => {
            assert_eq!(projects.len(), 1);
            assert_eq!(projects[0].id, "p1");
        }
        other => panic!("expected initial ProjectList push, got {other:?}"),
    }
    harness.shutdown();
}

#[tokio::test]
#[ignore = "fails until commit 3 wires the Push arm in drain_session_events / initial-push at connect"]
async fn mutation_broadcast_reaches_session_events() {
    let (projects, tasks) = seed_one_project_one_task();
    let mut harness = Harness::new_seeded(projects, tasks).await;
    // Drain the initial push (arrives once commit 3 lands) so we
    // observe only the mutation-induced push below.
    let _initial = harness.expect_push(Duration::from_millis(250)).await;
    // Fire a write-only mutation that touches `ui`. Reply is
    // `WorkerReply::Empty`; the projection-changed broadcast follows.
    harness
        .client
        .call(Control::SetSidebarGitMetadataVisible { visible: true })
        .await
        .expect("SetSidebarGitMetadataVisible call");
    let push = harness
        .expect_push(Duration::from_millis(250))
        .await
        .expect("post-mutation push within 250ms");
    match push {
        WorkerReply::ProjectList { ui, .. } => {
            assert!(
                ui.show_sidebar_git_metadata,
                "post-mutation push should reflect ui.show_sidebar_git_metadata = true"
            );
        }
        other => panic!("expected ProjectList push, got {other:?}"),
    }
    harness.shutdown();
}

// Silence unused-import warnings while the harness is being filled
// out — `Stream`/`DialTarget` come into use in commits 4–5.
#[allow(dead_code)]
fn _force_imports() {
    fn _take<S: Stream + Send>(_: S) {}
    let _: Option<DialTarget> = None;
}
