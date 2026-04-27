//! Boot the embedded daemon in-process (Flutter desktop only).
//!
//! Replaces the GPUI binary's `desktop::daemon_host` for the future
//! Flutter desktop. The bridge:
//!
//!   1. Loads the on-disk `ProjectStore` and constructs a
//!      [`RegistryState`].
//!   2. Wraps it in a `BridgeDaemonRegistry` that mirrors
//!      `desktop::DesktopTerminalRegistry`'s semantics but reads
//!      from the bridge's own state (no `AnotherOneApp` to project
//!      from).
//!   3. Spawns a dedicated OS thread with a tokio runtime, blocks on
//!      [`daemon_sandbox::run_endpoint`], and on success registers
//!      the resulting [`EndpointHandle`] with both
//!      [`crate::local_pair`] and [`crate::local_registry`] so
//!      `LocalSession` + the pair-mobile FRB API can read from
//!      it.
//!
//! Host ownership is explicit: concurrent `boot_embedded_daemon`
//! calls collapse into one in-flight attempt, failed startup reopens
//! the boot slot, and shutdown drops the endpoint plus local
//! registry/pairing handoffs so a later boot can install fresh state.
//!
//! The local MCP transport is also started from this runtime on
//! macOS/Linux. It binds the daemon-owned Unix socket used by the
//! `another-one-mcp-shim` stdio bridge and dispatches against the
//! same shared registry state as the iroh endpoint.

use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::broadcast;

use another_one_core::agents::{AgentProviderKind, TerminalLaunchConfig};
use another_one_core::daemon_embed::{
    daemon_paths, key_from_wire, RegistryState, TabLaunchRequest,
};
use another_one_core::mcp::orchestrator::{
    McpOrchestrator, ProjectInfo, RunCommandRequest, RunCommandResponse, SpawnTaskRequest,
    SpawnTaskResponse, SpawnTerminalRequest, SpawnTerminalResponse, TabInfo, TaskInfo, TaskStatus,
    TerminalSnapshot, RUN_COMMAND_TIMEOUT_CEILING_MS,
};
use another_one_core::open_in::OpenInAppKind;
use another_one_core::platform::{CurrentPlatform, HeadlessPlatform};
use another_one_core::project_store::ProjectKind as CoreProjectKind;
use another_one_core::project_store::ProjectStore;
use another_one_core::project_store::{
    PersistedSectionState, PersistedTerminalTab, ProjectAction, ProjectActionAccess,
    ProjectActionIcon, ProjectActionKind, ProjectActionScope,
};
use another_one_core::section::SectionId;
use another_one_core::terminal_types::TerminalRuntimeKey;

use daemon_sandbox::frame::{
    ActiveGitStateWire, AgentProvider, AgentSettingsRowWire, AgentSettingsViewWire,
    AgentSummaryWire, BranchCompareFileWire, BranchCompareWire, ChangedFileWire, Check,
    CheckBucket, CommitWire, EnabledAgentsViewWire, GitActionScriptsView, McpCatalogEntryDto,
    McpServerDto, McpSettingsView, McpSourceDto, McpTransportKindDto, OpenInAppSettingsRowWire,
    OpenInAppWire, OpenInSettingsViewWire, OpenInStateWire, ProjectActionAccessWire,
    ProjectActionIconWire, ProjectActionKindWire, ProjectActionScopeWire, ProjectActionWire,
    ProjectKind, ProjectPagePullRequest, ProjectSummary, PullRequestState, PullRequestStatus,
    RecentCommitsWire, ResolvedBranchSettingsWire, ShortcutSettingsRow, ShortcutSettingsView,
    TabSummary, TaskSummary, ToolbarActionOutcome,
};
use daemon_sandbox::registry::{RegistryFuture, TabAttachment};
use daemon_sandbox::{DaemonRegistry, EndpointHandle};

use crate::local_pair::{clear_local_pair_info, set_local_pair_info, LocalPairInfo};
use crate::local_registry::{clear_local_registry, set_local_registry};

/// Process-local owner for the embedded daemon. The FRB API remains
/// process-global, but the daemon resources themselves live in this
/// replaceable host state rather than one-shot handoff cells.
static HOST: OnceLock<Mutex<EmbeddedDaemonHost>> = OnceLock::new();

/// Providers whose most recent MCP sync failed. Mirrors GPUI's
/// `mcp_last_sync_errors` column-level state so the Flutter settings
/// page can tint the affected provider chips red after a partial
/// `sync_all` failure.
static MCP_LAST_SYNC_ERRORS: OnceLock<Mutex<HashSet<AgentProviderKind>>> = OnceLock::new();

const MCP_TAB_REF_SEPARATOR: &str = "::tab::";
const RUN_COMMAND_DEFAULT_TIMEOUT_MS: u64 = 30_000;
const RUN_COMMAND_IDLE_MS: u64 = 750;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BootPhase {
    Idle,
    Booting,
    Booted,
}

#[derive(Debug)]
struct BootCoordinator {
    phase: BootPhase,
    last_error: Option<String>,
}

impl Default for BootCoordinator {
    fn default() -> Self {
        Self {
            phase: BootPhase::Idle,
            last_error: None,
        }
    }
}

impl BootCoordinator {
    fn try_start(&mut self) -> bool {
        match self.phase {
            BootPhase::Idle => {
                self.phase = BootPhase::Booting;
                self.last_error = None;
                true
            }
            BootPhase::Booting | BootPhase::Booted => false,
        }
    }

    fn mark_booted(&mut self) {
        self.phase = BootPhase::Booted;
        self.last_error = None;
    }

    fn mark_failed(&mut self, message: impl Into<String>) {
        self.phase = BootPhase::Idle;
        self.last_error = Some(message.into());
    }

    fn last_error(&self) -> Option<String> {
        self.last_error.clone()
    }

    #[cfg(test)]
    fn phase(&self) -> BootPhase {
        self.phase
    }
}

struct BootAttemptGuard {
    finished: bool,
}

impl BootAttemptGuard {
    fn new() -> Self {
        Self { finished: false }
    }

    fn mark_booted(&mut self) {
        with_boot_coordinator(|coordinator| coordinator.mark_booted());
        self.finished = true;
    }

    fn mark_failed(&mut self, message: impl Into<String>) {
        with_boot_coordinator(|coordinator| coordinator.mark_failed(message));
        self.finished = true;
    }
}

impl Drop for BootAttemptGuard {
    fn drop(&mut self) {
        if !self.finished {
            with_boot_coordinator(|coordinator| {
                coordinator.mark_failed("embedded daemon boot attempt ended before endpoint bind")
            });
        }
    }
}

