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
use daemon_transport::{in_memory::pair, DialTarget, Session as ClientSession, SessionEvent};
use futures_core::Stream;

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
        archived: false,
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
        worktree: None,
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
    let harness = Harness::new_seeded(projects, tasks).await;
    let reply = harness
        .client
        .call(Control::ListProjects)
        .await
        .expect("ListProjects call");
    match reply {
        WorkerReply::ProjectList {
            projects, ui: _, ..
        } => {
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

/// Regression: a worktree task references its worktree-kind project via
/// `target_project_id`. If the wire projection filters out worktree
/// projects, the client's `absorb_projection` → `sanitize` path drops
/// the task because no projects_by_id entry matches `target_project_id`.
/// Symptom: desktop creates a worktree task; on the next absorb tick the
/// task disappears, and a later save() persists the wiped store to disk.
#[tokio::test]
async fn worktree_task_survives_projection_round_trip() {
    use another_one_core::project_store::{ProjectKind, TaskKind};
    let root = Project {
        id: "root".into(),
        repo_id: "repo1".into(),
        name: "root".into(),
        path: std::path::PathBuf::from("/tmp/root"),
        kind: ProjectKind::Root,
        archived: false,
        checkout: Default::default(),
        branch_settings: Default::default(),
        actions: Vec::new(),
        worktree_name: None,
        repo_common_dir: None,
    };
    let worktree = Project {
        id: "wt1".into(),
        repo_id: "repo1".into(),
        name: "root".into(),
        path: std::path::PathBuf::from("/tmp/root-wt"),
        kind: ProjectKind::Worktree,
        archived: false,
        checkout: Default::default(),
        branch_settings: Default::default(),
        actions: Vec::new(),
        worktree_name: Some("root-wt".into()),
        repo_common_dir: None,
    };
    let task = Task {
        id: "wt-task".into(),
        name: "wt task".into(),
        kind: TaskKind::Worktree,
        root_project_id: "root".into(),
        target_project_id: "wt1".into(),
        branch_name: "feat/wt".into(),
        section_id: "wt1::feat/wt::wt-task".into(),
        worktree: Some(another_one_core::project_store::TaskWorktree::from_project(
            &worktree,
        )),
        worktree_project_id: Some("wt1".into()),
        tabs: Vec::new(),
        active_tab_id: String::new(),
        next_tab_id: 0,
        cwd: None,
    };
    let mut harness = Harness::new_seeded(vec![root, worktree], vec![task]).await;
    let push = harness
        .expect_push(Duration::from_millis(250))
        .await
        .expect("initial push");
    let WorkerReply::ProjectList { projects, .. } = push else {
        panic!("expected ProjectList");
    };
    let target_present = projects.iter().any(|p| p.id == "wt1");
    assert!(
        !target_present,
        "worktree project should not appear as a top-level project"
    );
    let task_present = projects
        .iter()
        .flat_map(|p| p.tasks.iter())
        .any(|t| t.id == "wt-task" && t.target_project_id == "wt1" && t.worktree.is_some());
    assert!(task_present, "worktree task missing from projection");
    harness.shutdown();
}

// Silence unused-import warnings while the harness is being filled
// out — `Stream`/`DialTarget` come into use in commits 4–5.
#[allow(dead_code)]
fn _force_imports() {
    fn _take<S: Stream + Send>(_: S) {}
    let _: Option<DialTarget> = None;
}

// =====================================================================
// mod mutators — one test per mutating Control verb. Each test seeds a
// fresh harness, drains the initial-projection push, fires the verb,
// then drains the post-mutation push and asserts a specific projection
// delta. A verb that bypasses `with_store_mut` and skips the broadcast
// would fail this layer — exactly what surfaces the gaps the audit
// flagged for the unification refactor.
// =====================================================================

mod mutators {
    use super::*;

    /// Spin up a harness, drain the initial-projection push, return
    /// the harness ready to dispatch a single verb under test.
    async fn fresh_harness_after_initial_push() -> Harness {
        let (projects, tasks) = seed_one_project_one_task();
        let mut harness = Harness::new_seeded(projects, tasks).await;
        let _ = harness
            .expect_push(Duration::from_millis(250))
            .await
            .expect("initial projection push");
        harness
    }

    /// Fire `verb`, await its reply (so the dispatcher has finished
    /// the mutation), then drain the broadcast push that follows.
    async fn call_then_drain_push(harness: &mut Harness, verb: Control) -> WorkerReply {
        harness
            .client
            .call(verb)
            .await
            .expect("call returned daemon error");
        harness
            .expect_push(Duration::from_millis(250))
            .await
            .expect("post-mutation push within 250ms")
    }

    /// Convenience: extract `ProjectList` payload or panic.
    fn unwrap_projection(
        reply: WorkerReply,
    ) -> (Vec<daemon_proto::ProjectSummary>, daemon_proto::UiSnapshot) {
        match reply {
            WorkerReply::ProjectList { projects, ui, .. } => (projects, ui),
            other => panic!("expected ProjectList push, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn set_sidebar_git_metadata_visible_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetSidebarGitMetadataVisible { visible: true },
        )
        .await;
        let (_, ui) = unwrap_projection(reply);
        assert!(ui.show_sidebar_git_metadata);
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_shortcut_binding_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetShortcutBinding {
                action_id: "next-tab".into(),
                binding: "ctrl-shift-]".into(),
            },
        )
        .await;
        let (_, ui) = unwrap_projection(reply);
        let shortcuts = ui
            .shortcuts
            .as_ref()
            .expect("shortcuts present in projection");
        let mapping = shortcuts.as_object().expect("shortcuts is JSON object");
        assert!(
            mapping
                .iter()
                .any(|(_, v)| v.as_str() == Some("ctrl-shift-]")),
            "expected ctrl-shift-] binding somewhere in {mapping:?}"
        );
        harness.shutdown();
    }

    #[tokio::test]
    async fn reset_shortcut_binding_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::ResetShortcutBinding {
                action_id: "next-tab".into(),
            },
        )
        .await;
        let _ = unwrap_projection(reply);
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_git_commit_script_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetGitCommitScript {
                script: "test-commit-script".into(),
            },
        )
        .await;
        let (_, ui) = unwrap_projection(reply);
        assert_eq!(
            ui.git_commit_generation_script.as_deref(),
            Some("test-commit-script")
        );
        harness.shutdown();
    }

    #[tokio::test]
    async fn reset_git_commit_script_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let _ = call_then_drain_push(
            &mut harness,
            Control::SetGitCommitScript {
                script: "preset".into(),
            },
        )
        .await;
        let reply = call_then_drain_push(&mut harness, Control::ResetGitCommitScript).await;
        let (_, ui) = unwrap_projection(reply);
        assert!(
            ui.git_commit_generation_script
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true),
            "post-reset script should be empty/absent, got {:?}",
            ui.git_commit_generation_script
        );
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_git_pr_script_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetGitPrScript {
                script: "test-pr-script".into(),
            },
        )
        .await;
        let (_, ui) = unwrap_projection(reply);
        assert_eq!(
            ui.git_pr_generation_script.as_deref(),
            Some("test-pr-script")
        );
        harness.shutdown();
    }

    #[tokio::test]
    async fn reset_git_pr_script_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let _ = call_then_drain_push(
            &mut harness,
            Control::SetGitPrScript {
                script: "preset".into(),
            },
        )
        .await;
        let reply = call_then_drain_push(&mut harness, Control::ResetGitPrScript).await;
        let (_, ui) = unwrap_projection(reply);
        assert!(
            ui.git_pr_generation_script
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true),
            "post-reset script should be empty/absent, got {:?}",
            ui.git_pr_generation_script
        );
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_agent_enabled_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetAgentEnabled {
                agent_id: "claude-code".into(),
                enabled: false,
            },
        )
        .await;
        let (_, ui) = unwrap_projection(reply);
        // After explicit disable the daemon stores an allowlist with
        // claude-code excluded; check the wire's `enabled_agents`
        // doesn't contain the disabled id.
        let enabled = ui.enabled_agents.unwrap_or_default();
        assert!(
            !enabled.iter().any(|id| id == "claude-code"),
            "claude-code should be missing after disable: {enabled:?}"
        );
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_default_agent_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetDefaultAgent {
                agent_id: "codex".into(),
            },
        )
        .await;
        let (_, ui) = unwrap_projection(reply);
        assert_eq!(ui.default_agent_id.as_deref(), Some("codex"));
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_agent_launch_args_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetAgentLaunchArgs {
                agent_id: "claude-code".into(),
                args: vec!["--headless".into(), "--no-color".into()],
            },
        )
        .await;
        let (_, ui) = unwrap_projection(reply);
        let overrides = ui
            .agent_launch_args_overrides
            .as_ref()
            .expect("agent_launch_args_overrides present");
        let map = overrides
            .as_object()
            .expect("agent_launch_args_overrides is JSON object");
        let entry = map
            .get("claude-code")
            .and_then(|v| v.as_array())
            .expect("claude-code arg list present");
        assert_eq!(entry.len(), 2);
        assert_eq!(entry[0].as_str(), Some("--headless"));
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_git_commit_llm_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        // GitActionLlmSettings serializes as an object with an
        // optional "anthropic" or "openai" choice; we just round-trip
        // an empty object and verify the wire doesn't drop it.
        let settings = serde_json::json!({});
        let reply = call_then_drain_push(&mut harness, Control::SetGitCommitLlm { settings }).await;
        let (_, ui) = unwrap_projection(reply);
        // Wire field always present once set; existence is enough.
        assert!(ui.git_commit_generation_llm.is_some());
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_git_pr_llm_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let settings = serde_json::json!({});
        let reply = call_then_drain_push(&mut harness, Control::SetGitPrLlm { settings }).await;
        let (_, ui) = unwrap_projection(reply);
        assert!(ui.git_pr_generation_llm.is_some());
        harness.shutdown();
    }

    #[tokio::test]
    async fn remove_project_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::RemoveProject {
                project_id: "p1".into(),
            },
        )
        .await;
        let (projects, _) = unwrap_projection(reply);
        assert!(
            projects.iter().all(|p| p.id != "p1"),
            "p1 should be gone, got {projects:?}"
        );
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_task_pinned_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetTaskPinned {
                task_id: "t1".into(),
                pinned: true,
            },
        )
        .await;
        let (projects, ui) = unwrap_projection(reply);
        assert!(
            ui.pinned_task_ids.iter().any(|(_, t)| t == "t1"),
            "t1 should be in pinned_task_ids: {:?}",
            ui.pinned_task_ids
        );
        let task = projects[0]
            .tasks
            .iter()
            .find(|t| t.id == "t1")
            .expect("t1 present");
        assert!(task.pinned);
        harness.shutdown();
    }

    #[tokio::test]
    async fn rename_task_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::RenameTask {
                task_id: "t1".into(),
                new_name: "renamed".into(),
            },
        )
        .await;
        let (projects, _) = unwrap_projection(reply);
        let task = projects[0]
            .tasks
            .iter()
            .find(|t| t.id == "t1")
            .expect("t1 present");
        assert_eq!(task.name, "renamed");
        harness.shutdown();
    }

    #[tokio::test]
    async fn remove_task_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::RemoveTask {
                project_id: "p1".into(),
                task_id: "t1".into(),
            },
        )
        .await;
        let (projects, _) = unwrap_projection(reply);
        assert!(projects[0].tasks.is_empty(), "t1 should be removed");
        harness.shutdown();
    }

    #[tokio::test]
    async fn persist_section_state_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let persisted = serde_json::json!({
            "active_tab_id": "tab-a",
            "next_tab_id": 1,
            "cwd": null,
            "tabs": [{
                "id": "tab-a",
                "title": "Tab A",
                "pinned": false,
                "fixed_title": null,
                "provider": null,
                "launch_config": null,
                "restore_status": "ready",
                "failure_message": null,
                "failure_details": null,
            }]
        });
        let reply = call_then_drain_push(
            &mut harness,
            Control::PersistSectionState {
                section_id: "p1::main::t1".into(),
                persisted,
            },
        )
        .await;
        let (projects, _) = unwrap_projection(reply);
        let task = &projects[0].tasks[0];
        assert_eq!(task.active_tab_id, "tab-a");
        assert_eq!(task.tabs.len(), 1);
        assert_eq!(task.tabs[0].id, "tab-a");
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_last_active_section_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetLastActiveSection {
                section_id: Some("p1::main::t1".into()),
            },
        )
        .await;
        let (_, ui) = unwrap_projection(reply);
        assert_eq!(ui.last_active_section_id.as_deref(), Some("p1::main::t1"));
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_repo_default_commit_action_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        // No assertion on UI yet — the verb writes
        // `repo_default_commit_actions` but UiSnapshot doesn't carry
        // that field on the wire today. Verify the broadcast fires
        // (push arrives within 250ms) so a missing daemon route
        // would fail; richer assertion lands when the wire grows the
        // field.
        let _ = call_then_drain_push(
            &mut harness,
            Control::SetRepoDefaultCommitAction {
                repo_id: "p1".into(),
                action: "commit-and-push".into(),
            },
        )
        .await;
        harness.shutdown();
    }

    #[tokio::test]
    async fn update_task_branch_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::UpdateTaskBranch {
                task_id: "t1".into(),
                target_project_id: "p1".into(),
                branch_name: "feature".into(),
            },
        )
        .await;
        let (projects, _) = unwrap_projection(reply);
        let task = projects[0]
            .tasks
            .iter()
            .find(|t| t.id == "t1")
            .expect("t1 present");
        assert_eq!(task.branch_name, "feature");
        // `section_id` regenerates from the new branch name.
        assert!(
            task.section_id.contains("feature"),
            "section_id should encode new branch: {}",
            task.section_id
        );
        harness.shutdown();
    }

    #[tokio::test]
    async fn set_expanded_repos_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetExpandedRepos {
                expanded_repo_ids: vec!["p1".into()],
            },
        )
        .await;
        let (_, ui) = unwrap_projection(reply);
        assert!(
            ui.expanded_repo_ids.iter().any(|r| r == "p1"),
            "p1 should be in expanded_repo_ids: {:?}",
            ui.expanded_repo_ids
        );
        harness.shutdown();
    }

    // SetBranchSetting needs a real git repo at the project's path
    // (the daemon resolves branches via git), so seed a tempdir
    // initialised as a bare repo on the fly. Skip the real git init
    // — the verb just persists the override into the project's
    // branch_settings, which is what the wire mirrors.
    #[tokio::test]
    async fn set_branch_setting_emits_push() {
        let mut harness = fresh_harness_after_initial_push().await;
        let reply = call_then_drain_push(
            &mut harness,
            Control::SetBranchSetting {
                project_id: "p1".into(),
                field: "default-branch".into(),
                branch_name: Some("main".into()),
            },
        )
        .await;
        let (projects, _) = unwrap_projection(reply);
        let p = &projects[0];
        let bs = p
            .branch_settings
            .as_ref()
            .expect("branch_settings present in projection");
        let map = bs.as_object().expect("branch_settings is JSON object");
        // `core::ProjectBranchSettings` serializes overrides under a
        // map keyed by field id; existence is enough — the daemon
        // round-trip wrote *something*.
        assert!(!map.is_empty(), "branch_settings should be non-empty");
        harness.shutdown();
    }
}