#[derive(Default)]
struct EmbeddedDaemonHost {
    coordinator: BootCoordinator,
    registry_state: Option<Arc<Mutex<RegistryState>>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

fn with_host<R>(f: impl FnOnce(&mut EmbeddedDaemonHost) -> R) -> R {
    let host = HOST.get_or_init(|| Mutex::new(EmbeddedDaemonHost::default()));
    let mut guard = host.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

fn with_boot_coordinator<R>(f: impl FnOnce(&mut BootCoordinator) -> R) -> R {
    with_host(|host| f(&mut host.coordinator))
}

fn embedded_registry_state() -> Arc<Mutex<RegistryState>> {
    with_host(|host| {
        host.registry_state
            .get_or_insert_with(|| {
                let registry_state = Arc::new(Mutex::new(RegistryState::new(ProjectStore::load())));
                set_local_registry(registry_state.clone());
                crate::pty_drain::spawn_drain(registry_state.clone());
                registry_state
            })
            .clone()
    })
}

pub(crate) fn boot_error() -> Option<String> {
    with_boot_coordinator(|coordinator| coordinator.last_error())
}

pub(crate) fn shutdown() {
    let (shutdown_tx, thread) = with_host(|host| {
        host.coordinator = BootCoordinator::default();
        host.registry_state.take();
        (host.shutdown_tx.take(), host.thread.take())
    });
    if let Some(tx) = shutdown_tx {
        let _ = tx.send(());
    }
    if let Some(thread) = thread {
        if thread.thread().id() != thread::current().id() {
            let _ = thread.join();
        }
    }
    clear_local_pair_info();
    clear_local_registry();
}

/// Factory for the local MCP listener once `another-one-6gc.23.3`
/// moves listener startup into the embedded daemon boot path.
#[allow(dead_code)]
pub(crate) fn embedded_mcp_orchestrator() -> Arc<dyn McpOrchestrator> {
    let state = embedded_registry_state();
    Arc::new(BridgeMcpOrchestrator::new(Arc::downgrade(&state)))
}

fn with_mcp_last_sync_errors<R>(f: impl FnOnce(&mut HashSet<AgentProviderKind>) -> R) -> R {
    let errors = MCP_LAST_SYNC_ERRORS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut guard = errors
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

fn mcp_sync_error_provider_ids() -> Vec<String> {
    with_mcp_last_sync_errors(|errors| {
        let mut ids = errors
            .iter()
            .copied()
            .map(provider_id_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        ids.sort();
        ids
    })
}

fn record_mcp_sync_errors(
    report: &another_one_core::mcp::registry::SyncReport,
) -> Result<(), String> {
    let mut failures = Vec::new();
    with_mcp_last_sync_errors(|errors| {
        errors.clear();
        for (provider, result) in report {
            if let Err(err) = result {
                errors.insert(*provider);
                failures.push(format!(
                    "MCP sync failed for {}: {err}",
                    provider_id_str(*provider)
                ));
            }
        }
    });
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// Build, register, and boot the embedded daemon. A second call
/// while startup is already in flight or after a successful boot
/// no-ops. If startup later fails, the next call retries. Returns as
/// soon as the registry is wired and the daemon thread is spawned;
/// the endpoint handshake completes asynchronously on its own
/// runtime, so [`crate::api::pair::pairing_info`] may return `None`
/// for a few hundred milliseconds after this returns. The pair-mobile
/// UI's empty state covers that window.
pub(crate) fn boot() -> Result<(), String> {
    let registry_state = embedded_registry_state();

    if !with_boot_coordinator(|coordinator| coordinator.try_start()) {
        return Ok(());
    }

    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("another-one-embedded-daemon".into())
        .spawn(move || run(registry_state, shutdown_rx))
        .map_err(|e| {
            let message = format!("spawn embedded daemon thread: {e}");
            with_boot_coordinator(|coordinator| coordinator.mark_failed(message.clone()));
            message
        })?;

    with_host(|host| {
        host.shutdown_tx = Some(shutdown_tx);
        host.thread = Some(thread);
    });

    Ok(())
}

fn run(registry_state: Arc<Mutex<RegistryState>>, shutdown_rx: mpsc::Receiver<()>) {
    let mut boot_attempt = BootAttemptGuard::new();
    // Mirrors `desktop::daemon_host::run`. Four workers: a single
    // stuck PTY write under `block_in_place` shouldn't be able to
    // starve the accept loop + writers + forwarders all at once.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .thread_name("another-one-embedded-rt")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let message = format!("build embedded daemon runtime: {e:#}");
            tracing::error!("{message}");
            boot_attempt.mark_failed(message);
            return;
        }
    };

    let weak = Arc::downgrade(&registry_state);
    drop(registry_state);
    let registry: Arc<dyn DaemonRegistry> = Arc::new(BridgeDaemonRegistry::new(weak));

    let mcp_listener = runtime.block_on(async {
        daemon_sandbox::transport_mcp::spawn(
            daemon_sandbox::transport_mcp::default_socket_path(),
            embedded_mcp_orchestrator(),
        )
    });
    let _mcp_listener = match mcp_listener {
        Ok(listener) => Some(listener),
        Err(e) => {
            let message = format!("embedded daemon MCP listener failed to start: {e:#}");
            tracing::error!("{message}");
            boot_attempt.mark_failed(message);
            return;
        }
    };

    let paths = match daemon_paths() {
        Ok(p) => p,
        Err(e) => {
            let message = format!("resolve daemon paths: {e:#}");
            tracing::error!("{message}");
            boot_attempt.mark_failed(message);
            return;
        }
    };

    // Loopback self-trust (`another-one-ojm.9`): the desktop's UI
    // layer dials this same daemon over iroh, so the daemon needs to
    // recognise its own loopback client as already-paired and skip the
    // TOFU Hello dance — otherwise every cold boot would burn the
    // user-facing pair nonce on the in-process self-dial, leaving no
    // valid nonce for an actual mobile pair scan.
    //
    // Resolve the device's NodeId from the same iroh secret key
    // `iroh_connect` will use, then append it to `paths.paired_peers`.
    // Idempotent: `peer_status` short-circuits on the first match, so
    // repeated boots don't bloat the file with the same id.
    match crate::api::iroh_client::load_or_create_device_secret_key() {
        Ok(sk) => {
            let device_node_id = sk.public().to_string();
            if let Err(e) = daemon_sandbox::persist_pairing(&device_node_id, &paths.paired_peers) {
                let message = format!(
                    "loopback self-trust: persist_pairing failed \
                     (device_node_id={device_node_id}): {e:#}"
                );
                tracing::error!("{message}");
                boot_attempt.mark_failed(message);
                return;
            } else {
                tracing::info!(
                    "loopback self-trust: pre-allowlisted device NodeId {} in paired_peers",
                    device_node_id,
                );
            }
        }
        Err(e) => {
            let message = format!("loopback self-trust: could not load device secret key: {e:#}");
            tracing::error!("{message}");
            boot_attempt.mark_failed(message);
            return;
        }
    }

    let endpoint_result = runtime.block_on(async {
        daemon_sandbox::run_endpoint(registry, paths.secret_key, paths.paired_peers).await
    });

    match endpoint_result {
        Ok(handle) => {
            let handle = Arc::new(handle);
            let adapter: Arc<dyn LocalPairInfo> =
                Arc::new(EndpointHandlePairAdapter::new(handle.clone()));
            set_local_pair_info(adapter);
            boot_attempt.mark_booted();
            let _ = shutdown_rx.recv();
            drop(handle);
        }
        Err(e) => {
            let message = format!("embedded daemon boot failed: {e:#}");
            tracing::error!("{message}");
            boot_attempt.mark_failed(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        close_tab_in_section, launch_config_for_add_agent, shutdown, toggle_tab_pinned_in_section,
        with_boot_coordinator, BootCoordinator, BootPhase,
    };
    use another_one_core::agents::TerminalRestoreStatus;
    use another_one_core::project_store::{PersistedSectionState, PersistedTerminalTab};

    fn sample_tab(id: &str, pinned: bool) -> PersistedTerminalTab {
        PersistedTerminalTab {
            id: id.to_string(),
            title: id.to_string(),
            pinned,
            fixed_title: None,
            provider: None,
            launch_config: None,
            restore_status: TerminalRestoreStatus::NotStarted,
            failure_message: None,
            failure_details: None,
        }
    }

    #[test]
    fn try_start_should_only_claim_idle_boot_slot() {
        let mut coordinator = BootCoordinator::default();

        assert!(coordinator.try_start());
        assert_eq!(coordinator.phase(), BootPhase::Booting);
        assert!(!coordinator.try_start());
    }

    #[test]
    fn mark_failed_should_reopen_boot_slot_after_startup_failure() {
        let mut coordinator = BootCoordinator::default();

        assert!(coordinator.try_start());
        coordinator.mark_failed("bind failed");

        assert_eq!(coordinator.phase(), BootPhase::Idle);
        assert_eq!(coordinator.last_error().as_deref(), Some("bind failed"));
        assert!(coordinator.try_start());
        assert_eq!(coordinator.last_error(), None);
    }

    #[test]
    fn shutdown_should_reset_global_host_state() {
        with_boot_coordinator(|coordinator| {
            assert!(coordinator.try_start());
            coordinator.mark_booted();
        });

        shutdown();

        assert_eq!(
            with_boot_coordinator(|coordinator| coordinator.phase()),
            BootPhase::Idle
        );
    }

    #[test]
    fn mark_booted_should_keep_subsequent_boot_calls_noop() {
        let mut coordinator = BootCoordinator::default();

        assert!(coordinator.try_start());
        coordinator.mark_booted();

        assert_eq!(coordinator.phase(), BootPhase::Booted);
        assert!(!coordinator.try_start());
    }

    #[test]
    fn launch_config_for_add_agent_uses_shell_for_empty_selection() {
        let config = launch_config_for_add_agent("").expect("default shell config");

        assert_eq!(config.provider, None);
    }

    #[test]
    fn launch_config_for_add_agent_rejects_unknown_agent_id() {
        let err = launch_config_for_add_agent("missing").expect_err("unknown agent should fail");

        assert!(err.contains("unknown agent_id"));
    }

    #[test]
    fn close_tab_in_section_returns_empty_when_last_tab_is_removed() {
        let mut section = PersistedSectionState {
            active_tab_id: "tab-1".to_string(),
            next_tab_id: 2,
            cwd: None,
            tabs: vec![sample_tab("tab-1", false)],
        };

        let next = close_tab_in_section(&mut section, "tab-1");

        assert_eq!(next.as_deref(), Some(""));
        assert!(section.tabs.is_empty());
        assert!(section.active_tab_id.is_empty());
    }

    #[test]
    fn close_tab_in_section_keeps_neighbor_active_when_active_tab_is_removed() {
        let mut section = PersistedSectionState {
            active_tab_id: "b".to_string(),
            next_tab_id: 4,
            cwd: None,
            tabs: vec![
                sample_tab("a", false),
                sample_tab("b", false),
                sample_tab("c", false),
            ],
        };

        let next = close_tab_in_section(&mut section, "b");

        assert_eq!(next.as_deref(), Some("c"));
        assert_eq!(section.active_tab_id, "c");
        assert_eq!(
            section
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "c"]
        );
    }

    #[test]
    fn toggle_tab_pinned_in_section_reorders_without_losing_active_tab() {
        let mut section = PersistedSectionState {
            active_tab_id: "active".to_string(),
            next_tab_id: 4,
            cwd: None,
            tabs: vec![
                sample_tab("pinned", true),
                sample_tab("active", false),
                sample_tab("plain", false),
            ],
        };

        let pinned = toggle_tab_pinned_in_section(&mut section, "active");

        assert_eq!(pinned, Some(true));
        assert_eq!(section.active_tab_id, "active");
        assert_eq!(
            section
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["pinned", "active", "plain"]
        );
        assert!(section.tabs[1].pinned);
    }
}

/// Adapts an `EndpointHandle` to `LocalPairInfo`. Splitting the
/// trait off the concrete handle is the seam that lets the bridge
/// expose pair info without leaking `daemon_sandbox` types into
/// `crate::local_pair` (which `api/pair.rs` depends on).
struct EndpointHandlePairAdapter {
    handle: Arc<EndpointHandle>,
}

impl EndpointHandlePairAdapter {
    fn new(handle: Arc<EndpointHandle>) -> Self {
        Self { handle }
    }
}

impl LocalPairInfo for EndpointHandlePairAdapter {
    fn pairing_url(&self) -> String {
        self.handle.pairing_url()
    }

    fn qr_png_bytes(&self) -> Vec<u8> {
        self.handle.qr_png_bytes()
    }

    fn regenerate_pairing(&self) -> Result<(), String> {
        self.handle
            .regenerate_pairing()
            .map_err(|e| format!("{e:#}"))
    }

    fn endpoint_id(&self) -> String {
        self.handle.endpoint_id.clone()
    }

    fn direct_addrs(&self) -> Vec<String> {
        self.handle.direct_addrs()
    }

    fn relay_urls(&self) -> Vec<String> {
        self.handle.relay_urls()
    }
}

/// `DaemonRegistry` impl that operates directly on the bridge's
/// `RegistryState`. Mirrors `desktop::DesktopTerminalRegistry` but
/// without the desktop's project-summary projection logic — that
/// will return when the Flutter desktop port owns the project tree
/// directly. For now `list_projects` flattens the in-memory store
/// the same way `LocalSession::list_projects` does (see
/// `api/local_session.rs::flatten_project_store`).
struct BridgeDaemonRegistry {
    inner: Weak<Mutex<RegistryState>>,
}

impl BridgeDaemonRegistry {
    fn new(inner: Weak<Mutex<RegistryState>>) -> Self {
        Self { inner }
    }

    fn lock_state(&self) -> Result<Arc<Mutex<RegistryState>>, String> {
        let arc = self
            .inner
            .upgrade()
            .ok_or_else(|| "embedded daemon registry state has been dropped".to_string())?;
        {
            let _guard = arc
                .lock()
                .map_err(|_| "embedded daemon registry mutex is poisoned".to_string())?;
        }
        Ok(arc)
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut RegistryState) -> R) -> Option<R> {
        let arc = self.inner.upgrade()?;
        let mut guard = arc.lock().ok()?;
        Some(f(&mut guard))
    }

    fn mark_writer_failed(
        &self,
        key: &TerminalRuntimeKey,
        message: &'static str,
        err: anyhow::Error,
    ) {
        let details = format!("{err:#}");
        tracing::warn!(
            section_id = %key.section_id.store_key(),
            tab_id = %key.tab_id,
            message = message,
            details = details.as_str(),
            "terminal writer failed"
        );
        self.with_state(|state| state.fail_tab_io(key, message, details));
    }
}

struct BridgeMcpOrchestrator {
    inner: Weak<Mutex<RegistryState>>,
}

impl BridgeMcpOrchestrator {
    fn new(inner: Weak<Mutex<RegistryState>>) -> Self {
        Self { inner }
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut RegistryState) -> R) -> anyhow::Result<R> {
        let arc = self
            .inner
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("mcp orchestrator: registry state dropped"))?;
        let mut guard = arc
            .lock()
            .map_err(|_| anyhow::anyhow!("mcp orchestrator: registry mutex poisoned"))?;
        Ok(f(&mut guard))
    }

    fn with_state_opt<R>(&self, f: impl FnOnce(&mut RegistryState) -> R) -> Option<R> {
        let arc = self.inner.upgrade()?;
        let mut guard = arc.lock().ok()?;
        Some(f(&mut guard))
    }

    fn mark_writer_failed(
        &self,
        key: &TerminalRuntimeKey,
        message: &'static str,
        err: &anyhow::Error,
    ) {
        let details = format!("{err:#}");
        tracing::warn!(
            section_id = %key.section_id.store_key(),
            tab_id = %key.tab_id,
            message = message,
            details = details.as_str(),
            "terminal writer failed"
        );
        let _ = self.with_state(|state| state.fail_tab_io(key, message, details));
    }
}

impl McpOrchestrator for BridgeMcpOrchestrator {
    fn list_projects(&self) -> Vec<ProjectInfo> {
        self.with_state_opt(|state| {
            state
                .project_store
                .projects
                .iter()
                .map(|project| ProjectInfo {
                    id: project.id.clone(),
                    path: project.path.display().to_string(),
                    label: project.name.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn list_tasks(&self) -> Vec<TaskInfo> {
        self.with_state_opt(|state| {
            state
                .project_store
                .tasks
                .values()
                .flatten()
                .map(|task| TaskInfo {
                    project_id: task.root_project_id.clone(),
                    task_id: task.id.clone(),
                    branch: (task.kind != another_one_core::project_store::TaskKind::Direct)
                        .then(|| task.branch_name.clone()),
                    worktree_path: task
                        .worktree_project_id
                        .as_deref()
                        .and_then(|project_id| state.project_store.project(project_id))
                        .map(|project| project.path.display().to_string()),
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn list_tabs(&self, task_id: &str) -> Vec<TabInfo> {
        self.with_state_opt(|state| {
            let Some(task) = state.project_store.task(task_id) else {
                return Vec::new();
            };
            let Some(section) = state.project_store.terminal_sections.get(&task.section_id) else {
                return Vec::new();
            };
            section
                .tabs
                .iter()
                .map(|tab| TabInfo {
                    tab_id: mcp_tab_ref(&task.section_id, &tab.id),
                    provider: tab.provider.map(provider_id_str).map(str::to_string),
                    title: tab.title.clone(),
                    session_ref: tab
                        .launch_config
                        .as_ref()
                        .and_then(|config| config.session.as_ref())
                        .map(|session| session.id.clone()),
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn get_task_status(&self, task_id: &str) -> Option<TaskStatus> {
        self.with_state_opt(|state| {
            let task = state.project_store.task(task_id)?;
            let section = state
                .project_store
                .terminal_sections
                .get(&task.section_id)?;
            if section.tabs.is_empty() {
                return Some(TaskStatus::NoTabs);
            }

            let working = section.tabs.iter().any(|tab| {
                let Some(key) = key_from_wire(&task.section_id, &tab.id) else {
                    return false;
                };
                state.writers.contains_key(&key)
                    || state.broadcasts.contains_key(&key)
                    || state.in_flight_launches.contains(&key)
                    || state
                        .pending_tab_launches
                        .iter()
                        .any(|request| request.key == key)
                    || state.pending_post_launch_input.contains_key(&key)
            });

            Some(if working {
                TaskStatus::Working
            } else {
                TaskStatus::Idle
            })
        })
        .flatten()
    }

    fn read_terminal_output(&self, tab_id: &str, tail: usize) -> Option<TerminalSnapshot> {
        self.with_state_opt(|state| {
            let tab = resolve_mcp_tab_ref(state, tab_id)?;
            let (bytes, truncated_head) = state
                .terminal_replay
                .get(&tab.key)
                .map(|replay| replay.tail_bytes(tail))
                .unwrap_or((Vec::new(), false));
            Some(TerminalSnapshot {
                bytes,
                truncated_head,
            })
        })
        .flatten()
    }

    fn spawn_task(&self, req: SpawnTaskRequest) -> anyhow::Result<SpawnTaskResponse> {
        let provider = parse_provider_id(&req.harness)
            .ok_or_else(|| anyhow::anyhow!("spawn_task: unknown harness `{}`", req.harness))?;
        let launch_config = TerminalLaunchConfig::for_provider(provider);
        let initial_input = req.initial_prompt.as_deref().map(terminal_input_line);

        match req.branch {
            Some(branch) => self.spawn_worktree_task(
                req.project_id,
                branch,
                req.title,
                launch_config,
                initial_input,
            ),
            None => self.spawn_direct_task(req.project_id, req.title, launch_config, initial_input),
        }
    }

    fn spawn_terminal(&self, req: SpawnTerminalRequest) -> anyhow::Result<SpawnTerminalResponse> {
        match (req.project_id, req.task_id) {
            (Some(project_id), None) => {
                let inserted = self.with_state(|state| {
                    let project = state.project_store.project(&project_id).cloned();
                    let Some(project) = project else {
                        return Err(anyhow::anyhow!(
                            "spawn_terminal: unknown project_id `{project_id}`"
                        ));
                    };
                    let root_project_id = state
                        .project_store
                        .root_project_id_for_project(&project.id)
                        .unwrap_or_else(|| project.id.clone());
                    let branch_name =
                        another_one_core::project_store::current_branch(&project.path)
                            .or_else(|| state.project_store.current_branch_name(&project.id))
                            .unwrap_or_else(|| "main".to_string());
                    let cwd = req
                        .cwd
                        .as_deref()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| project.path.clone());
                    Ok(insert_task_with_initial_tab_details(
                        state,
                        root_project_id,
                        project.id.clone(),
                        another_one_core::project_store::TaskKind::Direct,
                        "Terminal".to_string(),
                        branch_name,
                        None,
                        cwd,
                        TerminalLaunchConfig::default(),
                    ))
                })??;
                Ok(SpawnTerminalResponse {
                    tab_id: mcp_tab_ref(&inserted.section_key, &inserted.tab_id),
                })
            }
            (None, Some(task_id)) => {
                let tab_ref = self.with_state(|state| {
                    let task = state.project_store.task(&task_id).cloned().ok_or_else(|| {
                        anyhow::anyhow!("spawn_terminal: unknown task_id `{task_id}`")
                    })?;
                    let section_id =
                        SectionId::from_store_key(&task.section_id).ok_or_else(|| {
                            anyhow::anyhow!(
                                "spawn_terminal: malformed section id `{}`",
                                task.section_id
                            )
                        })?;
                    let mut section = state
                        .project_store
                        .terminal_sections
                        .get(&task.section_id)
                        .cloned()
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "spawn_terminal: missing terminal section `{}`",
                                task.section_id
                            )
                        })?;
                    if let Some(cwd) = req.cwd.as_deref() {
                        section.cwd = Some(PathBuf::from(cwd));
                    }
                    let (tab_id, tab) = build_terminal_tab(TerminalLaunchConfig::default(), None);
                    append_tab_to_section(&mut section, tab);
                    state
                        .project_store
                        .set_section_state(task.section_id.clone(), section);
                    state.pending_tab_launches.push(TabLaunchRequest {
                        key: TerminalRuntimeKey {
                            section_id,
                            tab_id: tab_id.clone(),
                        },
                    });
                    Ok::<_, anyhow::Error>(mcp_tab_ref(&task.section_id, &tab_id))
                })??;
                Ok(SpawnTerminalResponse { tab_id: tab_ref })
            }
            (None, None) => anyhow::bail!("spawn_terminal requires project_id or task_id"),
            (Some(_), Some(_)) => anyhow::bail!("spawn_terminal accepts project_id XOR task_id"),
        }
    }

    fn send_input(&self, tab_id: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let (key, writer) = self.with_state(|state| {
            let tab = resolve_mcp_tab_ref(state, tab_id)
                .ok_or_else(|| anyhow::anyhow!("send_input: unknown tab_id `{tab_id}`"))?;
            let writer = state
                .writers
                .get(&tab.key)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("send_input: tab `{tab_id}` has no live PTY"))?;
            Ok::<_, anyhow::Error>((tab.key, writer))
        })??;
        if let Err(err) = write_tab_bytes(writer, bytes) {
            self.mark_writer_failed(&key, "Terminal input failed", &err);
            return Err(err);
        }
        Ok(())
    }

    fn run_command(&self, req: RunCommandRequest) -> anyhow::Result<RunCommandResponse> {
        if req.command.trim().is_empty() {
            anyhow::bail!("run_command: command must not be blank");
        }
        let target =
            self.resolve_run_command_target(req.tab_id.as_deref(), req.task_id.as_deref())?;
        let start_sequence = self.with_state(|state| {
            state
                .terminal_replay
                .get(&target.key)
                .and_then(|replay| replay.latest_sequence())
        })?;
        let writer = self.with_state(|state| {
            state
                .writers
                .get(&target.key)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("run_command: tab has no live PTY"))
        })??;
        if let Err(err) = write_tab_bytes(writer, &terminal_input_line(&req.command)) {
            self.mark_writer_failed(&target.key, "Terminal command input failed", &err);
            return Err(err);
        }

        let timeout_ms = req
            .timeout_ms
            .unwrap_or(RUN_COMMAND_DEFAULT_TIMEOUT_MS)
            .min(RUN_COMMAND_TIMEOUT_CEILING_MS);
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let idle = Duration::from_millis(RUN_COMMAND_IDLE_MS);
        let mut last_seen = start_sequence;
        let mut last_activity = Instant::now();
        let mut output = Vec::new();

        loop {
            let chunks = self.with_state(|state| {
                state
                    .terminal_replay
                    .get(&target.key)
                    .map(|replay| (replay.replay_after(last_seen), replay.latest_sequence()))
                    .unwrap_or_default()
            })?;

            if !chunks.0.is_empty() {
                last_seen = chunks.1;
                last_activity = Instant::now();
                for chunk in chunks.0 {
                    output.extend(chunk);
                }
            }

            let now = Instant::now();
            if now.duration_since(last_activity) >= idle {
                return Ok(RunCommandResponse {
                    output,
                    timed_out: false,
                });
            }
            if now >= deadline {
                return Ok(RunCommandResponse {
                    output,
                    timed_out: true,
                });
            }

            thread::sleep(Duration::from_millis(50));
        }
    }

    fn close_tab(&self, tab_id: &str) -> anyhow::Result<()> {
        self.with_state(|state| {
            let tab = resolve_mcp_tab_ref(state, tab_id)
                .ok_or_else(|| anyhow::anyhow!("close_tab: unknown tab_id `{tab_id}`"))?;
            let mut section = state
                .project_store
                .terminal_sections
                .get(&tab.section_key)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("close_tab: missing terminal section `{}`", tab.section_key)
                })?;
            close_tab_in_section(&mut section, &tab.key.tab_id)
                .ok_or_else(|| anyhow::anyhow!("close_tab: unknown tab_id `{tab_id}`"))?;
            state
                .project_store
                .set_section_state(tab.section_key, section);
            state.pending_tab_terminations.push(tab.key);
            Ok(())
        })?
    }
}

impl BridgeMcpOrchestrator {
    fn spawn_direct_task(
        &self,
        project_id: String,
        title: Option<String>,
        launch_config: TerminalLaunchConfig,
        initial_input: Option<Vec<u8>>,
    ) -> anyhow::Result<SpawnTaskResponse> {
        let inserted = self.with_state(|state| {
            let root_project_id = state
                .project_store
                .root_project_id_for_project(&project_id)
                .ok_or_else(|| anyhow::anyhow!("spawn_task: unknown project_id `{project_id}`"))?;
            let project = state
                .project_store
                .project(&root_project_id)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("spawn_task: unknown root project `{root_project_id}`")
                })?;
            let branch_name = another_one_core::project_store::current_branch(&project.path)
                .or_else(|| state.project_store.current_branch_name(&project.id))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "spawn_task: could not determine current branch for `{}`",
                        project.name
                    )
                })?;
            let task_name = task_title_or_default(title.as_deref(), &branch_name);
            let inserted = insert_task_with_initial_tab_details(
                state,
                root_project_id.clone(),
                project.id.clone(),
                another_one_core::project_store::TaskKind::Direct,
                task_name,
                branch_name,
                None,
                project.path.clone(),
                launch_config,
            );
            queue_initial_tab_input(state, &inserted, initial_input);
            Ok::<_, anyhow::Error>(inserted)
        })??;

        Ok(SpawnTaskResponse {
            project_id: inserted.root_project_id,
            task_id: inserted.task_id,
            worktree_path: None,
            tab_id: mcp_tab_ref(&inserted.section_key, &inserted.tab_id),
        })
    }

    fn spawn_worktree_task(
        &self,
        project_id: String,
        branch: String,
        title: Option<String>,
        launch_config: TerminalLaunchConfig,
        initial_input: Option<Vec<u8>>,
    ) -> anyhow::Result<SpawnTaskResponse> {
        let branch = branch.trim().to_string();
        if branch.is_empty() {
            anyhow::bail!("spawn_task: branch must not be blank for worktree tasks");
        }

        let (root_project_id, project_path, project_name, source_branch) =
            self.with_state(|state| {
                let root_project_id = state
                    .project_store
                    .root_project_id_for_project(&project_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("spawn_task: unknown project_id `{project_id}`")
                    })?;
                let project = state
                    .project_store
                    .project(&root_project_id)
                    .cloned()
                    .ok_or_else(|| {
                        anyhow::anyhow!("spawn_task: unknown root project `{root_project_id}`")
                    })?;
                let source_branch = another_one_core::project_store::current_branch(&project.path)
                    .or_else(|| state.project_store.current_branch_name(&project.id))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "spawn_task: could not determine source branch for `{}`",
                            project.name
                        )
                    })?;
                Ok::<_, anyhow::Error>((
                    root_project_id,
                    project.path.clone(),
                    project.name.clone(),
                    source_branch,
                ))
            })??;

        let task_name = task_title_or_default(title.as_deref(), &branch);
        let mut rx = another_one_core::project_service::spawn_task_creation(
            root_project_id.clone(),
            project_path,
            project_name,
            task_name,
            branch.clone(),
            another_one_core::project_store::TaskWorktreeBranchMode::NewBranchFrom {
                source_branch,
            },
            launch_config,
        );
        let reply = wait_for_task_creation(&mut rx, "spawn_task")?;
        let success = reply
            .result
            .map_err(|failure| anyhow::anyhow!("spawn_task: {}", failure.message))?;

        let (inserted, worktree_path) = self.with_state(|state| {
            let inserted_worktree = state
                .project_store
                .insert_prepared_project(success.project.clone());
            let worktree_project_id = if inserted_worktree {
                success.project.project.id.clone()
            } else {
                state
                    .project_store
                    .projects
                    .iter()
                    .find(|project| project.path == success.project.project.path)
                    .map(|project| project.id.clone())
                    .unwrap_or_else(|| success.project.project.id.clone())
            };
            let inserted = insert_task_with_initial_tab_details(
                state,
                root_project_id.clone(),
                worktree_project_id.clone(),
                another_one_core::project_store::TaskKind::Worktree,
                success.task_name,
                success.branch_name,
                Some(worktree_project_id),
                success.project.project.path.clone(),
                success.launch_config,
            );
            queue_initial_tab_input(state, &inserted, initial_input);
            Ok::<_, anyhow::Error>((inserted, success.project.project.path))
        })??;

        Ok(SpawnTaskResponse {
            project_id: inserted.root_project_id,
            task_id: inserted.task_id,
            worktree_path: Some(worktree_path.display().to_string()),
            tab_id: mcp_tab_ref(&inserted.section_key, &inserted.tab_id),
        })
    }

    fn resolve_run_command_target(
        &self,
        tab_id: Option<&str>,
        task_id: Option<&str>,
    ) -> anyhow::Result<McpResolvedTab> {
        match (tab_id, task_id) {
            (Some(tab_id), None) => self
                .with_state(|state| resolve_mcp_tab_ref(state, tab_id))?
                .ok_or_else(|| anyhow::anyhow!("run_command: unknown tab_id `{tab_id}`")),
            (None, Some(task_id)) => self
                .with_state(|state| {
                    let task = state.project_store.task(task_id)?;
                    let section = state
                        .project_store
                        .terminal_sections
                        .get(&task.section_id)?;
                    let tab_id = if section.active_tab_id.is_empty() {
                        section.tabs.first()?.id.as_str()
                    } else {
                        section.active_tab_id.as_str()
                    };
                    let key = key_from_wire(&task.section_id, tab_id)?;
                    Some(McpResolvedTab {
                        section_key: task.section_id.clone(),
                        key,
                    })
                })?
                .ok_or_else(|| anyhow::anyhow!("run_command: unknown task_id `{task_id}`")),
            (None, None) => anyhow::bail!("run_command requires tab_id or task_id"),
            (Some(_), Some(_)) => anyhow::bail!("run_command accepts tab_id XOR task_id"),
        }
    }
}

impl DaemonRegistry for BridgeDaemonRegistry {
    fn health(&self) -> Result<(), String> {
        self.lock_state().map(|_| ())
    }

    fn list_projects(&self) -> Vec<ProjectSummary> {
        // Project flattening mirrors `LocalSession::list_projects`'s
        // `flatten_project_store`. Worktree-kind projects collapse
        // into their root via `Task::target_project_id`; mobile sees
        // the same tree the desktop sidebar does.
        self.with_state(|state| flatten_state_to_frame(state))
            .unwrap_or_default()
    }

    fn attach_tab(&self, section_id: &str, tab_id: &str) -> Option<broadcast::Receiver<Vec<u8>>> {
        let key = key_from_wire(section_id, tab_id)?;
        self.with_state(|state| state.broadcasts.get(&key).map(|tx| tx.subscribe()))
            .flatten()
    }

    fn attach_tab_with_replay(
        &self,
        viewer_id: &str,
        section_id: &str,
        tab_id: &str,
    ) -> Option<TabAttachment> {
        let key = key_from_wire(section_id, tab_id)?;
        self.with_state(|state| {
            let last_seen = state
                .viewer_output_cursors
                .get(&(viewer_id.to_string(), key.clone()))
                .copied();
            let replay = state
                .terminal_replay
                .get(&key)
                .map(|buffer| buffer.replay_after(last_seen))
                .unwrap_or_default();
            state.broadcasts.get(&key).map(|tx| TabAttachment {
                replay,
                receiver: tx.subscribe(),
            })
        })
        .flatten()
    }

    fn note_tab_output_observed(&self, viewer_id: &str, section_id: &str, tab_id: &str) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        self.with_state(|state| {
            let Some(sequence) = state
                .terminal_replay
                .get(&key)
                .and_then(|buffer| buffer.latest_sequence())
            else {
                return;
            };
            state
                .viewer_output_cursors
                .insert((viewer_id.to_string(), key), sequence);
        });
    }

    fn tab_input(&self, section_id: &str, tab_id: &str, bytes: &[u8]) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        // Clone writer Arc out, drop state lock, then write — same
        // rationale as desktop::DesktopTerminalRegistry::tab_input.
        let writer = self
            .with_state(|state| state.writers.get(&key).cloned())
            .flatten();
        let Some(writer) = writer else { return };
        let result = tokio::task::block_in_place(|| write_tab_bytes(writer, bytes));
        if let Err(err) = result {
            self.mark_writer_failed(&key, "Terminal input failed", err);
        }
    }

    fn tab_resize(&self, viewer_id: &str, section_id: &str, tab_id: &str, cols: u16, rows: u16) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        self.with_state(|state| {
            if let Some(old_key) = state.viewer_focus.get(viewer_id).cloned() {
                if old_key != key {
                    if let Some(map) = state.active_viewers.get_mut(&old_key) {
                        map.remove(viewer_id);
                        if map.is_empty() {
                            state.active_viewers.remove(&old_key);
                            state.effective_sizes.remove(&old_key);
                        }
                    }
                    state.recompute_effective_size(&old_key);
                }
            }
            state
                .active_viewers
                .entry(key.clone())
                .or_default()
                .insert(viewer_id.to_string(), (cols, rows));
            state
                .viewer_focus
                .insert(viewer_id.to_string(), key.clone());
            state.recompute_effective_size(&key);
        });
    }

    fn viewer_disconnected(&self, viewer_id: &str) {
        self.with_state(|state| {
            state.viewer_focus.remove(viewer_id);
            let touched: Vec<TerminalRuntimeKey> = state
                .active_viewers
                .iter_mut()
                .filter_map(|(key, map)| {
                    if map.remove(viewer_id).is_some() {
                        Some(key.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for key in touched {
                let empty = state
                    .active_viewers
                    .get(&key)
                    .map(|m| m.is_empty())
                    .unwrap_or(true);
                if empty {
                    state.active_viewers.remove(&key);
                    state.effective_sizes.remove(&key);
                } else {
                    state.recompute_effective_size(&key);
                }
            }
        });
    }

    fn launch_tab(&self, section_id: &str, tab_id: &str) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        self.with_state(|state| {
            if state.broadcasts.contains_key(&key) {
                return;
            }
            if state.in_flight_launches.contains(&key) {
                return;
            }
            if state.pending_tab_launches.iter().any(|r| r.key == key) {
                return;
            }
            state.pending_tab_launches.push(TabLaunchRequest { key });
        });
    }

    // ── Task mutation (another-one-ojm.3) ─────────────────────────
    //
    // Mirror of `LocalSession::{create_worktree_task, rename_task,
    // set_task_pinned, remove_task}` in `api/local_session.rs`. The
    // delegating shape is intentional: same registry-locking pattern,
    // same core-service spawn, same persistence rules — both transports
    // converge on identical store mutations.

    fn create_worktree_task(
        &self,
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_provider: Option<AgentProvider>,
    ) -> RegistryFuture<'_, anyhow::Result<TaskSummary>> {
        let weak = self.inner.clone();
        Box::pin(async move {
            // Resolve project metadata up front so we can fail clearly
            // before spawning the worker thread.
            let (project_path, project_name, target_project_id) = {
                let arc = weak.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("create_worktree_task: registry state dropped")
                })?;
                let state = arc.lock().map_err(|_| {
                    anyhow::anyhow!("create_worktree_task: RegistryState mutex poisoned")
                })?;
                let project = state.project_store.project(&project_id).ok_or_else(|| {
                    anyhow::anyhow!("create_worktree_task: unknown project_id `{project_id}`")
                })?;
                (
                    project.path.clone(),
                    project.name.clone(),
                    project.id.clone(),
                )
            };

            let trimmed = task_name.trim().to_string();
            if trimmed.is_empty() {
                anyhow::bail!("create_worktree_task: task_name must not be blank");
            }
            let generated = trimmed.clone();

            let launch_config = match agent_provider.map(map_agent_provider_back) {
                Some(provider) => {
                    another_one_core::agents::TerminalLaunchConfig::for_provider(provider)
                }
                None => another_one_core::agents::TerminalLaunchConfig::default(),
            };
            let branch_mode =
                another_one_core::project_store::TaskWorktreeBranchMode::NewBranchFrom {
                    source_branch,
                };

            let mut rx = another_one_core::project_service::spawn_task_creation(
                target_project_id.clone(),
                project_path,
                project_name,
                trimmed,
                generated,
                branch_mode,
                launch_config,
            );
            let reply = rx
                .recv()
                .await
                .map_err(|_| anyhow::anyhow!("task creation worker dropped"))?;
            let success = reply
                .result
                .map_err(|f| anyhow::anyhow!("create task: {}", f.message))?;

            // Insert the prepared worktree project + the task under one
            // lock so the inline-snapshot reply observes both.
            let summary = {
                let arc = weak.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("create_worktree_task: registry state dropped after worker")
                })?;
                let mut state = arc.lock().map_err(|_| {
                    anyhow::anyhow!("create_worktree_task: registry mutex poisoned")
                })?;
                let inserted_worktree = state
                    .project_store
                    .insert_prepared_project(success.project.clone());
                let worktree_project_id = if inserted_worktree {
                    success.project.project.id.clone()
                } else {
                    state
                        .project_store
                        .projects
                        .iter()
                        .find(|p| p.path == success.project.project.path)
                        .map(|p| p.id.clone())
                        .unwrap_or_else(|| success.project.project.id.clone())
                };
                let section_key = insert_task_with_initial_tab(
                    &mut state,
                    target_project_id.clone(),
                    worktree_project_id.clone(),
                    another_one_core::project_store::TaskKind::Worktree,
                    success.task_name,
                    success.branch_name,
                    Some(worktree_project_id),
                    success.project.project.path.clone(),
                    success.launch_config,
                );
                let task_id = SectionId::from_store_key(&section_key)
                    .and_then(|section| section.task_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("create_worktree_task: inserted section missing task id")
                    })?;
                lookup_task_summary(&state, &task_id).ok_or_else(|| {
                    anyhow::anyhow!("create_worktree_task: task vanished after insert")
                })?
            };
            Ok(summary)
        })
    }

    fn rename_task(&self, task_id: &str, new_name: &str) -> (bool, Option<TaskSummary>) {
        let trimmed = new_name.trim().to_string();
        if trimmed.is_empty() {
            // Reject blank renames daemon-side, same as LocalSession.
            // Return the existing snapshot so the issuer can render
            // the old name in its UI without a follow-up read.
            return self
                .with_state(|state| (false, lookup_task_summary(state, task_id)))
                .unwrap_or((false, None));
        }
        self.with_state(|state| {
            let Some(task) = state.project_store.task_mut(task_id) else {
                return (false, None);
            };
            let changed = if task.name == trimmed {
                false
            } else {
                task.name = trimmed;
                true
            };
            if changed {
                state.project_store.save();
            }
            (changed, lookup_task_summary(state, task_id))
        })
        .unwrap_or((false, None))
    }

    fn set_task_pinned(&self, task_id: &str, pinned: bool) -> (bool, Option<TaskSummary>) {
        self.with_state(|state| {
            let changed = state.project_store.set_task_pinned(task_id, pinned);
            if changed {
                state.project_store.save();
            }
            (changed, lookup_task_summary(state, task_id))
        })
        .unwrap_or((false, None))
    }

    fn remove_task(&self, project_id: &str, task_id: &str) -> bool {
        self.with_state(|state| {
            state
                .project_store
                .remove_task(project_id, task_id)
                .is_some()
        })
        .unwrap_or(false)
    }

    // ── Project mutation (another-one-ojm.2) ──────────────────────

    /// Mirror of `LocalSession::add_project`: run `prepare_project`
    /// on a background thread (via `spawn_project_add`), then take
    /// the registry lock, insert, and project the post-mutation
    /// snapshot under the same lock so a concurrent removal can't
    /// race in between insertion and projection.
    fn add_project<'a>(
        &'a self,
        path: String,
    ) -> RegistryFuture<'a, anyhow::Result<ProjectSummary>> {
        Box::pin(async move {
            let mut rx = another_one_core::project_service::spawn_project_add(
                std::path::PathBuf::from(path),
            );
            let reply = rx
                .recv()
                .await
                .map_err(|_| anyhow::anyhow!("project add worker dropped"))?;
            let prepared = reply
                .result
                .map_err(|e| anyhow::anyhow!("prepare project: {e}"))?;
            let new_project_id = prepared.project.id.clone();

            let arc = self
                .inner
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("add_project: registry state dropped"))?;
            let mut guard = arc
                .lock()
                .map_err(|_| anyhow::anyhow!("add_project: RegistryState mutex poisoned"))?;
            let inserted = guard.project_store.insert_prepared_project(prepared);
            if !inserted {
                anyhow::bail!("project at this path already exists");
            }
            let project = guard
                .project_store
                .project(&new_project_id)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("add_project: inserted project missing from store")
                })?;
            Ok(project_to_frame(&guard, &project))
        })
    }

    /// Mirror of `LocalSession::remove_project`. Takes the registry
    /// lock and delegates to `project_store.remove_project`, which
    /// cascades to tasks + terminal sections. Idempotent on unknown
    /// ids — same semantics LocalSession exposes today.
    fn remove_project(&self, project_id: &str) -> anyhow::Result<()> {
        let arc = self
            .inner
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("remove_project: registry state dropped"))?;
        let mut guard = arc
            .lock()
            .map_err(|_| anyhow::anyhow!("remove_project: RegistryState mutex poisoned"))?;
        guard.project_store.remove_project(project_id);
        Ok(())
    }

    // ── Git state read verbs (`another-one-ojm.4`) ─────────────────
    //
    // Mirror of `LocalSession::read_*`/`*_branch_*` methods in
    // `api/local_session.rs`. The shapes are intentionally
    // field-for-field identical — keep them in sync when the FRB DTO
    // is extended.

    fn read_project_branches(&self, project_id: &str) -> Vec<String> {
        self.with_state(|state| state.project_store.branch_names(project_id))
            .unwrap_or_default()
    }

    fn primary_branch_for_project(&self, project_id: &str) -> Option<String> {
        self.with_state(|state| {
            state
                .project_store
                .primary_branch_for_project(project_id, true)
                .map(|branch| branch.name)
        })
        .flatten()
    }

    fn repo_default_commit_action(&self, project_id: &str) -> Option<String> {
        self.with_state(|state| {
            let project = state.project_store.project(project_id)?;
            state
                .project_store
                .repo_default_commit_action(&project.repo_id)
                .map(|a| match a {
                    another_one_core::project_store::RepoDefaultCommitAction::Commit => {
                        "commit".to_string()
                    }
                    another_one_core::project_store::RepoDefaultCommitAction::CommitAndPush => {
                        "commit-and-push".to_string()
                    }
                })
        })
        .flatten()
    }

    fn read_active_git_state(&self, project_id: &str) -> Option<ActiveGitStateWire> {
        let project_path = self.project_path(project_id)?;
        // The git invocation can shell out — wrap in `block_in_place`
        // so the daemon's tokio worker isn't held across the syscall.
        let state = tokio::task::block_in_place(|| {
            another_one_core::project_store::read_project_git_state(&project_path, true)
        });
        Some(ActiveGitStateWire {
            current_branch: state.current_branch,
            ahead_count: state.ahead_count as u32,
            behind_count: state.behind_count as u32,
        })
    }

    fn read_changed_files(&self, project_id: &str) -> Option<Vec<ChangedFileWire>> {
        let project_path = self.project_path(project_id)?;
        let git_state = tokio::task::block_in_place(|| {
            another_one_core::project_store::read_project_git_state(&project_path, false)
        });
        Some(
            git_state
                .changed_files
                .into_iter()
                .map(changed_file_to_wire)
                .collect(),
        )
    }

    fn read_project_github_url(&self, project_id: &str) -> Option<String> {
        let project_path = self.project_path(project_id)?;
        tokio::task::block_in_place(|| {
            another_one_core::git_actions::find_github_repo_url(&project_path)
        })
    }

    fn read_recent_commits(
        &self,
        project_id: &str,
        limit: usize,
    ) -> Result<Option<RecentCommitsWire>, String> {
        let Some(project_path) = self.project_path(project_id) else {
            return Ok(None);
        };
        let result = tokio::task::block_in_place(|| {
            another_one_core::project_store::read_project_branch_commit_state(&project_path, limit)
        })?;
        Ok(Some(RecentCommitsWire {
            current_branch: result.current_branch,
            has_more: result.has_more,
            commits: result.commits.into_iter().map(commit_to_wire).collect(),
        }))
    }

    fn read_commit_file_changes(
        &self,
        project_id: &str,
        commit_id: &str,
    ) -> Result<Option<Vec<BranchCompareFileWire>>, String> {
        let Some(project_path) = self.project_path(project_id) else {
            return Ok(None);
        };
        let result = tokio::task::block_in_place(|| {
            another_one_core::project_store::read_project_commit_file_changes(
                &project_path,
                commit_id,
            )
        })?;
        Ok(Some(
            result
                .files
                .into_iter()
                .map(branch_compare_file_to_wire)
                .collect(),
        ))
    }

    fn read_branch_compare_state(
        &self,
        project_id: &str,
        target_branch: &str,
    ) -> Result<Option<BranchCompareWire>, String> {
        let Some(project_path) = self.project_path(project_id) else {
            return Ok(None);
        };
        let result = tokio::task::block_in_place(|| {
            another_one_core::project_store::read_project_branch_compare_state(
                &project_path,
                target_branch,
            )
        })?;
        Ok(Some(BranchCompareWire {
            current_branch: result.current_branch,
            target_branch: result.target_branch,
            files: result
                .files
                .into_iter()
                .map(branch_compare_file_to_wire)
                .collect(),
        }))
    }

    fn read_branch_settings(&self, project_id: &str) -> Option<ResolvedBranchSettingsWire> {
        self.with_state(|state| {
            state
                .project_store
                .resolved_branch_settings(project_id)
                .map(|s| ResolvedBranchSettingsWire {
                    root_project_id: s.root_project_id,
                    available_branches: s.available_branches,
                    configured_default_branch: s.configured_default_branch,
                    effective_default_branch: s.effective_default_branch,
                    configured_default_target_branch: s.configured_default_target_branch,
                    effective_default_target_branch: s.effective_default_target_branch,
                })
        })
        .flatten()
    }

    fn set_branch_setting(
        &self,
        project_id: &str,
        field: &str,
        branch_name: Option<&str>,
    ) -> Result<bool, String> {
        let result = self.with_state(|state| match field {
            "default-branch" => state
                .project_store
                .update_default_branch(project_id, branch_name.map(|s| s.to_string())),
            "default-target-branch" => state
                .project_store
                .update_default_target_branch(project_id, branch_name.map(|s| s.to_string())),
            other => Err(format!("set_branch_setting: unknown field `{other}`")),
        });
        match result {
            Some(inner) => inner,
            None => Err("set_branch_setting: registry state unavailable".to_string()),
        }
    }

    fn stage_changed_file<'a>(
        &'a self,
        project_id: &'a str,
        path: &'a str,
        original_path: Option<&'a str>,
    ) -> RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        let path_arg = path.to_string();
        let original_path = original_path.map(str::to_string);
        Box::pin(async move {
            run_changed_file_mutation(
                &inner,
                "stage_changed_file",
                &project_id,
                move |project_path| {
                    let mut changed = another_one_core::project_store::ChangedFile::default();
                    changed.path = path_arg;
                    changed.original_path = original_path;
                    another_one_core::project_store::stage_changed_file(&project_path, &changed)
                },
            )
            .await
        })
    }

    fn unstage_changed_file<'a>(
        &'a self,
        project_id: &'a str,
        path: &'a str,
        original_path: Option<&'a str>,
    ) -> RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        let path_arg = path.to_string();
        let original_path = original_path.map(str::to_string);
        Box::pin(async move {
            run_changed_file_mutation(
                &inner,
                "unstage_changed_file",
                &project_id,
                move |project_path| {
                    let mut changed = another_one_core::project_store::ChangedFile::default();
                    changed.path = path_arg;
                    changed.original_path = original_path;
                    another_one_core::project_store::unstage_changed_file(&project_path, &changed)
                },
            )
            .await
        })
    }

    fn stage_all_changes<'a>(
        &'a self,
        project_id: &'a str,
    ) -> RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        Box::pin(async move {
            run_changed_file_mutation(&inner, "stage_all_changes", &project_id, |project_path| {
                another_one_core::project_store::stage_all_changes(&project_path)
            })
            .await
        })
    }

    fn unstage_all_changes<'a>(
        &'a self,
        project_id: &'a str,
    ) -> RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        Box::pin(async move {
            run_changed_file_mutation(&inner, "unstage_all_changes", &project_id, |project_path| {
                another_one_core::project_store::unstage_all_changes(&project_path)
            })
            .await
        })
    }

    fn discard_changed_file<'a>(
        &'a self,
        project_id: &'a str,
        path: &'a str,
        untracked: bool,
        original_path: Option<&'a str>,
    ) -> RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        let path_arg = path.to_string();
        let original_path = original_path.map(str::to_string);
        Box::pin(async move {
            run_changed_file_mutation(
                &inner,
                "discard_changed_file",
                &project_id,
                move |project_path| {
                    let mut changed = another_one_core::project_store::ChangedFile::default();
                    let path_for_err = path_arg.clone();
                    changed.path = path_arg;
                    changed.original_path = original_path;
                    changed.untracked = untracked;
                    if another_one_core::project_store::revert_changed_file(&project_path, &changed)
                    {
                        Ok(())
                    } else {
                        Err(format!("Could not discard {path_for_err}"))
                    }
                },
            )
            .await
        })
    }

    fn discard_all_changes<'a>(
        &'a self,
        project_id: &'a str,
        files: Vec<ChangedFileWire>,
    ) -> RegistryFuture<'a, anyhow::Result<(Vec<ChangedFileWire>, Vec<String>)>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        Box::pin(async move {
            let project_path = resolve_project_path(&inner, &project_id).ok_or_else(|| {
                anyhow::anyhow!("discard_all_changes: unknown project_id `{project_id}`")
            })?;
            let project_path_for_mutate = project_path.clone();
            let failures = tokio::task::spawn_blocking(move || {
                let mut failures = Vec::new();
                for changed in files.into_iter().map(changed_file_from_wire) {
                    let path_for_err = changed.path.clone();
                    if !another_one_core::project_store::revert_changed_file(
                        &project_path_for_mutate,
                        &changed,
                    ) {
                        failures.push(format!("Could not discard {path_for_err}"));
                    }
                }
                failures
            })
            .await
            .map_err(|e| anyhow::anyhow!("discard_all_changes join: {e}"))?;
            let project_path_for_read = project_path.clone();
            let git_state = tokio::task::spawn_blocking(move || {
                another_one_core::project_store::read_project_git_state(
                    &project_path_for_read,
                    false,
                )
            })
            .await
            .map_err(|e| anyhow::anyhow!("discard_all_changes post-read join: {e}"))?;
            Ok((
                git_state
                    .changed_files
                    .into_iter()
                    .map(changed_file_to_wire)
                    .collect(),
                failures,
            ))
        })
    }

    fn create_review_task<'a>(
        &'a self,
        project_id: &'a str,
        pull_request_number: u64,
        head_branch: &'a str,
        agent_provider: Option<AgentProvider>,
    ) -> RegistryFuture<'a, anyhow::Result<(String, Vec<ProjectSummary>)>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        let head_branch = head_branch.to_string();
        Box::pin(async move {
            // Phase 1: snapshot the project — path + name + id —
            // before kicking off the review-task worker. Mirrors
            // LocalSession::create_review_task's lookup-then-spawn
            // shape so the failure modes are identical.
            let (project_path, project_name, target_project_id) = {
                let arc = inner.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("create_review_task: registry vanished before lookup")
                })?;
                let state = arc.lock().map_err(|_| {
                    anyhow::anyhow!("create_review_task: RegistryState mutex poisoned")
                })?;
                let project = state.project_store.project(&project_id).ok_or_else(|| {
                    anyhow::anyhow!("create_review_task: unknown project_id `{project_id}`")
                })?;
                (
                    project.path.clone(),
                    project.name.clone(),
                    project.id.clone(),
                )
            };

            let task_name = format!("review-pr-{pull_request_number}");
            let launch_config = match agent_provider.map(map_agent_provider_back) {
                Some(provider) => {
                    another_one_core::agents::TerminalLaunchConfig::for_provider(provider)
                }
                None => another_one_core::agents::TerminalLaunchConfig::default(),
            };

            let mut rx = another_one_core::project_service::spawn_review_task_creation(
                target_project_id.clone(),
                project_path,
                task_name,
                pull_request_number,
                head_branch,
                launch_config,
                true,
                true,
            );
            let reply = rx
                .recv()
                .await
                .map_err(|_| anyhow::anyhow!("review task worker dropped before reply"))?;
            let success = reply
                .result
                .map_err(|f| anyhow::anyhow!("create review task: {}", f.message))?;

            // Phase 2: insert the prepared project + the review task.
            let section_id = {
                let arc = inner.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("create_review_task: registry vanished mid-flight")
                })?;
                let mut state = arc
                    .lock()
                    .map_err(|_| anyhow::anyhow!("create_review_task: registry mutex poisoned"))?;
                let inserted_worktree = state
                    .project_store
                    .insert_prepared_project(success.project.clone());
                let worktree_project_id = if inserted_worktree {
                    success.project.project.id.clone()
                } else {
                    state
                        .project_store
                        .projects
                        .iter()
                        .find(|p| p.path == success.project.project.path)
                        .map(|p| p.id.clone())
                        .unwrap_or_else(|| success.project.project.id.clone())
                };
                insert_task_with_initial_tab(
                    &mut state,
                    target_project_id,
                    worktree_project_id.clone(),
                    another_one_core::project_store::TaskKind::Worktree,
                    format!("Review #{pull_request_number} ({project_name})"),
                    success.branch_name,
                    Some(worktree_project_id),
                    success.project.project.path.clone(),
                    success.launch_config,
                )
            };

            // Phase 3: re-flatten for the inline-snapshot ack.
            let projects = {
                let arc = inner.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("create_review_task: registry vanished during snapshot read")
                })?;
                let state = arc
                    .lock()
                    .map_err(|_| anyhow::anyhow!("create_review_task: registry mutex poisoned"))?;
                flatten_state_to_frame(&state)
            };
            Ok((section_id, projects))
        })
    }

    fn create_branch<'a>(
        &'a self,
        project_id: &'a str,
        branch_name: &'a str,
        use_current_task: bool,
        migrate_changes: bool,
    ) -> RegistryFuture<'a, anyhow::Result<(String, Vec<ProjectSummary>)>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        let branch_name = branch_name.to_string();
        Box::pin(async move {
            // Phase 1: snapshot the project lookup (path + names)
            // outside the mutation worker. Mirrors LocalSession's
            // create_branch which does the same lookup-then-spawn split.
            let (project_path, target_project_id) = {
                let arc = inner.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("create_branch: registry vanished before lookup")
                })?;
                let state = arc
                    .lock()
                    .map_err(|_| anyhow::anyhow!("create_branch: RegistryState mutex poisoned"))?;
                let project = state.project_store.project(&project_id).ok_or_else(|| {
                    anyhow::anyhow!("create_branch: unknown project_id `{project_id}`")
                })?;
                (project.path.clone(), project.id.clone())
            };

            // Phase 2: kick off the worker thread that does the actual
            // git work. Same `spawn_branch_creation` LocalSession uses.
            let mut rx = another_one_core::project_service::spawn_branch_creation(
                target_project_id.clone(),
                project_path,
                branch_name,
                use_current_task,
                migrate_changes,
            );
            let reply = rx
                .recv()
                .await
                .map_err(|_| anyhow::anyhow!("branch creation worker dropped before reply"))?;
            let success = reply
                .result
                .map_err(|f| anyhow::anyhow!("create branch: {}", f.message))?;

            // Phase 3: insert the new project + task back into the
            // registry, capture the section_id, then re-flatten for
            // the inline-snapshot ack. Skipped for the current-task
            // mode where there's no new project, only a branch swap
            // on the existing checkout.
            let section_id = if let Some(prepared) = success.project {
                let arc = inner.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("create_branch: registry vanished mid-flight")
                })?;
                let mut state = arc
                    .lock()
                    .map_err(|_| anyhow::anyhow!("create_branch: registry mutex poisoned"))?;
                let inserted_worktree = state
                    .project_store
                    .insert_prepared_project(prepared.clone());
                let worktree_project_id = if inserted_worktree {
                    prepared.project.id.clone()
                } else {
                    state
                        .project_store
                        .projects
                        .iter()
                        .find(|p| p.path == prepared.project.path)
                        .map(|p| p.id.clone())
                        .unwrap_or_else(|| prepared.project.id.clone())
                };
                insert_task_with_initial_tab(
                    &mut state,
                    target_project_id,
                    worktree_project_id.clone(),
                    another_one_core::project_store::TaskKind::Worktree,
                    success.task_name,
                    success.branch_name,
                    Some(worktree_project_id),
                    prepared.project.path.clone(),
                    TerminalLaunchConfig::default(),
                )
            } else {
                // Current-task mode: no new project. The daemon's
                // checkout has already been swapped by
                // create_branch_from_head; nothing to insert.
                String::new()
            };

            // Phase 4: re-flatten the registry into the wire
            // ProjectSummary list so the ack carries the post-
            // mutation snapshot inline.
            let projects = {
                let arc = inner.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("create_branch: registry vanished during snapshot read")
                })?;
                let state = arc
                    .lock()
                    .map_err(|_| anyhow::anyhow!("create_branch: registry mutex poisoned"))?;
                flatten_state_to_frame(&state)
            };
            Ok((section_id, projects))
        })
    }

    fn run_toolbar_git_action<'a>(
        &'a self,
        project_id: &'a str,
        action_id: &'a str,
    ) -> RegistryFuture<'a, anyhow::Result<ToolbarActionOutcome>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        let action_id = action_id.to_string();
        Box::pin(async move {
            let project_path = resolve_project_path(&inner, &project_id).ok_or_else(|| {
                anyhow::anyhow!("run_toolbar_git_action: unknown project_id `{project_id}`")
            })?;
            let action = parse_toolbar_action_id(&action_id)?;
            let outcome = tokio::task::spawn_blocking(move || {
                let mut on_progress = |_msg: String| {};
                another_one_core::git_actions::execute_toolbar_git_action(
                    &project_path,
                    action,
                    another_one_core::git_actions::GitActionSettings::default(),
                    &mut on_progress,
                )
            })
            .await
            .map_err(|e| anyhow::anyhow!("run_toolbar_git_action join: {e}"))?;
            outcome
                .map(|o| ToolbarActionOutcome {
                    toast_message: o.toast_message,
                    warning: o.warning,
                    refresh_git_state: o.refresh_git_state,
                })
                .map_err(|err| anyhow::anyhow!(err.message))
        })
    }

    // ── Pull requests + checks (another-one-ojm.6) ─────────────────

    fn find_pull_request_status(
        &self,
        project_id: &str,
    ) -> Result<Option<PullRequestStatus>, String> {
        // Mirror `LocalSession::find_pull_request_status`: snapshot
        // the project's path + current branch under the registry
        // mutex, then drop the lock before shelling out so the
        // (slow) gh-CLI roundtrip never holds the daemon's project
        // store. `Ok(None)` covers both unknown project and "branch
        // has no PR"; gh-CLI execution failure surfaces upstream as
        // a WorkerReply::Err.
        let path_and_branch = self.with_state(|state| {
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .and_then(|project| {
                    project
                        .checkout
                        .current_branch
                        .clone()
                        .map(|branch| (project.path.clone(), branch))
                })
        });
        let Some(Some((project_path, head_branch))) = path_and_branch else {
            return Ok(None);
        };
        Ok(
            another_one_core::git_actions::find_latest_pull_request_status(
                &project_path,
                &head_branch,
            )
            .map(|status| PullRequestStatus {
                number: status.number,
                url: status.url,
                state: map_pull_request_state(status.state),
            }),
        )
    }

    fn read_pull_request_checks(&self, project_id: &str) -> Result<Option<Vec<Check>>, String> {
        // Same shape as `find_pull_request_status`: snapshot the
        // project path under the registry mutex, drop the lock,
        // then shell out via core's gh-CLI helper. The three-state
        // contract (`Some(list)` / `None` / `Err(_)`) maps onto
        // `WorkerReply::PullRequestChecksAck` / `WorkerReply::Err`
        // upstream. Mirrors `LocalSession::read_pull_request_checks`.
        let project_path = self.with_state(|state| {
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        });
        let Some(Some(project_path)) = project_path else {
            return Ok(None);
        };
        match another_one_core::git_actions::find_pull_request_checks(&project_path, None) {
            Ok(Some(checks)) => Ok(Some(checks.into_iter().map(map_check).collect())),
            Ok(None) => Ok(None),
            Err(message) => Err(message),
        }
    }

    fn find_project_pull_requests(
        &self,
        project_id: &str,
        filter_index: u32,
        query: &str,
    ) -> Result<Option<Vec<ProjectPagePullRequest>>, String> {
        // Mirrors `LocalSession::find_project_pull_requests`:
        // snapshot the project path under the registry mutex, drop
        // the lock, then shell out (`gh pr list`). `Ok(None)`
        // covers unknown-project so the UI can render its empty
        // state; gh CLI / auth / network errors propagate as Err
        // and surface upstream as WorkerReply::Err.
        let project_path = self.with_state(|state| {
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        });
        let Some(Some(project_path)) = project_path else {
            return Ok(None);
        };
        let trimmed = query.trim();
        let q = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
        let prs = another_one_core::git_actions::find_project_pull_requests(
            &project_path,
            filter_index as usize,
            q,
        )?;
        Ok(Some(prs.into_iter().map(map_project_page_pr).collect()))
    }

    // ── Custom actions + Open In + agents (another-one-ojm.7) ─────

    fn open_in_state(&self) -> Option<OpenInStateWire> {
        // Mirrors `LocalSession::open_in_state` (api/local_session.rs).
        // Cheap: install detection runs through HeadlessPlatform's
        // single PATH walk + the project-store read is one mutex lock.
        // No need to run on a blocking pool.
        let available = available_open_in_apps();
        self.with_state(|state| {
            let enabled_apps = state
                .project_store
                .enabled_open_in_apps(&available)
                .into_iter()
                .map(open_in_app_to_wire)
                .collect();
            let preferred_app_id = state
                .project_store
                .preferred_open_in_app(&available)
                .map(|app| app.id().to_string());
            OpenInStateWire {
                enabled_apps,
                preferred_app_id,
            }
        })
    }

    fn list_project_actions(&self, project_id: &str) -> Vec<ProjectActionWire> {
        // Mirrors `LocalSession::list_project_actions`.
        self.with_state(|state| {
            state
                .project_store
                .project_actions(project_id)
                .into_iter()
                .map(project_action_to_wire)
                .collect()
        })
        .unwrap_or_default()
    }

    fn read_enabled_agents(&self) -> EnabledAgentsViewWire {
        // Mirrors `LocalSession::read_enabled_agents`.
        self.with_state(|state| {
            let enabled = another_one_core::agents::effective_enabled_agents(
                state.project_store.ui.enabled_agents.as_ref(),
            );
            let agents = enabled
                .iter()
                .map(|agent| agent_def_to_wire(agent))
                .collect();
            let default_agent_id = state.project_store.default_agent_id().map(str::to_string);
            EnabledAgentsViewWire {
                agents,
                default_agent_id,
            }
        })
        .unwrap_or_else(|| EnabledAgentsViewWire {
            agents: Vec::new(),
            default_agent_id: None,
        })
    }

    fn submit_new_task(
        &self,
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_ids: Vec<String>,
        branch_mode_existing: bool,
        worktree_mode: bool,
    ) -> RegistryFuture<'_, anyhow::Result<String>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let root_project_id = {
                let arc = inner.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("submit_new_task: registry vanished before lookup")
                })?;
                let state = arc
                    .lock()
                    .map_err(|_| anyhow::anyhow!("submit_new_task: registry mutex poisoned"))?;
                state
                    .project_store
                    .root_project_id_for_project(&project_id)
                    .unwrap_or(project_id.clone())
            };

            let trimmed_task_name = task_name.trim().to_string();
            if trimmed_task_name.is_empty() {
                anyhow::bail!("submit_new_task: task_name must not be blank");
            }

            let launch_config = selected_agent_launch_config(&agent_ids);

            if !worktree_mode {
                let arc = inner.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("submit_new_task: registry vanished before direct insert")
                })?;
                let mut state = arc
                    .lock()
                    .map_err(|_| anyhow::anyhow!("submit_new_task: registry mutex poisoned"))?;
                let project = state
                    .project_store
                    .project(&root_project_id)
                    .cloned()
                    .ok_or_else(|| {
                        anyhow::anyhow!("submit_new_task: unknown project_id `{root_project_id}`")
                    })?;
                let branch_name = another_one_core::project_store::current_branch(&project.path)
                    .or_else(|| state.project_store.current_branch_name(&project.id))
                    .unwrap_or_else(|| source_branch.trim().to_string());
                if branch_name.trim().is_empty() {
                    anyhow::bail!(
                        "submit_new_task: could not determine current branch for `{}`",
                        project.name
                    );
                }

                return Ok(insert_task_with_initial_tab(
                    &mut state,
                    project.id.clone(),
                    project.id.clone(),
                    another_one_core::project_store::TaskKind::Direct,
                    trimmed_task_name,
                    branch_name,
                    None,
                    project.path.clone(),
                    launch_config,
                ));
            }

            let (project_path, project_name) = {
                let arc = inner.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("submit_new_task: registry vanished before worktree lookup")
                })?;
                let state = arc
                    .lock()
                    .map_err(|_| anyhow::anyhow!("submit_new_task: registry mutex poisoned"))?;
                let project = state
                    .project_store
                    .project(&root_project_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("submit_new_task: unknown project_id `{root_project_id}`")
                    })?;
                (project.path.clone(), project.name.clone())
            };

            let trimmed_source_branch = source_branch.trim().to_string();
            if trimmed_source_branch.is_empty() {
                anyhow::bail!("submit_new_task: source_branch must not be blank");
            }

            let branch_mode = if branch_mode_existing {
                another_one_core::project_store::TaskWorktreeBranchMode::ExistingBranch {
                    branch: trimmed_source_branch,
                }
            } else {
                another_one_core::project_store::TaskWorktreeBranchMode::NewBranchFrom {
                    source_branch: trimmed_source_branch,
                }
            };

            let mut rx = another_one_core::project_service::spawn_task_creation(
                root_project_id.clone(),
                project_path,
                project_name,
                trimmed_task_name.clone(),
                trimmed_task_name,
                branch_mode,
                launch_config,
            );
            let reply = rx
                .recv()
                .await
                .map_err(|_| anyhow::anyhow!("submit_new_task: worker dropped"))?;
            let success = reply
                .result
                .map_err(|f| anyhow::anyhow!("submit new task: {}", f.message))?;

            let arc = inner.upgrade().ok_or_else(|| {
                anyhow::anyhow!("submit_new_task: registry vanished after worker")
            })?;
            let mut state = arc
                .lock()
                .map_err(|_| anyhow::anyhow!("submit_new_task: registry mutex poisoned"))?;
            let inserted_worktree = state
                .project_store
                .insert_prepared_project(success.project.clone());
            let worktree_project_id = if inserted_worktree {
                success.project.project.id.clone()
            } else {
                state
                    .project_store
                    .projects
                    .iter()
                    .find(|project| project.path == success.project.project.path)
                    .map(|project| project.id.clone())
                    .unwrap_or_else(|| success.project.project.id.clone())
            };

            Ok(insert_task_with_initial_tab(
                &mut state,
                root_project_id,
                worktree_project_id.clone(),
                another_one_core::project_store::TaskKind::Worktree,
                success.task_name,
                success.branch_name,
                Some(worktree_project_id),
                success.project.project.path.clone(),
                success.launch_config,
            ))
        })
    }

    fn add_agent_to_section(&self, section_id: &str, agent_id: &str) -> Result<String, String> {
        let section = SectionId::from_store_key(section_id).ok_or_else(|| {
            format!(
                "add_agent_to_section: malformed section_id `{section_id}` — \
                 expected SectionId::store_key()"
            )
        })?;
        let launch_config = launch_config_for_add_agent(agent_id)?;
        let tab_id = self
            .with_state(|state| {
                let mut section_state = state
                    .project_store
                    .terminal_sections
                    .get(section_id)
                    .cloned()
                    .ok_or_else(|| {
                        format!("add_agent_to_section: unknown section_id `{section_id}`")
                    })?;
                let (tab_id, tab) = build_terminal_tab(launch_config, None);
                append_tab_to_section(&mut section_state, tab);
                state
                    .project_store
                    .set_section_state(section_id.to_string(), section_state);
                state.pending_tab_launches.push(TabLaunchRequest {
                    key: TerminalRuntimeKey {
                        section_id: section,
                        tab_id: tab_id.clone(),
                    },
                });
                Ok::<_, String>(tab_id)
            })
            .ok_or_else(|| "add_agent_to_section: registry state dropped".to_string())??;
        Ok(tab_id)
    }

    fn activate_section_tab(&self, section_id: &str, tab_id: &str) -> Result<(), String> {
        self.with_state(|state| {
            let mut section_state = state
                .project_store
                .terminal_sections
                .get(section_id)
                .cloned()
                .ok_or_else(|| {
                    format!("activate_section_tab: unknown section_id `{section_id}`")
                })?;
            if !set_active_tab_in_section(&mut section_state, tab_id) {
                return Err(format!(
                    "activate_section_tab: unknown tab_id `{tab_id}` for section `{section_id}`"
                ));
            }
            state
                .project_store
                .set_section_state(section_id.to_string(), section_state);
            Ok(())
        })
        .ok_or_else(|| "activate_section_tab: registry state dropped".to_string())?
    }

    fn close_section_tab(&self, section_id: &str, tab_id: &str) -> Result<String, String> {
        let section = SectionId::from_store_key(section_id).ok_or_else(|| {
            format!(
                "close_section_tab: malformed section_id `{section_id}` — \
                 expected SectionId::store_key()"
            )
        })?;
        let active_tab_id = self
            .with_state(|state| {
                let mut section_state = state
                    .project_store
                    .terminal_sections
                    .get(section_id)
                    .cloned()
                    .ok_or_else(|| {
                        format!("close_section_tab: unknown section_id `{section_id}`")
                    })?;
                let active_tab_id = close_tab_in_section(&mut section_state, tab_id).ok_or_else(
                    || {
                        format!(
                            "close_section_tab: unknown tab_id `{tab_id}` for section `{section_id}`"
                        )
                    },
                )?;
                state
                    .project_store
                    .set_section_state(section_id.to_string(), section_state);
                state.pending_tab_terminations.push(TerminalRuntimeKey {
                    section_id: section,
                    tab_id: tab_id.to_string(),
                });
                Ok::<_, String>(active_tab_id)
            })
            .ok_or_else(|| "close_section_tab: registry state dropped".to_string())??;
        Ok(active_tab_id)
    }

    fn toggle_section_tab_pinned(&self, section_id: &str, tab_id: &str) -> Result<bool, String> {
        self.with_state(|state| {
            let mut section_state = state
                .project_store
                .terminal_sections
                .get(section_id)
                .cloned()
                .ok_or_else(|| {
                    format!("toggle_section_tab_pinned: unknown section_id `{section_id}`")
                })?;
            let pinned =
                toggle_tab_pinned_in_section(&mut section_state, tab_id).ok_or_else(|| {
                    format!(
                        "toggle_section_tab_pinned: unknown tab_id `{tab_id}` \
                         for section `{section_id}`"
                    )
                })?;
            state
                .project_store
                .set_section_state(section_id.to_string(), section_state);
            Ok(pinned)
        })
        .ok_or_else(|| "toggle_section_tab_pinned: registry state dropped".to_string())?
    }

    fn read_agent_settings(&self) -> AgentSettingsViewWire {
        // Mirrors `LocalSession::read_agent_settings`.
        self.with_state(|state| {
            let default_agent_id = state.project_store.default_agent_id().map(str::to_string);
            let agents = another_one_core::agents::AGENTS
                .iter()
                .map(|agent| AgentSettingsRowWire {
                    id: agent.id.to_string(),
                    label: agent.label.to_string(),
                    icon_path: agent.icon.to_string(),
                    provider: agent.provider.map(map_agent_provider),
                    enabled: state.project_store.agent_enabled(agent.id),
                    is_default: default_agent_id.as_deref() == Some(agent.id),
                    launch_args: state.project_store.agent_launch_args(agent.id).to_vec(),
                })
                .collect();
            AgentSettingsViewWire {
                agents,
                default_agent_id,
            }
        })
        .unwrap_or_else(|| AgentSettingsViewWire {
            agents: Vec::new(),
            default_agent_id: None,
        })
    }

    fn set_agent_enabled(&self, agent_id: &str, enabled: bool) -> Result<bool, String> {
        ensure_known_agent_id(agent_id)?;
        self.with_state(|state| state.project_store.set_agent_enabled(agent_id, enabled))
            .ok_or_else(|| "set_agent_enabled: registry state dropped".to_string())
    }

    fn set_default_agent(&self, agent_id: &str) -> Result<bool, String> {
        ensure_known_agent_id(agent_id)?;
        self.with_state(|state| state.project_store.set_default_agent(agent_id))
            .ok_or_else(|| "set_default_agent: registry state dropped".to_string())
    }

    fn set_agent_launch_args(&self, agent_id: &str, args: Vec<String>) -> Result<bool, String> {
        ensure_known_agent_id(agent_id)?;
        self.with_state(|state| state.project_store.set_agent_launch_args(agent_id, args))
            .ok_or_else(|| "set_agent_launch_args: registry state dropped".to_string())
    }

    fn read_open_in_settings(&self) -> Option<OpenInSettingsViewWire> {
        let available = available_open_in_apps();
        self.with_state(|state| OpenInSettingsViewWire {
            available_apps: available
                .iter()
                .copied()
                .map(|app| OpenInAppSettingsRowWire {
                    id: app.id().to_string(),
                    label: app.label().to_string(),
                    description: app.description().to_string(),
                    icon_path: app.icon_path().to_string(),
                    enabled: state.project_store.open_in_app_enabled(app, &available),
                })
                .collect(),
        })
    }

    fn set_open_in_app_enabled(&self, app_id: &str, enabled: bool) -> Result<(), String> {
        let app = parse_open_in_app_id(app_id)
            .ok_or_else(|| format!("unknown open-in app `{app_id}`"))?;
        let available = available_open_in_apps();
        if !available.contains(&app) {
            return Err(format!(
                "open-in app `{app_id}` is not available on this host"
            ));
        }
        self.with_state(|state| {
            state
                .project_store
                .set_open_in_app_enabled(app, enabled, &available);
        })
        .ok_or_else(|| "registry state dropped".to_string())
    }

    fn open_project_in_app(&self, project_id: &str, app_id: &str) -> Result<(), String> {
        let app = parse_open_in_app_id(app_id)
            .ok_or_else(|| format!("unknown open-in app `{app_id}`"))?;
        let available = available_open_in_apps();
        if !available.contains(&app) {
            return Err(format!(
                "open-in app `{app_id}` is not available on this host"
            ));
        }
        let project_path = self
            .project_path(project_id)
            .ok_or_else(|| format!("unknown project_id `{project_id}`"))?;
        let enabled = self
            .with_state(|state| state.project_store.open_in_app_enabled(app, &available))
            .ok_or_else(|| "registry state dropped".to_string())?;
        if !enabled {
            return Err(format!("open-in app `{app_id}` is disabled"));
        }

        let mut command =
            <CurrentPlatform as HeadlessPlatform>::command_for_open_in(app, &project_path);
        command
            .spawn()
            .map_err(|err| format!("Could not open {}: {err}", app.label()))?;

        self.with_state(|state| {
            state
                .project_store
                .set_preferred_open_in_app(app, &available);
        })
        .ok_or_else(|| "registry state dropped".to_string())
    }

    fn run_project_action(
        &self,
        project_id: &str,
        section_id: &str,
        action_id: &str,
    ) -> Result<String, String> {
        // Mirrors `LocalSession::run_project_action`.
        let key_section = SectionId::from_store_key(section_id).ok_or_else(|| {
            format!(
                "run_project_action: malformed section_id `{section_id}` — \
                 expected SectionId::store_key()"
            )
        })?;
        let arc = self
            .inner
            .upgrade()
            .ok_or_else(|| "run_project_action: registry state dropped".to_string())?;
        let mut state = arc
            .lock()
            .map_err(|_| "run_project_action: RegistryState mutex poisoned".to_string())?;

        let action = state
            .project_store
            .project_actions(project_id)
            .into_iter()
            .find(|a| a.id == action_id)
            .ok_or_else(|| {
                format!(
                    "run_project_action: unknown action_id `{action_id}` for project `{project_id}`"
                )
            })?;

        let (launch_config, post_launch_input, fixed_title) = match &action.kind {
            ProjectActionKind::Shell { command } => {
                let trimmed = command.trim();
                if trimmed.is_empty() {
                    return Err("Shell actions need a command before they can run.".to_string());
                }
                let title = {
                    let name = action.name.trim();
                    (!name.is_empty()).then(|| name.to_string())
                };
                (
                    another_one_core::agents::TerminalLaunchConfig::default(),
                    Some(format!("{trimmed}\n").into_bytes()),
                    title,
                )
            }
            ProjectActionKind::Agent { provider, .. } => {
                let args =
                    another_one_core::project_store::project_action_agent_launch_args(&action)?;
                (
                    another_one_core::agents::TerminalLaunchConfig::for_provider(*provider)
                        .with_extra_args(args)
                        .with_agent_launch_args(false),
                    None,
                    None,
                )
            }
        };

        let (tab_id, tab) = build_terminal_tab(launch_config.clone(), fixed_title);

        let key = TerminalRuntimeKey {
            section_id: key_section,
            tab_id: tab_id.clone(),
        };

        let mut existing_section = state
            .project_store
            .terminal_sections
            .get(section_id)
            .cloned()
            .unwrap_or_else(|| PersistedSectionState {
                active_tab_id: String::new(),
                next_tab_id: 1,
                cwd: None,
                tabs: Vec::new(),
            });
        append_tab_to_section(&mut existing_section, tab);
        state
            .project_store
            .set_section_state(section_id.to_string(), existing_section);

        if let Some(input) = post_launch_input {
            state.pending_post_launch_input.insert(key.clone(), input);
        }
        state.pending_tab_launches.push(TabLaunchRequest { key });

        Ok(tab_id)
    }

    fn save_project_action(
        &self,
        project_id: &str,
        action: ProjectActionWire,
        save_global_copy: bool,
    ) -> Result<(), String> {
        let action = project_action_from_wire(action)?;
        self.with_state(|state| {
            state
                .project_store
                .upsert_project_action(project_id, action, save_global_copy)
        })
        .ok_or_else(|| "registry state dropped".to_string())?
    }

    fn delete_project_action(&self, project_id: &str, action_id: &str) -> bool {
        self.with_state(|state| {
            state
                .project_store
                .delete_project_action(project_id, action_id)
        })
        .unwrap_or(false)
    }

    // ── Settings → Git Actions (`another-one-ojm.8`) ───────────────

    fn read_git_action_scripts(&self) -> GitActionScriptsView {
        // Mirrors `LocalSession::read_git_action_scripts`.
        self.with_state(|state| {
            let store = &state.project_store;
            GitActionScriptsView {
                commit_script: store.git_commit_generation_script().to_string(),
                commit_using_default: store.ui.git_commit_generation_script.is_none(),
                pr_script: store.git_pr_generation_script().to_string(),
                pr_using_default: store.ui.git_pr_generation_script.is_none(),
            }
        })
        .unwrap_or_else(|| GitActionScriptsView {
            commit_script: String::new(),
            commit_using_default: true,
            pr_script: String::new(),
            pr_using_default: true,
        })
    }

    fn set_git_commit_script(&self, script: &str) -> Result<bool, String> {
        self.with_state(|state| {
            state
                .project_store
                .set_git_commit_generation_script(script.to_string())
        })
        .ok_or_else(|| "registry state dropped".to_string())
    }

    fn reset_git_commit_script(&self) -> Result<bool, String> {
        self.with_state(|state| state.project_store.reset_git_commit_generation_script())
            .ok_or_else(|| "registry state dropped".to_string())
    }

    fn set_git_pr_script(&self, script: &str) -> Result<bool, String> {
        self.with_state(|state| {
            state
                .project_store
                .set_git_pr_generation_script(script.to_string())
        })
        .ok_or_else(|| "registry state dropped".to_string())
    }

    fn reset_git_pr_script(&self) -> Result<bool, String> {
        self.with_state(|state| {
            let removed = state
                .project_store
                .ui
                .git_pr_generation_script
                .take()
                .is_some();
            if removed {
                state.project_store.save();
            }
            removed
        })
        .ok_or_else(|| "registry state dropped".to_string())
    }

    // ── Settings → Keybindings (`another-one-ojm.8`) ───────────────

    fn read_shortcut_settings(&self) -> ShortcutSettingsView {
        self.with_state(|state| {
            let shortcuts = &state.project_store.ui.shortcuts;
            let actions = another_one_core::shortcuts::ALL_SHORTCUT_ACTIONS
                .iter()
                .map(|action| ShortcutSettingsRow {
                    id: shortcut_action_id(*action).to_string(),
                    label: action.label().to_string(),
                    current_binding: shortcuts.binding_for(*action).to_string(),
                    default_binding: action.default_binding().to_string(),
                })
                .collect();
            ShortcutSettingsView { actions }
        })
        .unwrap_or(ShortcutSettingsView {
            actions: Vec::new(),
        })
    }

    fn set_shortcut_binding(&self, action_id: &str, binding: &str) -> Result<(), String> {
        let action = parse_shortcut_action_id(action_id)
            .ok_or_else(|| format!("unknown action id `{action_id}`"))?;
        self.with_state(|state| {
            state
                .project_store
                .set_shortcut_binding(action, binding.to_string());
        })
        .ok_or_else(|| "registry state dropped".to_string())
    }

    fn reset_shortcut_binding(&self, action_id: &str) -> Result<(), String> {
        let action = parse_shortcut_action_id(action_id)
            .ok_or_else(|| format!("unknown action id `{action_id}`"))?;
        self.with_state(|state| {
            state.project_store.reset_shortcut_binding(action);
        })
        .ok_or_else(|| "registry state dropped".to_string())
    }

    // ── Settings → MCP (`another-one-ojm.8`) ───────────────────────

    fn read_mcp_settings(&self) -> McpSettingsView {
        let mut registry = another_one_core::mcp::registry::McpRegistry::load();
        ensure_builtin_daemon_mcp_entry(&mut registry);
        let catalog_entries = another_one_core::mcp::catalog::entries()
            .iter()
            .map(|entry| McpCatalogEntryDto {
                id: entry.id.to_string(),
                label: entry.label.to_string(),
                description: entry.description.to_string(),
                docs_url: entry.docs_url.to_string(),
            })
            .collect();
        let registry_entries = registry.entries.iter().map(mcp_server_to_wire).collect();
        McpSettingsView {
            catalog_entries,
            registry_entries,
            sync_error_provider_ids: mcp_sync_error_provider_ids(),
        }
    }

    fn mcp_add_from_catalog(&self, catalog_id: &str) -> Result<(), String> {
        let entry = match another_one_core::mcp::catalog::find(catalog_id) {
            Some(e) => e,
            None => return Ok(()),
        };
        let mut registry = another_one_core::mcp::registry::McpRegistry::load();
        registry.upsert(another_one_core::mcp::catalog::instantiate(entry));
        registry
            .save()
            .map_err(|e| format!("save mcp registry: {e}"))?;
        Ok(())
    }

    fn mcp_toggle(&self, entry_id: &str, provider_id: &str, enabled: bool) -> Result<(), String> {
        let provider = parse_provider_id(provider_id)
            .ok_or_else(|| format!("unknown provider id `{provider_id}`"))?;
        let mut registry = another_one_core::mcp::registry::McpRegistry::load();
        ensure_builtin_daemon_mcp_entry(&mut registry);
        if !registry.toggle(entry_id, provider, enabled) {
            return Ok(());
        }
        let report = registry.sync_all();
        let sync_result = record_mcp_sync_errors(&report);
        registry
            .save()
            .map_err(|e| format!("save mcp registry: {e}"))?;
        sync_result
    }

    fn mcp_remove(&self, entry_id: &str) -> Result<(), String> {
        let mut registry = another_one_core::mcp::registry::McpRegistry::load();
        ensure_builtin_daemon_mcp_entry(&mut registry);
        if !registry.remove(entry_id) {
            return Ok(());
        }
        let report = registry.sync_all();
        let sync_result = record_mcp_sync_errors(&report);
        registry
            .save()
            .map_err(|e| format!("save mcp registry: {e}"))?;
        sync_result
    }
}

impl BridgeDaemonRegistry {
    /// Resolve a project id to its on-disk path by snapshot of the
    /// in-memory store. Used by every git-state read verb that shells
    /// out — the caller drops the registry lock before the (blocking)
    /// git work, so a hung `git status` doesn't block every other
    /// registry method for the duration.
    fn project_path(&self, project_id: &str) -> Option<std::path::PathBuf> {
        self.with_state(|state| {
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        })
        .flatten()
    }
}

#[derive(Clone, Debug)]
struct InsertedTask {
    root_project_id: String,
    task_id: String,
    section_key: String,
    tab_id: String,
}

impl InsertedTask {
    fn runtime_key(&self) -> Option<TerminalRuntimeKey> {
        key_from_wire(&self.section_key, &self.tab_id)
    }
}

#[derive(Clone, Debug)]
struct McpResolvedTab {
    section_key: String,
    key: TerminalRuntimeKey,
}

fn mcp_tab_ref(section_key: &str, tab_id: &str) -> String {
    format!("{section_key}{MCP_TAB_REF_SEPARATOR}{tab_id}")
}

fn resolve_mcp_tab_ref(state: &RegistryState, tab_ref: &str) -> Option<McpResolvedTab> {
    if let Some((section_key, tab_id)) = tab_ref.split_once(MCP_TAB_REF_SEPARATOR) {
        return resolve_mcp_tab_ref_parts(state, section_key, tab_id);
    }

    let mut matches = state
        .project_store
        .terminal_sections
        .iter()
        .filter(|(_, section)| section.tabs.iter().any(|tab| tab.id == tab_ref))
        .filter_map(|(section_key, _)| resolve_mcp_tab_ref_parts(state, section_key, tab_ref))
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn resolve_mcp_tab_ref_parts(
    state: &RegistryState,
    section_key: &str,
    tab_id: &str,
) -> Option<McpResolvedTab> {
    let section = state.project_store.terminal_sections.get(section_key)?;
    if !section.tabs.iter().any(|tab| tab.id == tab_id) {
        return None;
    }
    Some(McpResolvedTab {
        section_key: section_key.to_string(),
        key: key_from_wire(section_key, tab_id)?,
    })
}

fn queue_initial_tab_input(
    state: &mut RegistryState,
    inserted: &InsertedTask,
    input: Option<Vec<u8>>,
) {
    let Some(input) = input else {
        return;
    };
    let Some(key) = inserted.runtime_key() else {
        return;
    };
    state.pending_post_launch_input.insert(key, input);
}

fn terminal_input_line(input: &str) -> Vec<u8> {
    let mut bytes = input.as_bytes().to_vec();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    bytes
}

fn task_title_or_default(title: Option<&str>, fallback: &str) -> String {
    let title = title.map(str::trim).filter(|title| !title.is_empty());
    title.unwrap_or(fallback).to_string()
}

fn write_tab_bytes(writer: Arc<Mutex<Box<dyn Write + Send>>>, bytes: &[u8]) -> anyhow::Result<()> {
    let mut guard = writer
        .lock()
        .map_err(|_| anyhow::anyhow!("terminal writer mutex poisoned"))?;
    guard.write_all(bytes)?;
    guard.flush()?;
    Ok(())
}

fn wait_for_task_creation(
    rx: &mut broadcast::Receiver<another_one_core::project_service::TaskCreationReply>,
    context: &str,
) -> anyhow::Result<another_one_core::project_service::TaskCreationReply> {
    use tokio::sync::broadcast::error::TryRecvError;

    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match rx.try_recv() {
            Ok(reply) => return Ok(reply),
            Err(TryRecvError::Empty) => {
                if Instant::now() >= deadline {
                    anyhow::bail!("{context}: task creation worker timed out");
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(TryRecvError::Lagged(_)) => continue,
            Err(TryRecvError::Closed) => {
                anyhow::bail!("{context}: task creation worker dropped")
            }
        }
    }
}

fn insert_task_with_initial_tab(
    state: &mut RegistryState,
    root_project_id: String,
    target_project_id: String,
    kind: another_one_core::project_store::TaskKind,
    task_name: String,
    branch_name: String,
    worktree_project_id: Option<String>,
    project_path: std::path::PathBuf,
    launch_config: TerminalLaunchConfig,
) -> String {
    insert_task_with_initial_tab_details(
        state,
        root_project_id,
        target_project_id,
        kind,
        task_name,
        branch_name,
        worktree_project_id,
        project_path,
        launch_config,
    )
    .section_key
}

fn insert_task_with_initial_tab_details(
    state: &mut RegistryState,
    root_project_id: String,
    target_project_id: String,
    kind: another_one_core::project_store::TaskKind,
    task_name: String,
    branch_name: String,
    worktree_project_id: Option<String>,
    project_path: std::path::PathBuf,
    launch_config: TerminalLaunchConfig,
) -> InsertedTask {
    let task_id = uuid::Uuid::new_v4().to_string();
    let section = SectionId::for_task(&target_project_id, &branch_name, &task_id);
    let section_key = section.store_key();

    state
        .project_store
        .insert_task(another_one_core::project_store::Task {
            id: task_id.clone(),
            name: task_name,
            kind,
            root_project_id: root_project_id.clone(),
            target_project_id,
            branch_name,
            section_id: section_key.clone(),
            worktree_project_id,
            tabs: Vec::new(),
            active_tab_id: String::new(),
            next_tab_id: 0,
            cwd: None,
        });

    let initial_tab_id = "0".to_string();
    let initial_tab = PersistedTerminalTab {
        id: initial_tab_id.clone(),
        title: launch_config.default_title(),
        pinned: false,
        fixed_title: None,
        provider: launch_config.provider,
        launch_config: Some(launch_config),
        restore_status: another_one_core::agents::TerminalRestoreStatus::Launching,
        failure_message: None,
        failure_details: None,
    };
    state.project_store.set_section_state(
        section_key.clone(),
        PersistedSectionState {
            active_tab_id: initial_tab_id.clone(),
            next_tab_id: 1,
            cwd: Some(project_path),
            tabs: vec![initial_tab],
        },
    );
    state.pending_tab_launches.push(TabLaunchRequest {
        key: TerminalRuntimeKey {
            section_id: section,
            tab_id: initial_tab_id.clone(),
        },
    });

    InsertedTask {
        root_project_id,
        task_id,
        section_key,
        tab_id: initial_tab_id,
    }
}
fn launch_config_for_add_agent(agent_id: &str) -> Result<TerminalLaunchConfig, String> {
    let trimmed_agent_id = agent_id.trim();
    another_one_core::agents::terminal_launch_config_for_selected_agent(
        (!trimmed_agent_id.is_empty()).then_some(trimmed_agent_id),
    )
    .ok_or_else(|| format!("add_agent_to_section: unknown agent_id `{trimmed_agent_id}`"))
}

fn build_terminal_tab(
    launch_config: TerminalLaunchConfig,
    fixed_title: Option<String>,
) -> (String, PersistedTerminalTab) {
    let tab_id = uuid::Uuid::new_v4().to_string();
    let title = fixed_title
        .clone()
        .unwrap_or_else(|| launch_config.default_title());
    let tab = PersistedTerminalTab {
        id: tab_id.clone(),
        title,
        pinned: false,
        fixed_title,
        provider: launch_config.provider,
        launch_config: Some(launch_config),
        restore_status: another_one_core::agents::TerminalRestoreStatus::Launching,
        failure_message: None,
        failure_details: None,
    };
    (tab_id, tab)
}

fn append_tab_to_section(section: &mut PersistedSectionState, tab: PersistedTerminalTab) {
    section.active_tab_id = tab.id.clone();
    section.tabs.push(tab);
    section.next_tab_id = section.next_tab_id.saturating_add(1);
}

fn set_active_tab_in_section(section: &mut PersistedSectionState, tab_id: &str) -> bool {
    if !section.tabs.iter().any(|tab| tab.id == tab_id) {
        return false;
    }
    section.active_tab_id = tab_id.to_string();
    true
}

fn close_tab_in_section(section: &mut PersistedSectionState, tab_id: &str) -> Option<String> {
    let remove_index = section.tabs.iter().position(|tab| tab.id == tab_id)?;
    let mut active_index = if section.tabs.is_empty() {
        0
    } else {
        section
            .tabs
            .iter()
            .position(|tab| tab.id == section.active_tab_id)
            .unwrap_or_else(|| section.tabs.len().saturating_sub(1))
    };
    section.tabs.remove(remove_index);
    if section.tabs.is_empty() {
        section.active_tab_id.clear();
        return Some(String::new());
    }
    if remove_index < active_index {
        active_index = active_index.saturating_sub(1);
    }
    if active_index >= section.tabs.len() {
        active_index = section.tabs.len() - 1;
    }
    section.active_tab_id = section.tabs[active_index].id.clone();
    Some(section.active_tab_id.clone())
}

fn toggle_tab_pinned_in_section(section: &mut PersistedSectionState, tab_id: &str) -> Option<bool> {
    let index = section.tabs.iter().position(|tab| tab.id == tab_id)?;
    let pinned = !section.tabs[index].pinned;
    section.tabs[index].pinned = pinned;
    let active_tab_id = section.active_tab_id.clone();
    section.tabs.sort_by_key(|tab| !tab.pinned);
    if !section.tabs.iter().any(|tab| tab.id == active_tab_id) {
        section.active_tab_id = section
            .tabs
            .last()
            .map(|tab| tab.id.clone())
            .unwrap_or_default();
    }
    Some(pinned)
}

fn selected_agent_launch_config(agent_ids: &[String]) -> TerminalLaunchConfig {
    let selected = agent_ids
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    another_one_core::agents::terminal_launch_config_for_selected_agents(&selected)
}

fn ensure_known_agent_id(agent_id: &str) -> Result<(), String> {
    if another_one_core::agents::AGENTS
        .iter()
        .any(|agent| agent.id == agent_id)
    {
        Ok(())
    } else {
        Err(format!("unknown agent_id `{agent_id}`"))
    }
}

fn parse_open_in_app_id(app_id: &str) -> Option<OpenInAppKind> {
    match app_id {
        "cursor" => Some(OpenInAppKind::Cursor),
        "zed" => Some(OpenInAppKind::Zed),
        "vscode" => Some(OpenInAppKind::VsCode),
        "file-manager" => Some(OpenInAppKind::FileManager),
        _ => None,
    }
}

fn project_action_from_wire(action: ProjectActionWire) -> Result<ProjectAction, String> {
    Ok(ProjectAction {
        id: action.id,
        name: action.name,
        icon: map_project_action_icon_back(action.icon),
        run_on_worktree_create: action.run_on_worktree_create,
        scope: map_project_action_scope_back(action.scope),
        kind: map_project_action_kind_back(action.kind)?,
    })
}

fn map_project_action_kind_back(kind: ProjectActionKindWire) -> Result<ProjectActionKind, String> {
    match kind {
        ProjectActionKindWire::Shell { command } => Ok(ProjectActionKind::Shell { command }),
        ProjectActionKindWire::Agent {
            prompt,
            provider,
            model,
            traits,
            mode,
            access,
        } => Ok(ProjectActionKind::Agent {
            prompt,
            provider: project_action_provider_from_wire(provider)?,
            model,
            traits,
            mode,
            access: map_project_action_access_back(access),
        }),
    }
}

fn project_action_provider_from_wire(provider: AgentProvider) -> Result<AgentProviderKind, String> {
    match provider {
        AgentProvider::ClaudeCode => Ok(AgentProviderKind::ClaudeCode),
        AgentProvider::CursorAgent => Ok(AgentProviderKind::CursorAgent),
        AgentProvider::Codex => Ok(AgentProviderKind::Codex),
        AgentProvider::Pi => Ok(AgentProviderKind::Pi),
        AgentProvider::Gemini => Ok(AgentProviderKind::Gemini),
        AgentProvider::OpenCode => Ok(AgentProviderKind::OpenCode),
        AgentProvider::Amp => Ok(AgentProviderKind::Amp),
        AgentProvider::RovoDev => Ok(AgentProviderKind::RovoDev),
        AgentProvider::Forge => Ok(AgentProviderKind::Forge),
        AgentProvider::Shell => Err("agent actions require a concrete provider".to_string()),
    }
}

fn map_project_action_icon_back(icon: ProjectActionIconWire) -> ProjectActionIcon {
    match icon {
        ProjectActionIconWire::Play => ProjectActionIcon::Play,
        ProjectActionIconWire::Test => ProjectActionIcon::Test,
        ProjectActionIconWire::Lint => ProjectActionIcon::Lint,
        ProjectActionIconWire::Configure => ProjectActionIcon::Configure,
        ProjectActionIconWire::Build => ProjectActionIcon::Build,
        ProjectActionIconWire::Debug => ProjectActionIcon::Debug,
        ProjectActionIconWire::Agent => ProjectActionIcon::Agent,
    }
}

fn map_project_action_scope_back(scope: ProjectActionScopeWire) -> ProjectActionScope {
    match scope {
        ProjectActionScopeWire::Project => ProjectActionScope::Project,
        ProjectActionScopeWire::Global => ProjectActionScope::Global,
    }
}

fn map_project_action_access_back(access: ProjectActionAccessWire) -> ProjectActionAccess {
    match access {
        ProjectActionAccessWire::Default => ProjectActionAccess::Default,
        ProjectActionAccessWire::ReadOnly => ProjectActionAccess::ReadOnly,
        ProjectActionAccessWire::WorkspaceWrite => ProjectActionAccess::WorkspaceWrite,
        ProjectActionAccessWire::FullAccess => ProjectActionAccess::FullAccess,
    }
}

/// Wire `frame::AgentProvider` → core `AgentProviderKind`. Mirror of
/// the same-named helper in `api/local_session.rs`; the wire enum's
/// `Shell` variant has no core counterpart (it represents "no agent,
/// just a shell" — the caller treats `Some(Shell)` like `None`
/// upstream of this fn, but the match is exhaustive).
fn map_agent_provider_back(kind: AgentProvider) -> AgentProviderKind {
    match kind {
        AgentProvider::ClaudeCode => AgentProviderKind::ClaudeCode,
        AgentProvider::CursorAgent => AgentProviderKind::CursorAgent,
        AgentProvider::Codex => AgentProviderKind::Codex,
        AgentProvider::Pi => AgentProviderKind::Pi,
        AgentProvider::Gemini => AgentProviderKind::Gemini,
        AgentProvider::OpenCode => AgentProviderKind::OpenCode,
        AgentProvider::Amp => AgentProviderKind::Amp,
        AgentProvider::RovoDev => AgentProviderKind::RovoDev,
        AgentProvider::Forge => AgentProviderKind::Forge,
        AgentProvider::Shell => AgentProviderKind::ClaudeCode,
    }
}

fn map_pull_request_state(
    state: another_one_core::git_actions::PullRequestState,
) -> PullRequestState {
    match state {
        another_one_core::git_actions::PullRequestState::Open => PullRequestState::Open,
        another_one_core::git_actions::PullRequestState::Closed => PullRequestState::Closed,
        another_one_core::git_actions::PullRequestState::Merged => PullRequestState::Merged,
    }
}

fn map_check(check: another_one_core::git_actions::PullRequestCheck) -> Check {
    Check {
        name: check.name,
        state: check.state,
        bucket: map_check_bucket(check.bucket),
        description: check.description,
        link: check.link,
        duration_text: check.duration_text,
    }
}

fn map_check_bucket(bucket: another_one_core::git_actions::PullRequestCheckBucket) -> CheckBucket {
    match bucket {
        another_one_core::git_actions::PullRequestCheckBucket::Pass => CheckBucket::Pass,
        another_one_core::git_actions::PullRequestCheckBucket::Fail => CheckBucket::Fail,
        another_one_core::git_actions::PullRequestCheckBucket::Pending => CheckBucket::Pending,
        another_one_core::git_actions::PullRequestCheckBucket::Skipping => CheckBucket::Skipping,
        another_one_core::git_actions::PullRequestCheckBucket::Cancel => CheckBucket::Cancel,
    }
}

fn map_project_page_pr(
    pr: another_one_core::git_actions::ProjectPagePullRequest,
) -> ProjectPagePullRequest {
    ProjectPagePullRequest {
        number: pr.number,
        url: pr.url,
        title: pr.title,
        branch: pr.branch,
        author: pr.author,
        lines_added: pr.lines_added,
        lines_removed: pr.lines_removed,
        draft: pr.draft,
        review_required: pr.review_required,
        review_requested_to_me: pr.review_requested_to_me,
        created_by_me: pr.created_by_me,
        state: map_pull_request_state(pr.state),
    }
}

/// Project a `core::agents::AgentDef` into the wire DTO. Mirrors
/// `api/local_session.rs::agent_def_to_dto`.
fn agent_def_to_wire(agent: &&'static another_one_core::agents::AgentDef) -> AgentSummaryWire {
    AgentSummaryWire {
        id: agent.id.to_string(),
        label: agent.label.to_string(),
        icon_path: agent.icon.to_string(),
        provider: agent.provider.map(map_agent_provider),
    }
}

/// Filter [`OpenInAppKind::all`] down to what the host says is
/// installed, preserving the canonical order. Mirrors
/// `api/local_session.rs::available_open_in_apps`.
fn available_open_in_apps() -> Vec<OpenInAppKind> {
    OpenInAppKind::all()
        .into_iter()
        .filter(|app| <CurrentPlatform as HeadlessPlatform>::is_open_in_app_available(*app))
        .collect()
}

/// Hydrate an [`OpenInAppKind`] into the wire DTO with display
/// strings the mobile UI renders directly.
fn open_in_app_to_wire(app: OpenInAppKind) -> OpenInAppWire {
    OpenInAppWire {
        id: app.id().to_string(),
        label: app.label().to_string(),
        description: app.description().to_string(),
        icon_path: app.icon_path().to_string(),
    }
}

fn project_action_to_wire(action: ProjectAction) -> ProjectActionWire {
    let kind = match action.kind {
        ProjectActionKind::Shell { command } => ProjectActionKindWire::Shell { command },
        ProjectActionKind::Agent {
            prompt,
            provider,
            model,
            traits,
            mode,
            access,
        } => ProjectActionKindWire::Agent {
            prompt,
            provider: map_agent_provider(provider),
            model,
            traits,
            mode,
            access: map_project_action_access(access),
        },
    };
    ProjectActionWire {
        id: action.id,
        name: action.name,
        icon: map_project_action_icon(action.icon),
        run_on_worktree_create: action.run_on_worktree_create,
        scope: map_project_action_scope(action.scope),
        kind,
    }
}

fn map_project_action_icon(icon: ProjectActionIcon) -> ProjectActionIconWire {
    match icon {
        ProjectActionIcon::Play => ProjectActionIconWire::Play,
        ProjectActionIcon::Test => ProjectActionIconWire::Test,
        ProjectActionIcon::Lint => ProjectActionIconWire::Lint,
        ProjectActionIcon::Configure => ProjectActionIconWire::Configure,
        ProjectActionIcon::Build => ProjectActionIconWire::Build,
        ProjectActionIcon::Debug => ProjectActionIconWire::Debug,
        ProjectActionIcon::Agent => ProjectActionIconWire::Agent,
    }
}

fn map_project_action_scope(scope: ProjectActionScope) -> ProjectActionScopeWire {
    match scope {
        ProjectActionScope::Project => ProjectActionScopeWire::Project,
        ProjectActionScope::Global => ProjectActionScopeWire::Global,
    }
}

fn map_project_action_access(access: ProjectActionAccess) -> ProjectActionAccessWire {
    match access {
        ProjectActionAccess::Default => ProjectActionAccessWire::Default,
        ProjectActionAccess::ReadOnly => ProjectActionAccessWire::ReadOnly,
        ProjectActionAccess::WorkspaceWrite => ProjectActionAccessWire::WorkspaceWrite,
        ProjectActionAccess::FullAccess => ProjectActionAccessWire::FullAccess,
    }
}

// ── Settings helpers (`another-one-ojm.8`) ────────────────────────

fn shortcut_action_id(action: another_one_core::shortcuts::ShortcutAction) -> &'static str {
    use another_one_core::shortcuts::ShortcutAction;
    match action {
        ShortcutAction::CycleProjects => "cycle-projects",
        ShortcutAction::NewTabInCurrentTask => "new-tab-in-current-task",
        ShortcutAction::NewTask => "new-task",
        ShortcutAction::CloseCurrentTab => "close-current-tab",
        ShortcutAction::NextTab => "next-tab",
        ShortcutAction::PreviousTab => "previous-tab",
        ShortcutAction::NextTask => "next-task",
        ShortcutAction::PreviousTask => "previous-task",
    }
}

fn parse_shortcut_action_id(id: &str) -> Option<another_one_core::shortcuts::ShortcutAction> {
    use another_one_core::shortcuts::ShortcutAction;
    match id {
        "cycle-projects" => Some(ShortcutAction::CycleProjects),
        "new-tab-in-current-task" => Some(ShortcutAction::NewTabInCurrentTask),
        "new-task" => Some(ShortcutAction::NewTask),
        "close-current-tab" => Some(ShortcutAction::CloseCurrentTab),
        "next-tab" => Some(ShortcutAction::NextTab),
        "previous-tab" => Some(ShortcutAction::PreviousTab),
        "next-task" => Some(ShortcutAction::NextTask),
        "previous-task" => Some(ShortcutAction::PreviousTask),
        _ => None,
    }
}

fn provider_id_str(p: AgentProviderKind) -> &'static str {
    match p {
        AgentProviderKind::ClaudeCode => "claude-code",
        AgentProviderKind::CursorAgent => "cursor-agent",
        AgentProviderKind::Codex => "codex",
        AgentProviderKind::Pi => "pi",
        AgentProviderKind::Gemini => "gemini",
        AgentProviderKind::OpenCode => "opencode",
        AgentProviderKind::Amp => "amp",
        AgentProviderKind::RovoDev => "rovo-dev",
        AgentProviderKind::Forge => "forge",
    }
}

fn parse_provider_id(id: &str) -> Option<AgentProviderKind> {
    match id {
        "claude-code" => Some(AgentProviderKind::ClaudeCode),
        "cursor-agent" => Some(AgentProviderKind::CursorAgent),
        "codex" => Some(AgentProviderKind::Codex),
        "pi" => Some(AgentProviderKind::Pi),
        "gemini" => Some(AgentProviderKind::Gemini),
        "opencode" => Some(AgentProviderKind::OpenCode),
        "amp" => Some(AgentProviderKind::Amp),
        "rovo-dev" => Some(AgentProviderKind::RovoDev),
        "forge" => Some(AgentProviderKind::Forge),
        _ => None,
    }
}

fn ensure_builtin_daemon_mcp_entry(registry: &mut another_one_core::mcp::registry::McpRegistry) {
    let shim_path = resolve_builtin_daemon_mcp_shim_path();
    let socket_path = daemon_sandbox::transport_mcp::default_socket_path();
    registry.ensure_builtin(another_one_core::mcp::catalog::daemon_catalog_entry(
        &shim_path,
        &socket_path,
    ));
}

fn resolve_builtin_daemon_mcp_shim_path() -> std::path::PathBuf {
    let shim_name = if cfg!(target_os = "windows") {
        "another-one-mcp-shim.exe"
    } else {
        "another-one-mcp-shim"
    };

    let Some(current_exe) = std::env::current_exe().ok() else {
        return std::path::PathBuf::from(shim_name);
    };

    if let Some(parent) = current_exe.parent() {
        let sibling = parent.join(shim_name);
        if sibling.exists() {
            return sibling;
        }
    }

    for ancestor in current_exe.ancestors() {
        for profile in ["debug", "release"] {
            let candidate = ancestor.join("target").join(profile).join(shim_name);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    std::path::PathBuf::from(shim_name)
}

fn mcp_server_to_wire(server: &another_one_core::mcp::McpServer) -> McpServerDto {
    let enabled_for = [
        AgentProviderKind::ClaudeCode,
        AgentProviderKind::CursorAgent,
        AgentProviderKind::Codex,
        AgentProviderKind::Gemini,
        AgentProviderKind::OpenCode,
        AgentProviderKind::Amp,
    ]
    .into_iter()
    .filter(|p| server.enabled_for.contains(p))
    .map(provider_id_str)
    .map(str::to_string)
    .collect();
    McpServerDto {
        id: server.id.clone(),
        label: server.label.clone(),
        source: match server.source {
            another_one_core::mcp::McpSource::Catalog => McpSourceDto::Catalog,
            another_one_core::mcp::McpSource::Custom => McpSourceDto::Custom,
            another_one_core::mcp::McpSource::BuiltInDaemon => McpSourceDto::BuiltInDaemon,
        },
        transport_kind: match server.transport {
            another_one_core::mcp::McpTransport::Stdio { .. } => McpTransportKindDto::Stdio,
            another_one_core::mcp::McpTransport::Http { .. } => McpTransportKindDto::Http,
        },
        enabled_for,
    }
}

/// Flatten the bridge's `RegistryState` into the iroh wire's
/// `frame::ProjectSummary` shape. Mirrors `flatten_project_store`
/// in `api/local_session.rs` (which produces the FRB-side
/// `ProjectSummary`); the two namespaces are field-for-field
/// compatible. Worktree-kind projects collapse into their root via
/// `Task::target_project_id`, so mobile peers see the same tree
/// the desktop sidebar paints.
fn flatten_state_to_frame(state: &RegistryState) -> Vec<ProjectSummary> {
    let store = &state.project_store;
    store
        .projects
        .iter()
        .filter(|project| matches!(project.kind, CoreProjectKind::Root))
        .map(|project| project_to_frame(state, project))
        .collect()
}

fn project_to_frame(
    state: &RegistryState,
    project: &another_one_core::project_store::Project,
) -> ProjectSummary {
    let store = &state.project_store;
    let tasks = store
        .tasks
        .get(&project.id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|task| task_to_summary(state, task))
        .collect();
    ProjectSummary {
        id: project.id.clone(),
        name: project.name.clone(),
        path: project.path.to_string_lossy().into_owned(),
        kind: map_project_kind(project.kind),
        current_branch: project.checkout.current_branch.clone(),
        tasks,
    }
}

/// Project a single owned `core::project_store::Task` into the iroh
/// wire's [`TaskSummary`]. Same contract as the inline conversion
/// in [`flatten_state_to_frame`]; lifted into its own helper so the
/// task-mutation reply paths (`TaskCreated`, `TaskRenamed`, etc.)
/// can build inline snapshots without re-flattening the whole
/// project tree.
fn task_to_summary(
    state: &RegistryState,
    task: another_one_core::project_store::Task,
) -> TaskSummary {
    let store = &state.project_store;
    let section_key = task.section_id.clone();
    let parsed_section = SectionId::from_store_key(&section_key);
    let task_pinned = store.ui.pinned_task_ids.contains(&task.id);
    let tabs = task
        .tabs
        .into_iter()
        .map(|tab| {
            let running = parsed_section
                .as_ref()
                .map(|section| TerminalRuntimeKey {
                    section_id: section.clone(),
                    tab_id: tab.id.clone(),
                })
                .map(|key| state.broadcasts.contains_key(&key))
                .unwrap_or(false);
            TabSummary {
                id: tab.id,
                title: tab.title,
                provider: tab.provider.map(map_agent_provider),
                running,
                pinned: tab.pinned,
                fixed_title: tab.fixed_title,
                restore_status: tab.restore_status,
                failure_message: tab.failure_message,
                failure_details: tab.failure_details,
            }
        })
        .collect();
    let branch_view = store.branch_view(&task.target_project_id, &task.branch_name);
    let last_commit_relative = branch_view
        .as_ref()
        .map(|branch| branch.last_commit_relative.clone())
        .unwrap_or_default();
    let (lines_added, lines_removed) = branch_view
        .map(|branch| (branch.lines_added, branch.lines_removed))
        .unwrap_or((0, 0));
    TaskSummary {
        id: task.id,
        name: task.name,
        section_id: section_key,
        branch_name: task.branch_name,
        active_tab_id: task.active_tab_id,
        tabs,
        pinned: task_pinned,
        last_commit_relative,
        lines_added,
        lines_removed,
        target_project_id: task.target_project_id,
    }
}

/// Look up a task by id in the registry's project store and project
/// it as a [`TaskSummary`]. Returns `None` for an unknown id.
fn lookup_task_summary(state: &RegistryState, task_id: &str) -> Option<TaskSummary> {
    let task = state.project_store.task(task_id)?.clone();
    Some(task_to_summary(state, task))
}

fn map_project_kind(kind: CoreProjectKind) -> ProjectKind {
    match kind {
        CoreProjectKind::Root => ProjectKind::Root,
        CoreProjectKind::Worktree => ProjectKind::Worktree,
    }
}

fn map_agent_provider(kind: AgentProviderKind) -> AgentProvider {
    match kind {
        AgentProviderKind::ClaudeCode => AgentProvider::ClaudeCode,
        AgentProviderKind::CursorAgent => AgentProvider::CursorAgent,
        AgentProviderKind::Codex => AgentProvider::Codex,
        AgentProviderKind::Pi => AgentProvider::Pi,
        AgentProviderKind::Gemini => AgentProvider::Gemini,
        AgentProviderKind::OpenCode => AgentProvider::OpenCode,
        AgentProviderKind::Amp => AgentProvider::Amp,
        AgentProviderKind::RovoDev => AgentProvider::RovoDev,
        AgentProviderKind::Forge => AgentProvider::Forge,
        // `frame::AgentProvider::Shell` is the wire-side catch-all
        // for `tab.provider == None`; the bridge maps via
        // `.map(map_agent_provider)`, so a `None` core provider
        // stays `None` on the wire (mobile renders it as Shell on
        // its end). No core `Shell` variant exists, intentionally.
    }
}

fn changed_file_to_wire(f: another_one_core::project_store::ChangedFile) -> ChangedFileWire {
    ChangedFileWire {
        path: f.path,
        original_path: f.original_path,
        staged_additions: f.staged_additions,
        staged_deletions: f.staged_deletions,
        unstaged_additions: f.unstaged_additions,
        unstaged_deletions: f.unstaged_deletions,
        index_status: f.index_status.to_string(),
        worktree_status: f.worktree_status.to_string(),
        untracked: f.untracked,
    }
}

fn changed_file_from_wire(f: ChangedFileWire) -> another_one_core::project_store::ChangedFile {
    another_one_core::project_store::ChangedFile {
        path: f.path,
        original_path: f.original_path,
        staged_additions: f.staged_additions,
        staged_deletions: f.staged_deletions,
        unstaged_additions: f.unstaged_additions,
        unstaged_deletions: f.unstaged_deletions,
        index_status: f.index_status.chars().next().unwrap_or(' '),
        worktree_status: f.worktree_status.chars().next().unwrap_or(' '),
        untracked: f.untracked,
    }
}

fn commit_to_wire(c: another_one_core::project_store::BranchCommit) -> CommitWire {
    CommitWire {
        id: c.id,
        short_id: c.short_id,
        subject: c.subject,
        author_name: c.author_name,
        authored_relative: c.authored_relative,
    }
}

fn branch_compare_file_to_wire(
    f: another_one_core::project_store::BranchCompareFile,
) -> BranchCompareFileWire {
    BranchCompareFileWire {
        path: f.path,
        original_path: f.original_path,
        status: f.status.to_string(),
        additions: f.additions,
        deletions: f.deletions,
    }
}

fn parse_toolbar_action_id(
    id: &str,
) -> anyhow::Result<another_one_core::git_actions::ToolbarGitAction> {
    use another_one_core::git_actions::ToolbarGitAction;
    Ok(match id {
        "commit" => ToolbarGitAction::Commit,
        "commit-and-push" => ToolbarGitAction::CommitAndPush,
        "undo-last-commit" => ToolbarGitAction::UndoLastCommit,
        "fetch" => ToolbarGitAction::Fetch,
        "pull" => ToolbarGitAction::Pull,
        "push" => ToolbarGitAction::Push { force: false },
        "force-push" => ToolbarGitAction::Push { force: true },
        "create-pr" => ToolbarGitAction::CreatePr {
            draft: false,
            base_branch: None,
        },
        "create-draft-pr" => ToolbarGitAction::CreatePr {
            draft: true,
            base_branch: None,
        },
        other => {
            return Err(anyhow::anyhow!(
                "run_toolbar_git_action: unknown action_id `{other}`"
            ));
        }
    })
}

/// Common scaffolding for the stage / unstage / discard / stage-all /
/// unstage-all `DaemonRegistry` mutators: resolve `project_id` to a
/// path, spawn-blocking the git invocation, then re-read
/// `read_project_git_state` so the caller's ack carries the inline
/// post-mutation `changed_files` snapshot per the foundation's
/// inline-snapshot contract. Mirrors `LocalSession`'s
/// `run_changed_file_action` helper but returns the snapshot rather
/// than `()` (the iroh wire wants the snapshot to ride the ack
/// frame, not a separate `read_changed_files` round-trip).
async fn run_changed_file_mutation<F>(
    inner: &Weak<Mutex<another_one_core::daemon_embed::RegistryState>>,
    verb_label: &'static str,
    project_id: &str,
    mutate: F,
) -> anyhow::Result<Vec<ChangedFileWire>>
where
    F: FnOnce(std::path::PathBuf) -> Result<(), String> + Send + 'static,
{
    let project_path = resolve_project_path(inner, project_id)
        .ok_or_else(|| anyhow::anyhow!("{verb_label}: unknown project_id `{project_id}`"))?;
    let project_path_for_mutate = project_path.clone();
    tokio::task::spawn_blocking(move || mutate(project_path_for_mutate))
        .await
        .map_err(|e| anyhow::anyhow!("{verb_label} join: {e}"))?
        .map_err(|e| anyhow::anyhow!(e))?;
    let project_path_for_read = project_path.clone();
    let git_state = tokio::task::spawn_blocking(move || {
        another_one_core::project_store::read_project_git_state(&project_path_for_read, false)
    })
    .await
    .map_err(|e| anyhow::anyhow!("{verb_label} post-read join: {e}"))?;
    Ok(git_state
        .changed_files
        .into_iter()
        .map(changed_file_to_wire)
        .collect())
}

/// Resolve a `project_id` to its absolute path on disk by reading
/// from the bridge's `RegistryState`. Returns `None` for unknown
/// ids; the caller turns that into an `Err` with a verb-specific
/// message so logs name the offending verb.
fn resolve_project_path(
    inner: &Weak<Mutex<another_one_core::daemon_embed::RegistryState>>,
    project_id: &str,
) -> Option<std::path::PathBuf> {
    let arc = inner.upgrade()?;
    let state = arc.lock().ok()?;
    state
        .project_store
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .map(|project| project.path.clone())
}
