//! Application state, core event handlers, animation, and `Render` impl.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::sync::broadcast;

use gpui::{
    actions, div, hsla, img, prelude::*, px, rems, rgb, svg, AnyElement, AnyView, App, Bounds,
    ClipboardEntry, ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, Image, InspectorElementId,
    LayoutId, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ObjectFit, Pixels, Point, Render, ScrollDelta, ShapedLine, SharedString, Size, UTF16Selection,
    WeakEntity, Window,
};

actions!(zoom, [ZoomIn, ZoomOut, ZoomReset]);
actions!(
    terminal_search,
    [
        TerminalFind,
        TerminalSearchClose,
        TerminalSearchNext,
        TerminalSearchPrev,
        TerminalSearchBackspace
    ]
);
actions!(
    navigation,
    [
        NextTab,
        PreviousTab,
        NextTask,
        PreviousTask,
        NextProject,
        NewTab,
        NewTask
    ]
);

use crate::agents::{
    agent_id_for_provider, agent_output_indicates_missing_session, effective_enabled_agents,
    terminal_launch_config_for_selected_agent, terminal_launch_config_for_selected_agents,
    AgentDef, TerminalLaunchConfig, TerminalSessionRef, AGENTS,
};
use crate::background_ops::{BroadcastOperation, BroadcastOperationEvent};
use crate::git_workspace::GitWorkspace;
use crate::layout::*;
use crate::mobile::{self, MobileView};
use crate::open_in::{detect_available_open_in_apps, open_path_in_app, OpenInAppKind};
use crate::panels::terminal_cell_width;
use crate::platform::PlatformServices;
use crate::project_store::{
    ChangedFile, InvalidProjectBranchSetting, PersistedSectionState, PersistedTerminalTab,
    ProjectAction, ProjectActionKind, ProjectBranchCommitState, ProjectBranchSettingField,
    ProjectGitState, ProjectStore, RepoBranchRecord, Task, TaskKind,
    TaskWorktreeBranchMode,
};
use crate::resource_usage::{ResourceUsageSampler, ResourceUsageSnapshot, TrackedProcess};
use crate::task_launcher::{PendingTaskLaunch, TaskLaunchRequest};
use crate::terminal_launch::{
    spawn_terminal_launch, spawn_warm_terminal_launch, TerminalLaunchReply, WarmTerminalLaunchReply,
};
use crate::terminal_runtime::{
    LiveTerminalRuntime, TerminalGridSize, TerminalMouseEncoding, TerminalMouseLevel,
    TerminalMouseProtocol, TerminalRuntimeKey, TerminalRuntimeUpdate, TerminalScrollbackMatch,
    TerminalSurfaceSnapshot, TERMINAL_LINE_HEIGHT_RATIO,
};
use crate::theme;
use another_one_core::clients::{
    AttachTabRequest, AttachTabResponse, ClientEvent, ClientId, CloseTabRequest, Focus, JobId,
    OpenTabRequest, OpenTabResponse, OpenTaskRequest, OpenTaskResponse, SelectRequest,
};
pub use another_one_core::section::SectionId;
use daemon_proto::TerminalRestoreStatus;

const ACTIVE_GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(4);
const ACTIVE_GIT_METADATA_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const IDLE_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const RESOURCE_REFRESH_INTERVAL_OPEN: Duration = Duration::from_secs(1);
const RESOURCE_REFRESH_INTERVAL_CLOSED: Duration = Duration::from_secs(5);
const TOAST_ANIMATION_REFRESH_INTERVAL: Duration = Duration::from_millis(16);
const TERMINAL_FAST_REFRESH_GRACE: Duration = Duration::from_millis(300);
const TOAST_LIFETIME: Duration = Duration::from_secs(4);
const TOAST_ERROR_EXTRA_LIFETIME: Duration = Duration::from_secs(3);
const TOAST_FADE_IN: Duration = Duration::from_millis(220);
const TOAST_FADE_OUT: Duration = Duration::from_millis(220);
const PASTED_IMAGE_PREVIEW_LIFETIME: Duration = Duration::from_secs(4);
const TOAST_STACK_LIMIT: usize = 4;
const TOAST_SWIPE_DISMISS_THRESHOLD: f32 = 120.;
const TOAST_COPY_FEEDBACK: Duration = Duration::from_millis(1200);
const PULL_REQUEST_LOOKUP_TTL: Duration = Duration::from_secs(30);
const CHECK_RUNS_LOOKUP_TTL: Duration = Duration::from_secs(30);
const PENDING_CHECK_RUNS_LOOKUP_TTL: Duration = Duration::from_secs(10);
pub(crate) const SIDEBAR_TASK_DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(400);
const PROJECT_EXPAND_ANIMATION_DURATION: Duration = Duration::from_millis(160);
const PROJECT_EXPAND_ANIMATION_STEP: Duration = Duration::from_millis(16);
/// How long a terminal bell flash stays visible. Short enough to feel
/// like a glance, long enough to register.
pub(crate) const BELL_FLASH_DURATION: Duration = Duration::from_millis(180);
pub(crate) const RECENT_COMMITS_PAGE_SIZE: usize = 20;

/// Max queued `TerminalLaunchReply`s between PTY reader threads and
/// the GPUI drain. PTY reader threads produce ~one 8 KiB chunk per
/// successful `read()`, so this cap bounds the in-flight memory for
/// terminal output at roughly `8 KiB × CAP`. At 2048 that's ~16 MiB
/// per channel — enough that brief (≈2 s at 8 MiB/s steady output)
/// UI stalls drain cleanly without blocking, but tight enough that a
/// real stall applies backpressure instead of ballooning RSS.
///
/// This constant replaced an unbounded `mpsc::channel()`. With
/// multiple chatty agent tabs producing bytes faster than the GPUI
/// render thread could drain them, the unbounded version queued
/// ~37 GiB of pending output before the kernel OOM-killed the
/// process.
const TERMINAL_LAUNCH_QUEUE_CAP: usize = 2048;
/// Same shape as [`TERMINAL_LAUNCH_QUEUE_CAP`] but for prewarmed-tab
/// output. Prewarmed tabs emit far less output (they're idle at a
/// shell prompt until attached), so a matching cap is plenty of
/// headroom for the spiky-launch case and still keeps overall
/// in-flight memory in the tens of MiB.
const WARM_TERMINAL_LAUNCH_QUEUE_CAP: usize = 2048;

/// Ceiling on how many `Output` bytes the GPUI main thread will
/// parse + render within a single drain tick before yielding back
/// to the GPUI run loop. The drain used to greedily consume every
/// pending reply, so a CDP-sized burst (several MiB across tens of
/// chunks) would pay the full alacritty VT parse cost on one tick
/// and stall the main thread for hundreds of ms. See #127 / the
/// #128 watchdog data that motivated it.
///
/// 64 KiB keeps a single drain tick comfortably under one 60 Hz
/// frame on the parse hot path while still clearing a typical
/// interactive burst (`ls -la`, a prompt redraw, a test summary)
/// in one tick. Larger values (256 KiB was the first pass) still
/// blew the frame budget once a burst was dense enough. The
/// leftover sits in the bounded channel and gets picked up on the
/// next refresh tick (16 ms fast / 250 ms idle); the channel cap
/// absorbs many ticks of backlog before backpressuring the reader,
/// which is the intended behavior when the child is outpacing the
/// UI (see [`TERMINAL_LAUNCH_QUEUE_CAP`]'s comment).
const DRAIN_OUTPUT_BYTE_CAP: usize = 64 * 1024;

fn new_tab_seed_agent_id(
    state: Option<&SectionState>,
    default_agent_id: Option<&str>,
) -> Option<String> {
    state?;
    default_agent_id.map(str::to_string)
}

fn resolved_task_name(task_name: &str, generated_task_name: &str) -> String {
    let task_name = task_name.trim();
    if task_name.is_empty() {
        generated_task_name.to_string()
    } else {
        task_name.to_string()
    }
}

/// A single terminal tab within a section.
pub struct TerminalTab {
    pub id: String,
    pub title: String,
    pub pinned: bool,
    pub fixed_title: Option<String>,
    pub launch_config: TerminalLaunchConfig,
    pub restore_status: TerminalRestoreStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalSelectionRange {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalLinkRange {
    pub line: usize,
    pub start_column: usize,
    pub end_column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalCellPosition {
    line: usize,
    column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalSelectionState {
    key: TerminalRuntimeKey,
    anchor: TerminalCellPosition,
    head: TerminalCellPosition,
    dragging: bool,
    /// While dragging past the top/bottom of the viewport, the
    /// refresh tick auto-scrolls by this many lines per tick. Sign
    /// = direction (+ = scroll up / older, - = scroll down / newer);
    /// magnitude scales with how far past the edge the pointer is
    /// so a small overshoot inches and a far overshoot races. Zero
    /// means the pointer is inside the viewport or drag ended.
    autoscroll_dir: i32,
}

#[derive(Clone, Debug, PartialEq)]
struct TerminalPanelMetrics {
    key: TerminalRuntimeKey,
    left: f32,
    top: f32,
    padding: f32,
    cell_width: f32,
    cell_height: f32,
    columns: usize,
    rows: usize,
}

struct PrewarmedTerminalLaunch {
    cwd: std::path::PathBuf,
    launch_config: TerminalLaunchConfig,
    attached_tab: Option<TerminalRuntimeKey>,
    runtime: Option<LiveTerminalRuntime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabCloseScope {
    Other,
    Left,
    Right,
}

/// Per-section state: which terminal tabs are open and which is active.
pub struct SectionState {
    pub tabs: Vec<TerminalTab>,
    pub active_tab: usize, // index into tabs
    next_tab_id: usize,
    /// Working directory for new terminals in this section.
    pub cwd: Option<std::path::PathBuf>,
}

impl SectionState {
    pub fn with_cwd(cwd: Option<std::path::PathBuf>) -> Self {
        Self::with_initial_tab(cwd, TerminalLaunchConfig::default())
    }

    pub fn with_initial_tab(
        cwd: Option<std::path::PathBuf>,
        launch_config: TerminalLaunchConfig,
    ) -> Self {
        Self {
            tabs: vec![TerminalTab::new(launch_config)],
            active_tab: 0,
            next_tab_id: 1,
            cwd,
        }
    }

    pub fn add_tab_with_launch_config(
        &mut self,
        launch_config: TerminalLaunchConfig,
        fixed_title: Option<String>,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.next_tab_id += 1;
        let new_tab = TerminalTab::with_id(id.clone(), launch_config, fixed_title);
        self.tabs.push(new_tab);
        self.active_tab = self.tabs.len() - 1;
        id
    }

    pub fn close_tab(&mut self, index: usize) -> Option<String> {
        if index >= self.tabs.len() {
            return None;
        }

        let removed = self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active_tab = 0;
        } else {
            if index < self.active_tab {
                self.active_tab = self.active_tab.saturating_sub(1);
            }
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
        }
        Some(removed.id)
    }

    pub(crate) fn close_tabs_by_ids(
        &mut self,
        tab_ids: impl IntoIterator<Item = String>,
    ) -> Vec<String> {
        let tab_ids = tab_ids.into_iter().collect::<HashSet<_>>();
        if tab_ids.is_empty() {
            return Vec::new();
        }

        let mut removed = Vec::new();
        for index in (0..self.tabs.len()).rev() {
            if tab_ids.contains(&self.tabs[index].id) {
                if let Some(tab_id) = self.close_tab(index) {
                    removed.push(tab_id);
                }
            }
        }
        removed.reverse();
        removed
    }

    pub(crate) fn tab_ids_for_close_scope(
        &self,
        anchor_tab_id: &str,
        scope: TabCloseScope,
    ) -> Vec<String> {
        let Some(anchor_index) = self.tabs.iter().position(|tab| tab.id == anchor_tab_id) else {
            return Vec::new();
        };

        self.tabs
            .iter()
            .enumerate()
            .filter(|(index, _)| match scope {
                TabCloseScope::Other => *index != anchor_index,
                TabCloseScope::Left => *index < anchor_index,
                TabCloseScope::Right => *index > anchor_index,
            })
            .map(|(_, tab)| tab.id.clone())
            .collect()
    }

    #[cfg(test)]
    pub fn tab_is_pinned(&self, index: usize) -> bool {
        self.tabs.get(index).is_some_and(|tab| tab.pinned)
    }

    pub fn set_tab_pinned(&mut self, index: usize, pinned: bool) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        if self.tabs[index].pinned == pinned {
            return false;
        }

        let active_tab_id = self.active_tab_id();
        self.tabs[index].pinned = pinned;
        self.sort_tabs_by_pin();
        self.active_tab = self
            .tabs
            .iter()
            .position(|tab| tab.id == active_tab_id)
            .unwrap_or_else(|| self.tabs.len().saturating_sub(1));
        true
    }

    fn sort_tabs_by_pin(&mut self) {
        self.tabs.sort_by_key(|tab| !tab.pinned);
    }

    pub fn activate_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }

        self.active_tab = index;
        true
    }

    pub fn active_tab_id(&self) -> String {
        self.tabs
            .get(self.active_tab)
            .map(|tab| tab.id.clone())
            .unwrap_or_default()
    }

    pub fn update_cwd(&mut self, cwd: Option<std::path::PathBuf>) -> bool {
        if self.cwd == cwd {
            return false;
        }
        self.cwd = cwd;
        true
    }

    pub fn to_persisted(&self) -> PersistedSectionState {
        PersistedSectionState {
            active_tab_id: self.active_tab_id(),
            next_tab_id: self.next_tab_id,
            cwd: self.cwd.clone(),
            tabs: self.tabs.iter().map(TerminalTab::to_persisted).collect(),
        }
    }

    pub fn from_persisted(
        persisted: PersistedSectionState,
        fallback_cwd: Option<std::path::PathBuf>,
    ) -> Self {
        let tabs = persisted
            .tabs
            .into_iter()
            .map(TerminalTab::from_persisted)
            .collect::<Vec<_>>();
        let mut tabs = tabs;
        tabs.sort_by_key(|tab| !tab.pinned);

        let active_tab = if tabs.is_empty() {
            0
        } else {
            tabs.iter()
                .position(|tab| tab.id == persisted.active_tab_id)
                .unwrap_or_else(|| tabs.len().saturating_sub(1))
        };

        Self {
            tabs,
            active_tab,
            next_tab_id: persisted.next_tab_id.max(1),
            cwd: persisted.cwd.or(fallback_cwd),
        }
    }
}

impl TerminalTab {
    fn new(launch_config: TerminalLaunchConfig) -> Self {
        Self::with_id(uuid::Uuid::new_v4().to_string(), launch_config, None)
    }

    fn with_id(
        id: String,
        launch_config: TerminalLaunchConfig,
        fixed_title: Option<String>,
    ) -> Self {
        let title = fixed_title
            .clone()
            .unwrap_or_else(|| launch_config.default_title());
        Self {
            id,
            title,
            pinned: false,
            fixed_title,
            launch_config,
            restore_status: TerminalRestoreStatus::NotStarted,
        }
    }

    fn to_persisted(&self) -> PersistedTerminalTab {
        PersistedTerminalTab {
            id: self.id.clone(),
            title: self.title.clone(),
            pinned: self.pinned,
            fixed_title: self.fixed_title.clone(),
            provider: self.launch_config.provider,
            launch_config: Some(self.launch_config.clone()),
            restore_status: self.restore_status,
            failure_message: None,
            failure_details: None,
        }
    }

    fn from_persisted(persisted: PersistedTerminalTab) -> Self {
        let launch_config = persisted.launch_config.unwrap_or_else(|| {
            if let Some(provider) = persisted.provider {
                TerminalLaunchConfig::for_provider(provider)
            } else {
                TerminalLaunchConfig::default()
            }
        });

        Self {
            id: persisted.id,
            title: persisted.title,
            pinned: persisted.pinned,
            fixed_title: persisted.fixed_title,
            launch_config,
            restore_status: persisted.restore_status,
        }
    }
}

fn apply_terminal_title_update(tab: &mut TerminalTab, terminal_update: &TerminalRuntimeUpdate) {
    if tab.fixed_title.is_some() {
        return;
    }

    if terminal_update.reset_title {
        tab.title = tab.launch_config.default_title();
    } else if let Some(title) = &terminal_update.title {
        tab.title = title.clone();
    }
}

fn fixed_title_for_project_action(action: &ProjectAction) -> Option<String> {
    if !matches!(&action.kind, ProjectActionKind::Shell { .. }) {
        return None;
    }

    let name = action.name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

// Moved to `another_one_core::git_service::GitRefreshReply`; the
// struct stays named the same at this path so existing call sites
// keep compiling, but the body + the spawn worker now live in core.
use another_one_core::git_service::{
    ChangedFileDiffReply, GitRefreshReply, RemoteBranchRefreshReply,
};

enum GitActionReply {
    Progress {
        toast_kind: ToastKind,
        toast_message: String,
    },
    Finished {
        project_id: String,
        refresh_git_state: bool,
        git_state: Option<ProjectGitState>,
        toast_kind: ToastKind,
        toast_message: String,
    },
}

pub(crate) struct ActiveToolbarGitAction {
    pub(crate) action: crate::git_actions::ToolbarGitAction,
    pub(crate) branch_name_at_start: Option<String>,
    receiver: mpsc::Receiver<GitActionReply>,
}

enum DrainedGitAction {
    Reply {
        active_project_id: String,
        reply: GitActionReply,
    },
    Disconnected {
        project_id: String,
    },
}

fn active_toolbar_git_action_entry<'a>(
    active_git_actions: &'a HashMap<String, ActiveToolbarGitAction>,
    project_id: &str,
) -> Option<&'a ActiveToolbarGitAction> {
    active_git_actions.get(project_id)
}

fn has_active_toolbar_git_action(
    active_git_actions: &HashMap<String, ActiveToolbarGitAction>,
    project_id: &str,
) -> bool {
    active_toolbar_git_action_entry(active_git_actions, project_id).is_some()
}

fn collect_drained_git_action_replies(
    active_git_actions: &HashMap<String, ActiveToolbarGitAction>,
) -> Vec<DrainedGitAction> {
    let mut drained = Vec::new();
    let active_project_ids: Vec<_> = active_git_actions.keys().cloned().collect();
    for project_id in active_project_ids {
        loop {
            let Some(result) = active_git_actions
                .get(&project_id)
                .map(|active| active.receiver.try_recv())
            else {
                break;
            };

            match result {
                Ok(reply) => {
                    let finished = matches!(reply, GitActionReply::Finished { .. });
                    drained.push(DrainedGitAction::Reply {
                        active_project_id: project_id.clone(),
                        reply,
                    });
                    if finished {
                        break;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    drained.push(DrainedGitAction::Disconnected {
                        project_id: project_id.clone(),
                    });
                    break;
                }
            }
        }
    }
    drained
}

pub(crate) struct WorktreeDeletionReply {
    pub(crate) confirm: SidebarTaskDeleteConfirmState,
    pub(crate) was_active_project: bool,
    pub(crate) result: Result<Option<String>, String>,
}

struct CommitFileChangesReply {
    project_id: String,
    commit_id: String,
    result: Result<Vec<crate::project_store::BranchCompareFile>, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommitFileChangesState {
    Loading,
    Loaded(Arc<[crate::project_store::BranchCompareFile]>),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitDiffPaneState {
    Loading,
    Loaded(Arc<crate::project_store::GitDiff>),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RightSidebarMode {
    WorkingTree,
    Commits,
    Checks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceKeyboardFocus {
    MainPane,
    GitPanel,
}

struct TerminalRuntimeRequest {
    key: TerminalRuntimeKey,
    cwd: std::path::PathBuf,
    launch_config: TerminalLaunchConfig,
    restore_status: TerminalRestoreStatus,
    agent_launch_args: Vec<String>,
    size: TerminalGridSize,
}

// Both types + the worker fn now live in another_one_core::git_service;
// re-exported under the same local names so every existing call site
// (the right-sidebar event handlers, the drain loop, the pending-mutation
// helpers) keeps compiling.
use another_one_core::git_service::{ChangedFilesGitMutation, ChangedFilesGitMutationReply};

#[derive(Clone)]
struct PendingChangedFilesGitMutations {
    confirmed_files: Option<Arc<[ChangedFile]>>,
    in_flight: Option<ChangedFilesGitMutation>,
    queued: VecDeque<ChangedFilesGitMutation>,
}

impl PendingChangedFilesGitMutations {
    fn mutations(&self) -> impl Iterator<Item = &ChangedFilesGitMutation> {
        self.in_flight.iter().chain(self.queued.iter())
    }
}

// Worker bodies + reply types moved to core::project_service;
// re-exported at these paths so existing channel fields and drain
// loops keep compiling. Only the two Reply types are reached from
// outside the drain loop; Success/Failure are inspected via
// `reply.result` at the call site.
use another_one_core::project_service::{ProjectAddReply, TaskCreationReply};

// All four github-lookup reply types + their spawn workers live in
// another_one_core::git_service now; re-exported here so existing
// channel-field types and drain-loop field accesses keep compiling.
use another_one_core::git_service::{
    ProjectCheckRunsReply, ProjectGitHubLinkReply, ProjectPagePullRequestsReply,
    ProjectPullRequestReply,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectCheckRunsState {
    Loading,
    Loaded(Arc<[crate::git_actions::PullRequestCheck]>),
    NoPullRequest,
    Failed(String),
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarTaskRenameState {
    pub(crate) project_id: String,
    pub(crate) row_id: String,
    pub(crate) original_name: String,
    pub(crate) task_name: String,
    pub(crate) task_name_cursor: usize,
    pub(crate) task_name_selection_anchor: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SettingsAgentInputState {
    pub(crate) drafts: HashMap<String, String>,
    pub(crate) focused_agent_id: Option<String>,
    pub(crate) cursor: usize,
    pub(crate) selection_anchor: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct SettingsGitActionScriptInputState {
    pub(crate) draft: String,
    pub(crate) focused: bool,
    pub(crate) cursor: usize,
    pub(crate) selection_anchor: Option<usize>,
}

impl Default for SettingsGitActionScriptInputState {
    fn default() -> Self {
        Self {
            draft: String::new(),
            focused: false,
            cursor: 0,
            selection_anchor: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsGitActionScriptKind {
    Commit,
    PullRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsGitActionLlmDropdown {
    CommitProvider,
    CommitModel,
    CommitThinking,
    PullRequestProvider,
    PullRequestModel,
    PullRequestThinking,
}

#[derive(Clone)]
pub(crate) struct SettingsGitActionScriptLineLayout {
    pub(crate) range: std::ops::Range<usize>,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) line: ShapedLine,
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarTaskMenuState {
    pub(crate) project_id: String,
    pub(crate) root_project_id: String,
    pub(crate) row_id: String,
    pub(crate) task_id: Option<String>,
    pub(crate) task_name: String,
    pub(crate) branch_name: String,
    pub(crate) is_worktree: bool,
    pub(crate) anchor_x: f32,
    pub(crate) anchor_y: f32,
}

/// Cmd-F scrollback search overlay for one terminal pane. Lives at the
/// app level so the highlight set survives renders without re-running
/// the scan on every paint. `current_index` always points at a valid
/// entry of `matches` when non-empty.
#[derive(Debug, Clone)]
pub(crate) struct TerminalSearchState {
    pub(crate) key: TerminalRuntimeKey,
    pub(crate) query: String,
    pub(crate) matches: Vec<TerminalScrollbackMatch>,
    pub(crate) current_index: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TerminalTabMenuState {
    pub(crate) section_id: SectionId,
    pub(crate) tab_id: String,
    pub(crate) anchor_x: f32,
    pub(crate) anchor_y: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct TerminalTabRenameState {
    pub(crate) section_id: SectionId,
    pub(crate) tab_id: String,
    pub(crate) draft: String,
    pub(crate) cursor: usize,
}

/// Per-frame hover hint for a terminal link. Carries the link target
/// (so the tooltip can show what's about to open) and the cursor
/// screen position (so the tooltip renders next to the cursor without
/// occluding the link itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalLinkHoverState {
    pub(crate) section_id: SectionId,
    pub(crate) tab_id: String,
    pub(crate) link: String,
    pub(crate) anchor_x: i32,
    pub(crate) anchor_y: i32,
}

/// Right-click context menu over the terminal pane. Surfaced when the
/// running TUI is NOT consuming mouse events (`mouse_protocol() == None`),
/// otherwise the right-click is forwarded to the application and this
/// state stays `None`.
#[derive(Debug, Clone)]
pub(crate) struct TerminalContextMenuState {
    pub(crate) key: TerminalRuntimeKey,
    pub(crate) anchor_x: f32,
    pub(crate) anchor_y: f32,
    /// If the click landed on a hyperlink, this carries the target — the
    /// menu offers an "Open Link" item only when this is `Some`.
    pub(crate) link: Option<String>,
    /// Selection text captured at menu-open time, so "Copy" works even if
    /// the menu's own click clears the visual selection state.
    pub(crate) selected_text: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PinnedTabCloseConfirmState {
    pub(crate) section_id: SectionId,
    pub(crate) tab_id: String,
    pub(crate) title: String,
    pub(crate) tab_ids: Vec<String>,
    pub(crate) pinned_tab_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarTaskDeleteConfirmState {
    pub(crate) project_id: String,
    pub(crate) root_project_id: String,
    pub(crate) task_id: Option<String>,
    pub(crate) task_name: String,
    pub(crate) branch_name: String,
    pub(crate) project_path: std::path::PathBuf,
    pub(crate) repo_path: std::path::PathBuf,
    pub(crate) is_worktree: bool,
    pub(crate) other_tasks_in_worktree: usize,
    pub(crate) force_delete_branch: bool,
    pub(crate) has_unstaged_changes: bool,
}

#[derive(Clone)]
pub(crate) struct SidebarTaskDeleteRequest {
    pub(crate) project_id: String,
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) branch_name: String,
    pub(crate) is_worktree: bool,
    pub(crate) preferred_project_id: String,
}

#[derive(Clone)]
pub(crate) struct ProjectRemoveConfirmState {
    pub(crate) project_name: String,
    pub(crate) project_ids: Vec<String>,
    pub(crate) open_task_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastKind {
    Success,
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
struct AppToast {
    id: u64,
    kind: ToastKind,
    message: SharedString,
    copy_message: SharedString,
    shown_at: Instant,
    dismiss_at: Instant,
}

impl AppToast {
    fn new(
        id: u64,
        kind: ToastKind,
        message: impl Into<SharedString>,
        shown_at: Instant,
        dismiss_at: Instant,
    ) -> Self {
        let message = message.into();
        Self {
            id,
            kind,
            copy_message: message.clone(),
            message,
            shown_at,
            dismiss_at,
        }
    }

    fn with_copy_message(
        id: u64,
        kind: ToastKind,
        message: impl Into<SharedString>,
        copy_message: impl Into<SharedString>,
        shown_at: Instant,
        dismiss_at: Instant,
    ) -> Self {
        Self {
            id,
            kind,
            message: message.into(),
            copy_message: copy_message.into(),
            shown_at,
            dismiss_at,
        }
    }
}

#[derive(Debug, Clone)]
struct ToastDrag {
    toast_id: u64,
    start_x: f32,
    current_x: f32,
}

#[derive(Debug, Clone)]
struct PastedImagePreview {
    image: Arc<Image>,
    shown_at: Instant,
    dismiss_at: Instant,
}

#[derive(Clone)]
struct ActionTooltip {
    label: SharedString,
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarProjectExpandAnimation {
    pub(crate) progress: f32,
    pub(crate) target_expanded: bool,
    pub(crate) generation: u64,
}

impl Render for ActionTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let border = gpui::white().opacity(0.08);
        let text_col = hsla(0., 0., 0.92, 1.);

        div()
            .px(px(8.))
            .py(px(5.))
            .rounded(px(7.))
            .bg(rgb(0x1e2024))
            .border_1()
            .border_color(border)
            .shadow_md()
            .child(
                div()
                    .text_size(rems(11. / 16.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(text_col)
                    .child(self.label.clone()),
            )
    }
}

pub(crate) struct WorkspacePane {
    pub(crate) app: WeakEntity<AnotherOneApp>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) sidebar_w: f32,
    pub(crate) right_w: f32,
    pub(crate) font_size: f32,
    /// Currently selected section (project + branch).
    pub(crate) active_section: Option<SectionId>,
    /// Currently displayed project page (project_id). Mutually exclusive with terminal view.
    pub(crate) active_project_page: Option<String>,
    /// Currently displayed read-only changed-file diff.
    pub(crate) active_git_diff: Option<crate::project_store::GitDiffSelection>,
    /// Whether the Open PRs section is collapsed on the project page.
    pub(crate) project_page_prs_collapsed: bool,
    /// Active PR filter tab index (0=All Open, 1=Needs My Review, 2=My PRs, 3=Draft).
    pub(crate) project_page_pr_filter: usize,
    pub(crate) project_page_pr_query: String,
    pub(crate) project_page_pr_query_draft: String,
    /// Per-section placeholder tab state.
    pub(crate) section_states: HashMap<SectionId, SectionState>,
    /// Context menu for a terminal tab.
    pub(crate) terminal_tab_menu: Option<TerminalTabMenuState>,
    pub(crate) terminal_tab_rename: Option<TerminalTabRenameState>,
    /// Tooltip state for the link the cursor is currently over inside a
    /// terminal pane. Populated on `on_mouse_move`; cleared on
    /// mouse-leave. The tooltip itself renders alongside the canvas in
    /// `panels.rs`.
    pub(crate) terminal_link_hover: Option<TerminalLinkHoverState>,
    /// Right-click context menu over a terminal pane (Copy/Paste/Open Link).
    pub(crate) terminal_context_menu: Option<TerminalContextMenuState>,
    /// Confirmation state for closing a pinned terminal tab.
    pub(crate) pinned_tab_close_confirm: Option<PinnedTabCloseConfirmState>,
    /// Last workspace region that intentionally claimed bare navigation keys.
    pub(crate) keyboard_focus: WorkspaceKeyboardFocus,
}

impl WorkspacePane {
    pub(crate) fn new(
        app: WeakEntity<AnotherOneApp>,
        focus_handle: FocusHandle,
        sidebar_w: f32,
        right_w: f32,
        font_size: f32,
        active_section: Option<SectionId>,
        section_states: HashMap<SectionId, SectionState>,
    ) -> Self {
        Self {
            app,
            focus_handle,
            sidebar_w,
            right_w,
            font_size,
            active_section,
            active_project_page: None,
            active_git_diff: None,
            project_page_prs_collapsed: false,
            project_page_pr_filter: 0,
            project_page_pr_query: String::new(),
            project_page_pr_query_draft: String::new(),
            section_states,
            terminal_tab_menu: None,
            terminal_tab_rename: None,
            terminal_context_menu: None,
            terminal_link_hover: None,
            pinned_tab_close_confirm: None,
            keyboard_focus: WorkspaceKeyboardFocus::MainPane,
        }
    }

    pub(crate) fn sync_layout(
        &mut self,
        sidebar_w: f32,
        right_w: f32,
        font_size: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut changed = false;

        if (self.sidebar_w - sidebar_w).abs() > f32::EPSILON {
            self.sidebar_w = sidebar_w;
            changed = true;
        }
        if (self.right_w - right_w).abs() > f32::EPSILON {
            self.right_w = right_w;
            changed = true;
        }
        if (self.font_size - font_size).abs() > f32::EPSILON {
            self.font_size = font_size;
            changed = true;
        }

        if changed {
            cx.notify();
        }

        changed
    }

    pub(crate) fn ensure_section(
        &mut self,
        section_id: SectionId,
        cwd: Option<std::path::PathBuf>,
        launch_config: Option<TerminalLaunchConfig>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;

        if let Some(state) = self.section_states.get_mut(&section_id) {
            changed |= state.update_cwd(cwd);
        } else {
            let state = match launch_config {
                Some(launch_config) => SectionState::with_initial_tab(cwd, launch_config),
                None => SectionState::with_cwd(cwd),
            };
            self.section_states.insert(section_id.clone(), state);
            changed = true;
        }

        if changed {
            self.persist_section_state(&section_id, cx);
        }
    }

    pub(crate) fn activate_project_page(
        &mut self,
        project_id: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let project_id = project_id.into();
        let changed = self.active_project_page.as_deref() != Some(project_id.as_str())
            || self.active_section.is_some();
        self.active_project_page = Some(project_id);
        self.active_section = None;
        self.active_git_diff = None;
        self.keyboard_focus = WorkspaceKeyboardFocus::MainPane;
        if changed {
            cx.notify();
        }
    }

    pub(crate) fn activate_section(
        &mut self,
        section_id: SectionId,
        cwd: Option<std::path::PathBuf>,
        launch_config: Option<TerminalLaunchConfig>,
        cx: &mut Context<Self>,
    ) {
        self.ensure_section(section_id.clone(), cwd, launch_config, cx);
        let changed = select_active_section(
            &mut self.active_section,
            &mut self.active_project_page,
            section_id,
        );
        let closed_git_diff = self.active_git_diff.is_some();
        if changed || closed_git_diff {
            self.active_git_diff = None;
            self.keyboard_focus = WorkspaceKeyboardFocus::MainPane;
        }
        self.persist_active_section(cx);
        if changed || closed_git_diff {
            cx.notify();
        }
    }

    pub(crate) fn remove_project_sections(
        &mut self,
        project_ids: &HashSet<String>,
        cx: &mut Context<Self>,
    ) {
        let removed_section_ids = self
            .section_states
            .keys()
            .filter(|section_id| project_ids.contains(&section_id.project_id))
            .cloned()
            .collect::<HashSet<_>>();
        let before_len = self.section_states.len();
        self.section_states
            .retain(|section_id, _| !project_ids.contains(&section_id.project_id));

        let mut changed = before_len != self.section_states.len();

        if self
            .active_section
            .as_ref()
            .is_some_and(|section| project_ids.contains(&section.project_id))
        {
            self.active_section = None;
            self.active_git_diff = None;
            changed = true;
        }

        if self
            .active_project_page
            .as_ref()
            .is_some_and(|project_id| project_ids.contains(project_id))
        {
            self.active_project_page = None;
            self.active_git_diff = None;
            changed = true;
        }

        if !removed_section_ids.is_empty() {
            self.cleanup_removed_sections(&removed_section_ids, cx);
        }

        if changed {
            cx.notify();
        }
    }

    pub(crate) fn remove_task_sections(&mut self, task_id: &str, cx: &mut Context<Self>) {
        let removed_section_ids = self
            .section_states
            .keys()
            .filter(|section_id| section_id.task_id.as_deref() == Some(task_id))
            .cloned()
            .collect::<HashSet<_>>();
        let before_len = self.section_states.len();
        self.section_states
            .retain(|section_id, _| section_id.task_id.as_deref() != Some(task_id));

        let mut changed = before_len != self.section_states.len();
        if self
            .active_section
            .as_ref()
            .is_some_and(|section| section.task_id.as_deref() == Some(task_id))
        {
            self.active_section = None;
            self.active_project_page = None;
            self.active_git_diff = None;
            changed = true;
        }

        if !removed_section_ids.is_empty() {
            self.cleanup_removed_sections(&removed_section_ids, cx);
        }

        if changed {
            cx.notify();
        }
    }

    pub(crate) fn restore_view(
        &mut self,
        preferred_project_id: &str,
        preferred_project_exists: bool,
        fallback: Option<(SectionId, std::path::PathBuf)>,
        cx: &mut Context<Self>,
    ) {
        if self.active_section.is_some() || self.active_project_page.is_some() {
            return;
        }

        if preferred_project_exists {
            self.active_project_page = Some(preferred_project_id.to_string());
            cx.notify();
            return;
        }

        if let Some((section_id, cwd)) = fallback {
            self.ensure_section(section_id.clone(), Some(cwd), None, cx);
            select_active_section(
                &mut self.active_section,
                &mut self.active_project_page,
                section_id,
            );
            self.persist_active_section(cx);
            cx.notify();
        }
    }

    pub(crate) fn activate_tab(
        &mut self,
        section_id: &SectionId,
        tab_index: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let activated = self
            .section_states
            .get_mut(section_id)
            .is_some_and(|state| state.activate_tab(tab_index));
        if activated {
            let _ = select_active_section(
                &mut self.active_section,
                &mut self.active_project_page,
                section_id.clone(),
            );
            // Hover state belongs to the previously-active tab; a
            // keyboard-driven tab switch (no mouse-leave event)
            // would otherwise leave a stale tooltip waiting to
            // re-paint when the user toggles back.
            self.terminal_link_hover = None;
            self.persist_section_state(section_id, cx);
            self.persist_active_section(cx);
            cx.notify();
        }
        activated
    }

    pub(crate) fn add_tab_with_launch_config(
        &mut self,
        section_id: &SectionId,
        launch_config: TerminalLaunchConfig,
        fixed_title: Option<String>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.add_tab_with_launch_config_attributed(
            section_id,
            launch_config,
            fixed_title,
            ClientId::gui_desktop(),
            cx,
        )
    }

    /// Same as [`add_tab_with_launch_config`] but lets the caller
    /// stamp the originating client on the resulting `TabOpened`
    /// event. The trait-verb path passes `mcp:<handle>` /
    /// `mobile:<endpoint>`; sidebar / shortcut callers default to
    /// `gui:desktop` via the wrapper above.
    pub(crate) fn add_tab_with_launch_config_attributed(
        &mut self,
        section_id: &SectionId,
        launch_config: TerminalLaunchConfig,
        fixed_title: Option<String>,
        originator: ClientId,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let added_tab_id = self
            .section_states
            .get_mut(section_id)
            .map(|state| state.add_tab_with_launch_config(launch_config.clone(), fixed_title));
        if let Some(tab_id) = added_tab_id.as_ref() {
            let app = self.app.clone();
            let section_clone = section_id.clone();
            let tab_clone = tab_id.clone();
            cx.defer(move |cx| {
                if app
                    .update(cx, |app, _| {
                        app.emit_client_event(ClientEvent::TabOpened {
                            originator,
                            section_id: section_clone,
                            tab_id: tab_clone.clone(),
                        });
                    })
                    .is_err()
                {
                    // The app entity dropped between deferring and
                    // running; nothing to subscribe anymore. Log so
                    // we don't silently lose a TabOpened event when
                    // shutdown races a tab spawn.
                    log::warn!("TabOpened defer skipped — app entity gone (tab {tab_clone})");
                }
            });
        }
        if added_tab_id.is_some() {
            self.persist_section_state(section_id, cx);
            cx.notify();
        }
        added_tab_id
    }

    pub(crate) fn close_tab(
        &mut self,
        section_id: &SectionId,
        tab_index: usize,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let removed_tab_id = self
            .section_states
            .get_mut(section_id)
            .and_then(|state| state.close_tab(tab_index));
        if removed_tab_id.is_some() {
            if let Some(ref tab_id) = removed_tab_id {
                self.cleanup_removed_tab(section_id, tab_id.clone(), cx);
            }
            self.persist_section_state(section_id, cx);
            cx.notify();
        }
        removed_tab_id
    }

    pub(crate) fn close_tab_ids(
        &mut self,
        section_id: &SectionId,
        tab_ids: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Vec<String> {
        let removed_tab_ids = self
            .section_states
            .get_mut(section_id)
            .map(|state| state.close_tabs_by_ids(tab_ids))
            .unwrap_or_default();
        if !removed_tab_ids.is_empty() {
            for tab_id in &removed_tab_ids {
                self.cleanup_removed_tab(section_id, tab_id.clone(), cx);
            }
            self.persist_section_state(section_id, cx);
            cx.notify();
        }
        removed_tab_ids
    }

    pub(crate) fn request_close_tab(
        &mut self,
        section_id: &SectionId,
        tab_index: usize,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let tab_id = self
            .section_states
            .get(section_id)
            .and_then(|state| state.tabs.get(tab_index))
            .map(|tab| tab.id.clone())?;

        self.request_close_tab_ids(section_id, vec![tab_id], cx)
            .into_iter()
            .next()
    }

    pub(crate) fn request_close_tabs_for_scope(
        &mut self,
        section_id: &SectionId,
        anchor_tab_id: &str,
        scope: TabCloseScope,
        cx: &mut Context<Self>,
    ) -> Vec<String> {
        let tab_ids = self
            .section_states
            .get(section_id)
            .map(|state| state.tab_ids_for_close_scope(anchor_tab_id, scope))
            .unwrap_or_default();
        self.request_close_tab_ids(section_id, tab_ids, cx)
    }

    pub(crate) fn request_close_tab_ids(
        &mut self,
        section_id: &SectionId,
        tab_ids: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Vec<String> {
        if tab_ids.is_empty() {
            return Vec::new();
        }

        let requested_tab_ids = tab_ids.iter().collect::<HashSet<_>>();
        let pinned_tabs = self
            .section_states
            .get(section_id)
            .map(|state| {
                state
                    .tabs
                    .iter()
                    .filter(|tab| requested_tab_ids.contains(&tab.id) && tab.pinned)
                    .map(|tab| (tab.id.clone(), tab.title.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if let Some((first_pinned_tab_id, first_pinned_title)) = pinned_tabs.first() {
            self.pinned_tab_close_confirm = Some(PinnedTabCloseConfirmState {
                section_id: section_id.clone(),
                tab_id: first_pinned_tab_id.clone(),
                title: first_pinned_title.clone(),
                tab_ids,
                pinned_tab_count: pinned_tabs.len(),
            });
            cx.notify();
            return Vec::new();
        }

        self.close_tab_ids(section_id, tab_ids, cx)
    }

    pub(crate) fn confirm_close_pinned_tab(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let confirm = self.pinned_tab_close_confirm.take()?;
        let tab_ids = if confirm.tab_ids.is_empty() {
            vec![confirm.tab_id]
        } else {
            confirm.tab_ids
        };
        let removed = self.close_tab_ids(&confirm.section_id, tab_ids, cx);
        cx.notify();
        removed.into_iter().next()
    }

    pub(crate) fn begin_tab_rename(
        &mut self,
        section_id: &SectionId,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(title) = self
            .section_states
            .get(section_id)
            .and_then(|state| state.tabs.iter().find(|tab| tab.id == tab_id))
            .map(|tab| tab.title.clone())
        else {
            return false;
        };

        self.terminal_tab_menu = None;
        self.terminal_tab_rename = Some(TerminalTabRenameState {
            section_id: section_id.clone(),
            tab_id: tab_id.to_string(),
            cursor: title.len(),
            draft: title,
        });
        cx.notify();
        true
    }

    pub(crate) fn cancel_tab_rename(&mut self, cx: &mut Context<Self>) {
        if self.terminal_tab_rename.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn commit_tab_rename(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rename) = self.terminal_tab_rename.take() else {
            return false;
        };
        let title = rename.draft.trim().to_string();
        if title.is_empty() {
            cx.notify();
            return false;
        }

        let changed = self
            .section_states
            .get_mut(&rename.section_id)
            .and_then(|state| state.tabs.iter_mut().find(|tab| tab.id == rename.tab_id))
            .is_some_and(|tab| {
                if tab.title == title && tab.fixed_title.as_deref() == Some(title.as_str()) {
                    return false;
                }
                tab.title = title.clone();
                tab.fixed_title = Some(title);
                true
            });
        if changed {
            self.persist_section_state(&rename.section_id, cx);
        }
        cx.notify();
        changed
    }

    pub(crate) fn handle_tab_rename_key_down(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rename) = self.terminal_tab_rename.as_mut() else {
            return false;
        };
        cx.stop_propagation();
        let modifiers = ev.keystroke.modifiers;
        match ev.keystroke.key.as_str() {
            "escape" => self.cancel_tab_rename(cx),
            "enter" => {
                self.commit_tab_rename(cx);
            }
            "backspace" => {
                if rename.cursor > 0 {
                    let start = if modifiers.platform {
                        0
                    } else if modifiers.control || modifiers.alt {
                        word_start_before(&rename.draft, rename.cursor)
                    } else {
                        rename.draft[..rename.cursor]
                            .char_indices()
                            .next_back()
                            .map_or(0, |(index, _)| index)
                    };
                    rename.draft.replace_range(start..rename.cursor, "");
                    rename.cursor = start;
                    cx.notify();
                }
            }
            "delete" => {
                if rename.cursor < rename.draft.len() {
                    let end = rename.draft[rename.cursor..]
                        .char_indices()
                        .nth(1)
                        .map_or(rename.draft.len(), |(index, _)| rename.cursor + index);
                    rename.draft.replace_range(rename.cursor..end, "");
                    cx.notify();
                }
            }
            "left" => {
                if rename.cursor > 0 {
                    rename.cursor = rename.draft[..rename.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(index, _)| index);
                    cx.notify();
                }
            }
            "right" => {
                if rename.cursor < rename.draft.len() {
                    rename.cursor = rename.draft[rename.cursor..]
                        .char_indices()
                        .nth(1)
                        .map_or(rename.draft.len(), |(index, _)| rename.cursor + index);
                    cx.notify();
                }
            }
            "home" => {
                rename.cursor = 0;
                cx.notify();
            }
            "end" => {
                rename.cursor = rename.draft.len();
                cx.notify();
            }
            _ if modifiers.platform && ev.keystroke.key.as_str() == "v" => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let text = text.replace(['\n', '\r', '\t'], " ");
                    rename.draft.insert_str(rename.cursor, &text);
                    rename.cursor += text.len();
                    cx.notify();
                }
            }
            _ if modifiers.control || modifiers.platform || modifiers.function => {}
            _ => {
                if let Some(key_char) = ev.keystroke.key_char.as_deref() {
                    rename.draft.insert_str(rename.cursor, key_char);
                    rename.cursor += key_char.len();
                    cx.notify();
                }
            }
        }
        true
    }

    pub(crate) fn toggle_tab_pinned(
        &mut self,
        section_id: &SectionId,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self
            .section_states
            .get_mut(section_id)
            .is_some_and(|state| {
                let Some(index) = state.tabs.iter().position(|tab| tab.id == tab_id) else {
                    return false;
                };
                let pinned = !state.tabs[index].pinned;
                state.set_tab_pinned(index, pinned)
            });
        if changed {
            self.persist_section_state(section_id, cx);
            cx.notify();
        }
        changed
    }

    pub(crate) fn show_error_toast(
        &self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let message = message.into();
        let _ = self.app.update(cx, |app, app_cx| {
            app.show_error_toast(message.clone(), app_cx)
        });
    }

    fn persist_section_state(&self, section_id: &SectionId, cx: &mut Context<Self>) {
        let Some(persisted) = self
            .section_states
            .get(section_id)
            .map(SectionState::to_persisted)
        else {
            return;
        };
        let app = self.app.clone();
        let section_id = section_id.clone();
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, _| {
                app.persist_section_state(&section_id, persisted.clone());
            });
        });
    }

    fn cleanup_removed_tab(&self, section_id: &SectionId, tab_id: String, cx: &mut Context<Self>) {
        let app = self.app.clone();
        let section_id = section_id.clone();
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, _| {
                app.cleanup_removed_tab(&section_id, tab_id);
            });
        });
    }

    fn cleanup_removed_sections(&self, section_ids: &HashSet<SectionId>, cx: &mut Context<Self>) {
        let app = self.app.clone();
        let section_ids = section_ids.clone();
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, _| {
                app.cleanup_removed_sections(&section_ids);
            });
        });
    }

    fn persist_active_section(&self, cx: &mut Context<Self>) {
        let app = self.app.clone();
        let section_key = persisted_active_section_key(self.active_section.as_ref());
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, _| {
                app.set_last_active_section_key(section_key.clone());
            });
        });
    }

    pub(crate) fn request_remove_project_group(&self, project_id: &str, cx: &mut Context<Self>) {
        let project_id = project_id.to_string();
        let app = self.app.clone();
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, app_cx| {
                app.request_remove_project_group(&project_id, app_cx);
            });
        });
    }

    pub(crate) fn open_new_task_modal(&self, project_id: &str, cx: &mut Context<Self>) {
        let project_id = project_id.to_string();
        let _ = self.app.update(cx, |app, app_cx| {
            app.open_new_task_modal(&project_id, app_cx);
            app_cx.notify();
        });
    }

    pub(crate) fn open_add_agent_modal(&self, section_id: &SectionId, cx: &mut Context<Self>) {
        let section_id = section_id.clone();
        let app = self.app.clone();
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, app_cx| {
                let selected_agent_id = {
                    let workspace = app.workspace_pane.read(app_cx);
                    new_tab_seed_agent_id(
                        workspace.section_states.get(&section_id),
                        app.default_agent_id(),
                    )
                };
                app.open_add_agent_modal(section_id.clone(), selected_agent_id.clone(), app_cx);
                app_cx.notify();
            });
        });
    }
}

pub struct AnotherOneApp {
    pub(crate) sidebar_w: f32,
    pub(crate) sidebar_saved: f32,
    pub(crate) right_w: f32,
    pub(crate) right_saved: f32,
    pub(crate) drag: Option<(Gutter, f32)>,
    pub(crate) animating: bool,
    /// Currently visible pane in narrow (phone) layout. Ignored when the
    /// viewport is wide — the desktop three-column layout controls
    /// visibility itself. Defaults to `Home`.
    pub(crate) mobile_view: MobileView,
    /// Master/detail navigation history for narrow mode. Each push records
    /// the previous `mobile_view`; `mobile_back` pops it. Empty stack = the
    /// header should show a hamburger instead of a back chevron.
    pub(crate) mobile_nav_stack: Vec<MobileView>,
    pub(crate) project_store: ProjectStore,
    pub(crate) project_github_links: HashMap<String, String>,
    pub(crate) expanded_projects: HashSet<String>,
    pub(crate) project_expand_animations: HashMap<String, SidebarProjectExpandAnimation>,
    pub(crate) next_project_expand_animation_id: u64,
    pub(crate) project_menu_project: Option<String>,
    pub(crate) sidebar_task_menu: Option<SidebarTaskMenuState>,
    /// Collapsed change-file sections in the right sidebar (e.g. "staged", "uncommitted").
    pub(crate) collapsed_change_sections: HashSet<String>,
    /// Expanded recent-commit rows keyed by `project_id:commit_id`.
    pub(crate) expanded_commit_rows: HashSet<String>,
    /// Whether the right-sidebar git actions dropdown menu is open.
    pub(crate) git_actions_menu_open: bool,
    /// Whether the titlebar custom actions dropdown menu is open.
    pub(crate) custom_actions_menu_open: bool,
    /// Last manually-run custom action id used to label and target the titlebar action button.
    pub(crate) last_used_custom_action_id: Option<String>,
    /// Active transient notifications displayed above the app chrome.
    toasts: Vec<AppToast>,
    next_toast_id: u64,
    toast_drag: Option<ToastDrag>,
    copied_toast: Option<(u64, Instant)>,
    pasted_image_preview: Option<PastedImagePreview>,
    /// Pending discard confirmation: (project_id, files_to_discard).
    pub(crate) discard_confirm: Option<(String, Vec<ChangedFile>)>,
    /// Pending project removal confirmation for a project group with open tasks.
    pub(crate) project_remove_confirm: Option<ProjectRemoveConfirmState>,
    /// Pending task deletion confirmation for a worktree task.
    pub(crate) sidebar_task_delete_confirm: Option<SidebarTaskDeleteConfirmState>,
    pub(crate) workspace_pane: Entity<WorkspacePane>,
    /// Cached changed-file snapshot per project.
    pub(crate) changed_files: HashMap<String, Arc<[ChangedFile]>>,
    /// Cached partitioned sidebar data derived from `changed_files`.
    pub(crate) changed_files_list_snapshots:
        HashMap<String, crate::right_sidebar::ChangedFilesListSnapshot>,
    /// Focus handle for terminal keyboard input.
    pub(crate) focus_handle: FocusHandle,
    /// Whether the titlebar background should begin a window drag on the next mouse move.
    pub(crate) titlebar_drag_pending: bool,
    /// Whether the refresh timer has been started.
    pub(crate) refresh_timer_started: bool,
    /// Toolbar git actions currently running in the background, keyed by originating project id.
    pub(crate) active_git_actions: HashMap<String, ActiveToolbarGitAction>,
    /// Pending right-sidebar git mutations keyed by project id.
    pending_changed_files_git_mutations: HashMap<String, PendingChangedFilesGitMutations>,
    /// Sender for background right-sidebar git mutation replies.
    changed_files_git_mutation_sender: broadcast::Sender<ChangedFilesGitMutationReply>,
    /// Receiver for background right-sidebar git mutation replies.
    changed_files_git_mutation_receiver: broadcast::Receiver<ChangedFilesGitMutationReply>,
    /// Active changed-file diff load result for the main pane.
    pub(crate) git_diff_state: Option<GitDiffPaneState>,
    /// Receiver for the in-flight changed-file diff load.
    git_diff_receiver: Option<broadcast::Receiver<ChangedFileDiffReply>>,
    /// Lifecycle slot for the in-flight automatic git refresh result.
    git_refresh_operation: BroadcastOperation<GitRefreshReply>,
    /// Lifecycle slot for refreshing remote refs while the new-task modal is open.
    new_task_branch_refresh_operation: BroadcastOperation<RemoteBranchRefreshReply>,
    /// Receiver for the in-flight new task worktree creation result.
    task_creation_receiver: Option<broadcast::Receiver<TaskCreationReply>>,
    /// Receiver for the in-flight create-branch operation.
    branch_creation_receiver:
        Option<broadcast::Receiver<another_one_core::project_service::BranchCreationReply>>,
    /// UI context for the in-flight task creation worker.
    pending_task_launch: Option<PendingTaskLaunch>,
    /// Receiver for the in-flight add-project background preparation result.
    project_add_receiver: Option<broadcast::Receiver<ProjectAddReply>>,
    /// Sender used by background worktree deletion operations.
    pub(crate) worktree_deletion_sender: mpsc::Sender<WorktreeDeletionReply>,
    /// Receiver for background worktree deletion operations.
    worktree_deletion_receiver: mpsc::Receiver<WorktreeDeletionReply>,
    /// Sender used by background commit file-change lookups.
    commit_file_changes_sender: mpsc::Sender<CommitFileChangesReply>,
    /// Receiver for background commit file-change lookups.
    commit_file_changes_receiver: mpsc::Receiver<CommitFileChangesReply>,
    /// Sender used by background project GitHub-link lookups.
    project_github_link_sender: broadcast::Sender<ProjectGitHubLinkReply>,
    /// Receiver for background project GitHub-link lookups.
    project_github_link_receiver: broadcast::Receiver<ProjectGitHubLinkReply>,
    /// Cached pull request metadata keyed by `project_id:branch_name`.
    pub(crate) project_pull_requests: HashMap<String, crate::git_actions::PullRequestStatus>,
    /// Sender used by background pull-request lookups.
    project_pull_request_sender: broadcast::Sender<ProjectPullRequestReply>,
    /// Receiver for background pull-request lookups.
    project_pull_request_receiver: broadcast::Receiver<ProjectPullRequestReply>,
    project_page_pull_requests_sender: broadcast::Sender<ProjectPagePullRequestsReply>,
    project_page_pull_requests_receiver: broadcast::Receiver<ProjectPagePullRequestsReply>,
    /// Sender used by background PR check lookups.
    project_check_runs_sender: broadcast::Sender<ProjectCheckRunsReply>,
    /// Receiver for background PR check lookups.
    project_check_runs_receiver: broadcast::Receiver<ProjectCheckRunsReply>,
    /// Sender used by background terminal launch/resume work.
    ///
    /// Bounded (`sync_channel`) so PTY reader threads experience natural
    /// backpressure when the GPUI drain falls behind: a full queue
    /// blocks the reader on `.send()`, which in turn lets the kernel
    /// PTY buffer fill and apply `write()` backpressure to the child
    /// process — exactly how a real terminal emulator behaves. An
    /// unbounded channel here produced the 37 GiB RSS leak that caused
    /// this type to exist in its current shape. See
    /// [`TERMINAL_LAUNCH_QUEUE_CAP`].
    terminal_launch_sender: mpsc::SyncSender<TerminalLaunchReply>,
    /// Receiver for background terminal launch/resume work.
    terminal_launch_receiver: mpsc::Receiver<TerminalLaunchReply>,
    /// Sender used by hidden add-agent terminal prewarming work.
    ///
    /// Bounded for the same reason as `terminal_launch_sender`. See
    /// [`WARM_TERMINAL_LAUNCH_QUEUE_CAP`].
    warm_terminal_launch_sender: mpsc::SyncSender<WarmTerminalLaunchReply>,
    /// Receiver for hidden add-agent terminal prewarming work.
    warm_terminal_launch_receiver: mpsc::Receiver<WarmTerminalLaunchReply>,
    /// Live PTY-backed terminal runtimes keyed by section and tab id.
    live_terminal_runtimes: HashMap<TerminalRuntimeKey, LiveTerminalRuntime>,
    /// Last terminal mutation that needs low-latency drain/render ticks.
    last_terminal_activity: Instant,
    /// Input to send once a newly launched action terminal is ready.
    pending_post_launch_input: HashMap<TerminalRuntimeKey, Vec<u8>>,
    /// GPUI-free bookkeeping for per-tab terminal state: recent output,
    /// last error, tracked process, pending-launch flag. Backed by
    /// `another_one_core::terminal_manager::TerminalManager` so it can
    /// be consumed by the headless daemon too.
    pub(crate) terminal_manager: another_one_core::terminal_manager::TerminalManager,
    /// Cached render snapshots for live terminal tabs.
    terminal_surface_snapshots: HashMap<TerminalRuntimeKey, TerminalSurfaceSnapshot>,
    /// Fractional wheel delta carried across scroll events for terminal scrollback.
    terminal_scroll_remainder_lines: HashMap<TerminalRuntimeKey, f32>,
    /// Mouse selection state for the currently selected terminal text.
    terminal_selection: Option<TerminalSelectionState>,
    /// Active scrollback search (Cmd-F). At most one terminal at a time;
    /// opening the search on a different terminal closes any previous.
    pub(crate) terminal_search: Option<TerminalSearchState>,
    /// Last time each terminal rang its bell. The renderer flashes the
    /// pane briefly while the entry is fresher than `BELL_FLASH_DURATION`.
    pub(crate) terminal_bell_at: HashMap<TerminalRuntimeKey, Instant>,
    /// Per-client focus tracker. Every connected client (the GUI, any
    /// MCP harness, mobile peers) has at most one entry here. The
    /// `select` / `select_for` verbs on the `DaemonClient` trait flow
    /// through this map.
    pub(crate) client_focus: HashMap<ClientId, Focus>,
    /// Last `Focus` known for the GUI client. Compared on every drain
    /// tick to detect mouse-driven navigation (sidebar clicks, tab
    /// switches, project-page activations) so a corresponding
    /// `FocusChanged` event flows onto the bus — closing the loop
    /// for MCP harnesses that want to observe the human.
    pub(crate) last_observed_gui_focus: Focus,
    /// In-flight worktree-task creations keyed by `JobId`. The async
    /// `project_service::spawn_task_creation` runs in the
    /// background; the GUI submit path stores a `(originator,
    /// project_id)` against a fresh JobId here so the drain can
    /// fire correlated `TaskOpened` / `TaskOpenFailed` events.
    /// HashMap (rather than the original single Option) because a
    /// future MCP `create_worktree_task` verb concurrent with a
    /// GUI submit must not clobber the GUI's job.
    ///
    /// Today the underlying `task_creation_receiver` is single-slot
    /// — concurrent creations queue at the receiver — so the map
    /// holds at most one entry in practice. Promoting to multi-job
    /// only requires extending `task_creation_receiver` to a
    /// HashMap too; the event correlation is already job-keyed.
    pub(crate) pending_worktree_jobs: HashMap<JobId, (ClientId, String)>,
    /// GUI's own `broadcast::Receiver` clone — the desktop app is
    /// itself a client and uses it to surface a toast when *another*
    /// client (MCP, mobile) drives a state change. Drained on every
    /// render tick alongside other observability work.
    pub(crate) gui_event_receiver: Option<tokio::sync::broadcast::Receiver<ClientEvent>>,
    /// Daemon-side `ClientEvent` broadcast bus. Owned directly by
    /// the app (rather than tucked inside `Mutex<RegistryState>`)
    /// so emits — which fire on every state change including the
    /// per-PTY-chunk `Output` event — never have to take the
    /// registry mutex. Cloned into the MCP orchestrator at startup
    /// so per-session subscribers can `.subscribe()` without round-
    /// tripping through the lock either.
    pub(crate) event_bus: tokio::sync::broadcast::Sender<ClientEvent>,
    /// Prewarmed launches keyed by launch id until they are canceled or exit.
    prewarmed_terminal_launches: HashMap<u64, PrewarmedTerminalLaunch>,
    /// Child process ids for hidden prewarmed launches.
    prewarmed_terminal_processes: HashMap<u64, TrackedProcess>,
    /// Prewarmed launch ids that were canceled before the process fully exited.
    canceled_prewarmed_launch_ids: HashSet<u64>,
    /// Current warm launch reserved for the open Add Agent modal.
    pub(crate) active_add_agent_warm_launch_id: Option<u64>,
    /// Current warm launch reserved for the open New Task modal.
    pub(crate) active_new_task_warm_launch_id: Option<u64>,
    /// Monotonic id generator for warm launches.
    next_prewarmed_launch_id: u64,
    /// In-flight project GitHub-link lookups keyed by project id.
    pub(crate) project_github_link_requests: HashSet<String>,
    /// Projects whose GitHub link has already been resolved this session.
    pub(crate) project_github_link_checked: HashSet<String>,
    /// In-flight pull-request lookups keyed by `project_id:branch_name`.
    pub(crate) project_pull_request_requests: HashSet<String>,
    pub(crate) project_page_pull_requests:
        HashMap<String, Arc<[crate::git_actions::ProjectPagePullRequest]>>,
    pub(crate) project_page_pull_requests_loading: HashSet<String>,
    pub(crate) project_page_pull_requests_errors: HashMap<String, String>,
    /// Branches whose pull-request lookup has been resolved at least once.
    pub(crate) project_pull_request_checked: HashSet<String>,
    /// Last successful lookup completion time keyed by `project_id:branch_name`.
    pub(crate) project_pull_request_checked_at: HashMap<String, Instant>,
    /// Cached PR check-run state keyed by `project_id:branch_name`.
    pub(crate) project_check_runs_states: HashMap<String, ProjectCheckRunsState>,
    /// In-flight PR check lookups keyed by `project_id:branch_name`.
    pub(crate) project_check_runs_requests: HashSet<String>,
    /// Last successful check-run lookup completion time keyed by `project_id:branch_name`.
    pub(crate) project_check_runs_checked_at: HashMap<String, Instant>,
    /// New Task modal state. Some when open, None when closed.
    pub(crate) new_task_modal: Option<crate::new_task_modal::NewTaskModalState>,
    /// Add Agent modal state. Some when open, None when closed.
    pub(crate) add_agent_modal: Option<crate::add_agent_modal::AddAgentModalState>,
    /// Add/Edit custom action modal state. Some when open, None when closed.
    pub(crate) custom_action_modal: Option<crate::custom_actions_modal::CustomActionModalState>,
    /// Create Branch modal state. Some when open, None when closed.
    pub(crate) create_branch_modal: Option<crate::create_branch_modal::CreateBranchModalState>,
    /// Inline rename state for a direct task in the left sidebar.
    pub(crate) sidebar_task_rename: Option<SidebarTaskRenameState>,
    /// Most recent direct-task click used to detect double-click rename reliably.
    pub(crate) sidebar_task_last_click: Option<(String, String, Instant)>,
    /// IME marked text state for terminal input.
    pub(crate) marked_text: Option<String>,
    /// Whether the settings page is open.
    pub(crate) settings_open: bool,
    /// Which settings section is currently active.
    pub(crate) settings_section: crate::settings_page::SettingsSection,
    /// Canonical MCP server registry — source of truth for which
    /// MCP servers AnotherOne manages and which harnesses each is
    /// enabled for. Synced into per-harness config files on toggle.
    pub(crate) mcp_registry: another_one_core::mcp::registry::McpRegistry,
    /// Providers whose most recent MCP sync failed. The MCP page
    /// renders a column-level error indicator on these; we don't
    /// track which row caused the failure because `sync_all`
    /// returns a single Result per provider rather than per id.
    /// Cleared before each sync.
    pub(crate) mcp_last_sync_errors:
        std::collections::HashSet<another_one_core::agents::AgentProviderKind>,
    /// Apps detected on this machine that support opening a project directory.
    pub(crate) available_open_in_apps: Vec<OpenInAppKind>,
    /// Project id whose header "Open In" menu is currently expanded.
    pub(crate) project_page_open_in_menu_project_id: Option<String>,
    /// Whether the bottom project configuration panel is expanded.
    pub(crate) project_page_config_panel_expanded: bool,
    /// Whether the project configuration panel should render as targeted.
    pub(crate) project_page_config_panel_targeted: bool,
    /// Which project configuration dropdown is currently open.
    pub(crate) project_page_config_dropdown: Option<ProjectBranchSettingField>,
    /// Shortcut row currently waiting for key capture in settings.
    pub(crate) shortcut_capture_action: Option<crate::shortcuts::ShortcutAction>,
    /// Local draft and selection state for per-agent launch arg editing in settings.
    pub(crate) settings_agent_input: SettingsAgentInputState,
    /// Local draft and selection state for the git commit generation script editor.
    pub(crate) settings_git_commit_script_input: SettingsGitActionScriptInputState,
    /// Local draft and selection state for the git PR generation script editor.
    pub(crate) settings_git_pr_script_input: SettingsGitActionScriptInputState,
    /// Last measured line layouts for the git commit generation script editor.
    pub(crate) settings_git_commit_script_layout: Vec<SettingsGitActionScriptLineLayout>,
    /// Last measured line layouts for the git PR generation script editor.
    pub(crate) settings_git_pr_script_layout: Vec<SettingsGitActionScriptLineLayout>,
    /// Selection anchor while dragging in the git commit generation script editor.
    pub(crate) settings_git_commit_script_drag_anchor: Option<usize>,
    /// Selection anchor while dragging in the git PR generation script editor.
    pub(crate) settings_git_pr_script_drag_anchor: Option<usize>,
    /// Open model-configuration dropdown on the Git Actions settings page.
    pub(crate) settings_git_action_llm_dropdown: Option<crate::app::SettingsGitActionLlmDropdown>,
    /// Active right-sidebar mode for task views.
    pub(crate) right_sidebar_mode: RightSidebarMode,
    /// Session-scoped recent-commit page sizes keyed by project id.
    pub(crate) commit_page_sizes: HashMap<String, usize>,
    /// Cached recent-commit snapshots keyed by project id.
    pub(crate) branch_commit_states: HashMap<String, ProjectBranchCommitState>,
    /// Cached per-commit file-change snapshots keyed by `project_id:commit_id`.
    pub(crate) commit_file_changes_states: HashMap<String, CommitFileChangesState>,
    /// UI font size (adjusted by Cmd+/Cmd- zoom).
    pub(crate) font_size: f32,
    /// Last observed viewport size used to detect real resize events.
    pub(crate) last_viewport_size: Size<Pixels>,
    /// User-facing Git workspace refresh lifecycle for active project Git UI.
    pub(crate) git_workspace: GitWorkspace,
    /// Whether the resource usage panel is visible.
    pub(crate) resource_indicator_open: bool,
    /// Whether the "Pair mobile" modal is open. See
    /// `titlebar::titlebar_pair_mobile_button` / `pair_mobile_overlay`.
    /// The modal now reads pairing material from
    /// `Self::daemon_handle`; nothing on disk is consulted.
    pub(crate) pair_mobile_modal_open: bool,
    /// Two-click confirmation for the "Reset pairings" action — the
    /// first click arms this flag and repaints the button in a
    /// danger state; the second click actually wipes the allowlist.
    /// Cleared on modal close so an accidental re-open doesn't
    /// carry the armed state over.
    pub(crate) pair_mobile_reset_pending: bool,
    /// Shared registry state read by the embedded daemon's tokio
    /// tasks. Owns the live PTY broadcast senders + writer handles so
    /// a mobile client's `AttachTab` can subscribe to the same
    /// stream the GPUI renderer consumes. See
    /// `crate::daemon_host::RegistryState` for the full layout.
    pub(crate) registry_state: Arc<Mutex<crate::daemon_host::RegistryState>>,
    /// Receives the `EndpointHandle` from the daemon-host thread once
    /// the iroh endpoint is up. Polled on every render tick until the
    /// handle arrives, then `daemon_handle` is set and the receiver
    /// is cleared.
    pub(crate) daemon_handle_rx: Option<mpsc::Receiver<anyhow::Result<daemon::EndpointHandle>>>,
    /// Endpoint handle from the embedded daemon (pairing URL + QR
    /// PNG). `None` until the daemon-host thread finishes booting;
    /// after that, `pair_mobile_overlay` reads from here. Keeping the
    /// handle alive keeps the endpoint alive — drop aborts it.
    pub(crate) daemon_handle: Option<daemon::EndpointHandle>,
    /// Session used to reach the daemon. On desktop this is the
    /// client half of an `in_memory::pair()` whose server half the
    /// embedded daemon's runtime drives via
    /// `daemon::dispatch::serve_session`. On mobile this starts as a
    /// `NoSession` placeholder and swaps to an `IrohSession` once the
    /// QR pair flow lands a successful dial. Every state-mutating GUI
    /// call that has a `Control` equivalent routes through here, so
    /// the desktop/mobile/render-vs-network distinction is opaque to
    /// callers — both platforms run the same code path.
    ///
    /// Stored behind a `parking_lot`-style `Mutex` (using `std::sync`
    /// here for parity with the rest of the app) so the QR pair flow
    /// can swap the impl in place without `&mut self` plumbing through
    /// every call site that reads it.
    pub(crate) session: Arc<Mutex<Arc<dyn daemon_transport::Session>>>,
    /// Stable producer side of the session-events fan-in. Cloned by
    /// `replace_session` to spawn a fresh events pump on the new
    /// session (post-pair) without touching the receiver. Held for
    /// the lifetime of the app — its existence keeps `session_events_rx`
    /// from disconnecting when an old pump task ends.
    pub(crate) session_events_tx:
        tokio::sync::mpsc::UnboundedSender<daemon_transport::SessionEvent>,
    /// Single-consumer side of the session-events fan-in. Drained on
    /// the GPUI render tick by `drain_session_events`. PTY bytes
    /// arrive here as `SessionEvent::PtyBytes` from whatever session
    /// is current — desktop's in-memory pair never pushes (no
    /// `AttachTab` issued); mobile's wrapped iroh session pushes via
    /// the legacy session's `next_incoming_bytes` bridge.
    pub(crate) session_events_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<daemon_transport::SessionEvent>>,
    /// Collapsed resource tree node ids in the resource usage panel.
    pub(crate) resource_collapsed_nodes: HashSet<String>,
    /// Latest sampled resource usage snapshot.
    pub(crate) resource_usage: ResourceUsageSnapshot,
    /// Native process sampler state used to calculate CPU deltas across refreshes.
    pub(crate) resource_usage_sampler: ResourceUsageSampler,
    /// Last time resource usage was sampled.
    pub(crate) last_resource_usage_refresh: Instant,
    /// In-app updater worker handle. Owns a dedicated OS thread
    /// that polls the public release manifest on a 10 minute
    /// cadence and downloads/verifies new payloads. The
    /// render-tick drain pulls events off it via `try_recv`.
    pub(crate) updater: crate::updater::UpdaterHandle,
    /// Latest updater state surfaced in Settings → General.
    pub(crate) updater_state: crate::updater::UpdateState,
}

impl Focusable for AnotherOneApp {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

struct AppInputHost {
    child: AnyElement,
    focus_handle: FocusHandle,
    view: Entity<AnotherOneApp>,
}

impl AppInputHost {
    fn new(
        child: impl IntoElement,
        focus_handle: FocusHandle,
        view: Entity<AnotherOneApp>,
    ) -> Self {
        Self {
            child: child.into_any_element(),
            focus_handle,
            view,
        }
    }
}

impl IntoElement for AppInputHost {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for AppInputHost {
    type RequestLayoutState = ();
    type PrepaintState = Bounds<Pixels>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.child.prepaint(window, cx);
        bounds
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        input_bounds: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
        window.handle_input(
            &self.focus_handle,
            ElementInputHandler::new(*input_bounds, self.view.clone()),
            cx,
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextInputTarget {
    NewTaskModal,
    NewTaskModalBranchFilter,
    CreateBranchModal,
    SidebarTaskRename,
    CustomActionModal,
    SettingsAgentInput,
    SettingsGitActionScript,
    Terminal,
    Blocked,
}

fn open_in_target_path_for_project(
    project_id: &str,
    project_path: &std::path::Path,
    active_git_diff: Option<&crate::project_store::GitDiffSelection>,
) -> std::path::PathBuf {
    active_git_diff
        .filter(|selection| selection.project_id == project_id && !selection.path.is_empty())
        .map(|selection| project_path.join(&selection.path))
        .unwrap_or_else(|| project_path.to_path_buf())
}

impl AnotherOneApp {
    /// Snapshot the current `Session` impl. Cheap (`Arc::clone` under
    /// a brief mutex lock) so call sites can call this on every
    /// dispatch without worrying about contention; the lock is only
    /// held for the duration of the clone.
    #[allow(dead_code)] // wired up by the per-call-site migrations (see another-one-oja sub-issues)
    pub(crate) fn session_handle(&self) -> Arc<dyn daemon_transport::Session> {
        self.session
            .lock()
            .expect("session slot poisoned")
            .clone()
    }

    /// Replace the current `Session` impl. Used by the QR pair flow
    /// on mobile to swap the `NoSession` placeholder for a live
    /// `IrohSession` once `daemon_client::iroh_factory().dial(...)`
    /// returns.
    #[allow(dead_code)] // wired up by the mobile pair-completion commit
    pub(crate) fn replace_session(&self, session: Arc<dyn daemon_transport::Session>) {
        let events = session.events();
        *self.session.lock().expect("session slot poisoned") = session;
        let tx = self.session_events_tx.clone();
        crate::session_host::spawn_event_pump(events, move |event| {
            let _ = tx.send(event);
        });
    }

    pub(crate) fn focused_settings_git_action_script_kind(
        &self,
    ) -> Option<SettingsGitActionScriptKind> {
        if self.settings_git_commit_script_input.focused {
            Some(SettingsGitActionScriptKind::Commit)
        } else if self.settings_git_pr_script_input.focused {
            Some(SettingsGitActionScriptKind::PullRequest)
        } else {
            None
        }
    }

    pub(crate) fn settings_git_action_script_input(
        &self,
        kind: SettingsGitActionScriptKind,
    ) -> &SettingsGitActionScriptInputState {
        match kind {
            SettingsGitActionScriptKind::Commit => &self.settings_git_commit_script_input,
            SettingsGitActionScriptKind::PullRequest => &self.settings_git_pr_script_input,
        }
    }

    pub(crate) fn settings_git_action_script_input_mut(
        &mut self,
        kind: SettingsGitActionScriptKind,
    ) -> &mut SettingsGitActionScriptInputState {
        match kind {
            SettingsGitActionScriptKind::Commit => &mut self.settings_git_commit_script_input,
            SettingsGitActionScriptKind::PullRequest => &mut self.settings_git_pr_script_input,
        }
    }

    pub(crate) fn settings_git_action_script_layout(
        &self,
        kind: SettingsGitActionScriptKind,
    ) -> &[SettingsGitActionScriptLineLayout] {
        match kind {
            SettingsGitActionScriptKind::Commit => &self.settings_git_commit_script_layout,
            SettingsGitActionScriptKind::PullRequest => &self.settings_git_pr_script_layout,
        }
    }
}

impl EntityInputHandler for AnotherOneApp {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        match self.text_input_target(_cx) {
            TextInputTarget::NewTaskModal => self
                .new_task_modal
                .as_ref()
                .map(|state| text_for_utf16_range(&state.task_name, range, adjusted_range)),
            TextInputTarget::NewTaskModalBranchFilter => self
                .new_task_modal
                .as_ref()
                .map(|state| text_for_utf16_range(&state.branch_filter, range, adjusted_range)),
            TextInputTarget::CreateBranchModal => self
                .create_branch_modal
                .as_ref()
                .map(|state| text_for_utf16_range(&state.branch_name, range, adjusted_range)),
            TextInputTarget::SidebarTaskRename => self
                .sidebar_task_rename
                .as_ref()
                .map(|state| text_for_utf16_range(&state.task_name, range, adjusted_range)),
            TextInputTarget::CustomActionModal => {
                self.custom_action_modal.as_ref().and_then(|state| {
                    state
                        .focused_text_value()
                        .map(|text| text_for_utf16_range(text, range, adjusted_range))
                })
            }
            TextInputTarget::SettingsAgentInput => self
                .settings_agent_input
                .focused_agent_id
                .as_ref()
                .and_then(|agent_id| self.settings_agent_input.drafts.get(agent_id))
                .map(|draft| text_for_utf16_range(draft, range, adjusted_range)),
            TextInputTarget::SettingsGitActionScript => {
                self.focused_settings_git_action_script_kind().map(|kind| {
                    text_for_utf16_range(
                        &self.settings_git_action_script_input(kind).draft,
                        range,
                        adjusted_range,
                    )
                })
            }
            TextInputTarget::Terminal => None,
            TextInputTarget::Blocked => None,
        }
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        match self.text_input_target(_cx) {
            TextInputTarget::NewTaskModal => self.new_task_modal.as_ref().map(|state| {
                utf16_selection_for_text(
                    &state.task_name,
                    state.task_name_cursor,
                    state.task_name_selection_anchor,
                )
            }),
            TextInputTarget::NewTaskModalBranchFilter => {
                self.new_task_modal.as_ref().map(|state| {
                    utf16_selection_for_text(
                        &state.branch_filter,
                        state.branch_filter_cursor,
                        state.branch_filter_selection_anchor,
                    )
                })
            }
            TextInputTarget::CreateBranchModal => self.create_branch_modal.as_ref().map(|state| {
                utf16_selection_for_text(
                    &state.branch_name,
                    state.branch_name_cursor,
                    state.branch_name_selection_anchor,
                )
            }),
            TextInputTarget::SidebarTaskRename => self.sidebar_task_rename.as_ref().map(|state| {
                utf16_selection_for_text(
                    &state.task_name,
                    state.task_name_cursor,
                    state.task_name_selection_anchor,
                )
            }),
            TextInputTarget::CustomActionModal => {
                self.custom_action_modal.as_ref().and_then(|state| {
                    state.focused_text_value().map(|text| {
                        utf16_selection_for_text(
                            text,
                            state.text_cursor,
                            state.text_selection_anchor,
                        )
                    })
                })
            }
            TextInputTarget::SettingsAgentInput => self
                .settings_agent_input
                .focused_agent_id
                .as_ref()
                .and_then(|agent_id| self.settings_agent_input.drafts.get(agent_id))
                .map(|draft| {
                    utf16_selection_for_text(
                        draft,
                        self.settings_agent_input.cursor,
                        self.settings_agent_input.selection_anchor,
                    )
                }),
            TextInputTarget::SettingsGitActionScript => {
                self.focused_settings_git_action_script_kind().map(|kind| {
                    let input = self.settings_git_action_script_input(kind);
                    utf16_selection_for_text(&input.draft, input.cursor, input.selection_anchor)
                })
            }
            TextInputTarget::Terminal => None,
            TextInputTarget::Blocked => None,
        }
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_text = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = None;
        match self.text_input_target(cx) {
            TextInputTarget::NewTaskModal => {
                if let Some(state) = self.new_task_modal.as_mut() {
                    replace_custom_text(
                        &mut state.task_name,
                        &mut state.task_name_cursor,
                        &mut state.task_name_selection_anchor,
                        range,
                        text,
                        false,
                    );
                    cx.notify();
                }
            }
            TextInputTarget::NewTaskModalBranchFilter => {
                if let Some(state) = self.new_task_modal.as_mut() {
                    replace_custom_text(
                        &mut state.branch_filter,
                        &mut state.branch_filter_cursor,
                        &mut state.branch_filter_selection_anchor,
                        range,
                        text,
                        false,
                    );
                    cx.notify();
                }
            }
            TextInputTarget::CreateBranchModal => {
                if let Some(state) = self.create_branch_modal.as_mut() {
                    replace_custom_text(
                        &mut state.branch_name,
                        &mut state.branch_name_cursor,
                        &mut state.branch_name_selection_anchor,
                        range,
                        text,
                        false,
                    );
                    cx.notify();
                }
            }
            TextInputTarget::SidebarTaskRename => {
                if let Some(state) = self.sidebar_task_rename.as_mut() {
                    replace_custom_text(
                        &mut state.task_name,
                        &mut state.task_name_cursor,
                        &mut state.task_name_selection_anchor,
                        range,
                        text,
                        false,
                    );
                    cx.notify();
                }
            }
            TextInputTarget::CustomActionModal => {
                if let Some(state) = self.custom_action_modal.as_mut() {
                    let preserve_newlines = state.focused_field_preserves_newlines();
                    if let Some((current_text, cursor, selection_anchor)) =
                        state.focused_text_parts()
                    {
                        replace_custom_text(
                            current_text,
                            cursor,
                            selection_anchor,
                            range,
                            text,
                            preserve_newlines,
                        );
                        cx.notify();
                    }
                }
            }
            TextInputTarget::SettingsAgentInput => {
                if let Some(agent_id) = self.settings_agent_input.focused_agent_id.clone() {
                    let draft = self
                        .settings_agent_input
                        .drafts
                        .entry(agent_id)
                        .or_default();
                    replace_custom_text(
                        draft,
                        &mut self.settings_agent_input.cursor,
                        &mut self.settings_agent_input.selection_anchor,
                        range,
                        text,
                        false,
                    );
                    cx.notify();
                }
            }
            TextInputTarget::SettingsGitActionScript => {
                if let Some(kind) = self.focused_settings_git_action_script_kind() {
                    let saved_draft = {
                        let input = self.settings_git_action_script_input_mut(kind);
                        replace_custom_text(
                            &mut input.draft,
                            &mut input.cursor,
                            &mut input.selection_anchor,
                            range,
                            text,
                            true,
                        );
                        input.draft.clone()
                    };
                    match kind {
                        SettingsGitActionScriptKind::Commit => {
                            self.dispatch_set_git_commit_script(saved_draft);
                        }
                        SettingsGitActionScriptKind::PullRequest => {
                            self.dispatch_set_git_pr_script(saved_draft);
                        }
                    }
                }
                cx.notify();
            }
            TextInputTarget::Terminal => {
                if !text.is_empty() {
                    let _ = self.write_active_terminal_input(cx, text.as_bytes());
                }
            }
            TextInputTarget::Blocked => {}
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.text_input_target(cx) {
            TextInputTarget::NewTaskModal
            | TextInputTarget::NewTaskModalBranchFilter
            | TextInputTarget::CreateBranchModal
            | TextInputTarget::SidebarTaskRename
            | TextInputTarget::CustomActionModal
            | TextInputTarget::SettingsAgentInput
            | TextInputTarget::SettingsGitActionScript => {
                self.replace_text_in_range(range, new_text, _window, cx);
                self.marked_text = if new_text.is_empty() {
                    None
                } else {
                    Some(new_text.to_string())
                };
                return;
            }
            TextInputTarget::Terminal => {
                self.marked_text = if new_text.is_empty() {
                    None
                } else {
                    Some(new_text.to_string())
                };
                cx.notify();
                return;
            }
            TextInputTarget::Blocked => {}
        }

        self.marked_text = if new_text.is_empty() {
            None
        } else {
            Some(new_text.to_string())
        };
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: std::ops::Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.text_input_target(cx) != TextInputTarget::SettingsGitActionScript {
            return None;
        }
        let kind = self.focused_settings_git_action_script_kind()?;
        let input = self.settings_git_action_script_input(kind);

        let byte_range =
            utf16_range_to_byte_range(&input.draft, clamp_utf16_range(&input.draft, range_utf16));
        let mut line_bounds = self
            .settings_git_action_script_layout(kind)
            .iter()
            .filter_map(|line| {
                let start = byte_range.start.max(line.range.start);
                let end = byte_range.end.min(line.range.end);
                (start <= end).then(|| {
                    let local_start = start - line.range.start;
                    let local_end = end - line.range.start;
                    Bounds::from_corners(
                        gpui::point(
                            line.bounds.left() + line.line.x_for_index(local_start),
                            line.bounds.top(),
                        ),
                        gpui::point(
                            line.bounds.left() + line.line.x_for_index(local_end),
                            line.bounds.bottom(),
                        ),
                    )
                })
            });
        let first = line_bounds.next()?;
        Some(line_bounds.fold(first, |acc, bounds| {
            Bounds::from_corners(
                gpui::point(acc.left().min(bounds.left()), acc.top().min(bounds.top())),
                gpui::point(
                    acc.right().max(bounds.right()),
                    acc.bottom().max(bounds.bottom()),
                ),
            )
        }))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.text_input_target(cx) != TextInputTarget::SettingsGitActionScript {
            return None;
        }
        let kind = self.focused_settings_git_action_script_kind()?;

        Some(byte_to_utf16_offset(
            &self.settings_git_action_script_input(kind).draft,
            self.settings_git_action_script_index_for_point(kind, point),
        ))
    }
}

fn text_for_utf16_range(
    text: &str,
    range: std::ops::Range<usize>,
    adjusted_range: &mut Option<std::ops::Range<usize>>,
) -> String {
    let clamped_range = clamp_utf16_range(text, range);
    *adjusted_range = Some(clamped_range.clone());
    let byte_range = utf16_range_to_byte_range(text, clamped_range);
    text[byte_range].to_string()
}

fn utf16_selection_for_text(
    text: &str,
    cursor: usize,
    selection_anchor: Option<usize>,
) -> UTF16Selection {
    let cursor_utf16 = byte_to_utf16_offset(text, cursor.min(text.len()));
    if let Some(anchor) = selection_anchor {
        let anchor_utf16 = byte_to_utf16_offset(text, anchor.min(text.len()));
        UTF16Selection {
            range: anchor_utf16.min(cursor_utf16)..anchor_utf16.max(cursor_utf16),
            reversed: anchor_utf16 > cursor_utf16,
        }
    } else {
        UTF16Selection {
            range: cursor_utf16..cursor_utf16,
            reversed: false,
        }
    }
}

fn replace_custom_text(
    current_text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    range_utf16: Option<std::ops::Range<usize>>,
    new_text: &str,
    preserve_newlines: bool,
) {
    let replacement = if preserve_newlines {
        new_text.replace('\r', "")
    } else {
        sanitize_single_line_input(new_text)
    };
    let current_selection = selection_anchor
        .map(|anchor| anchor.min(*cursor)..anchor.max(*cursor))
        .filter(|range| range.start != range.end)
        .unwrap_or(*cursor..*cursor);
    let replacement_range = range_utf16
        .map(|range| {
            utf16_range_to_byte_range(current_text, clamp_utf16_range(current_text, range))
        })
        .unwrap_or(current_selection);

    current_text.replace_range(replacement_range.clone(), &replacement);
    *cursor = replacement_range.start + replacement.len();
    *selection_anchor = None;
}

fn sanitize_single_line_input(text: &str) -> String {
    text.replace(['\n', '\r', '\t'], " ")
}

fn clamp_utf16_range(text: &str, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let max = text.encode_utf16().count();
    range.start.min(max)..range.end.min(max)
}

fn utf16_range_to_byte_range(text: &str, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    utf16_offset_to_byte(text, range.start)..utf16_offset_to_byte(text, range.end)
}

fn utf16_offset_to_byte(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }

    let mut utf16_count = 0;
    for (byte_index, ch) in text.char_indices() {
        let next = utf16_count + ch.len_utf16();
        if offset < next {
            return byte_index;
        }
        if offset == next {
            return byte_index + ch.len_utf8();
        }
        utf16_count = next;
    }

    text.len()
}

fn byte_to_utf16_offset(text: &str, byte_offset: usize) -> usize {
    let clamped = byte_offset.min(text.len());
    text[..clamped].encode_utf16().count()
}

fn terminal_selection_range(
    anchor: TerminalCellPosition,
    head: TerminalCellPosition,
) -> Option<TerminalSelectionRange> {
    if anchor == head {
        return None;
    }

    let (start, end) = if (anchor.line, anchor.column) <= (head.line, head.column) {
        (anchor, head)
    } else {
        (head, anchor)
    };

    Some(TerminalSelectionRange {
        start_line: start.line,
        start_column: start.column,
        end_line: end.line,
        end_column: end.column,
    })
}

fn terminal_cell_position_from_mouse(
    point: Point<Pixels>,
    metrics: &TerminalPanelMetrics,
) -> Option<TerminalCellPosition> {
    if metrics.columns == 0 || metrics.rows == 0 {
        return None;
    }

    let x = (f32::from(point.x) - metrics.left - metrics.padding).max(0.0);
    let y = (f32::from(point.y) - metrics.top - metrics.padding).max(0.0);
    let column = (x / metrics.cell_width)
        .floor()
        .clamp(0.0, (metrics.columns.saturating_sub(1)) as f32) as usize;
    let line = (y / metrics.cell_height)
        .floor()
        .clamp(0.0, (metrics.rows.saturating_sub(1)) as f32) as usize;

    Some(TerminalCellPosition { line, column })
}

fn terminal_selected_text(
    snapshot: &TerminalSurfaceSnapshot,
    selection: TerminalSelectionRange,
) -> Option<String> {
    if snapshot.lines.is_empty() || snapshot.columns == 0 {
        return None;
    }

    let last_line = snapshot.lines.len().saturating_sub(1);
    let start_line = selection.start_line.min(last_line);
    let end_line = selection.end_line.min(last_line);
    let mut lines = Vec::new();

    for line_index in start_line..=end_line {
        let line = snapshot.lines.get(line_index)?;
        let mut line_text = String::new();
        let start_column = if line_index == start_line {
            selection
                .start_column
                .min(snapshot.columns.saturating_sub(1))
        } else {
            0
        };
        let end_column = if line_index == end_line {
            selection.end_column.min(snapshot.columns.saturating_sub(1))
        } else {
            snapshot.columns.saturating_sub(1)
        };

        for cell in &line.cells {
            if cell.column > end_column {
                break;
            }
            if cell.column + cell.width <= start_column {
                continue;
            }
            line_text.push_str(&cell.copy_text);
        }

        lines.push(line_text.trim_end_matches(' ').to_string());
    }

    Some(lines.join("\n"))
}

fn terminal_scroll_lines(
    delta: ScrollDelta,
    line_height: Pixels,
    remainder_lines: f32,
) -> (i32, f32) {
    let delta_lines = f32::from(delta.pixel_delta(line_height).y) / f32::from(line_height);
    let total_lines = remainder_lines + delta_lines;
    let whole_lines = if total_lines >= 0.0 {
        total_lines.floor()
    } else {
        total_lines.ceil()
    };

    (whole_lines as i32, total_lines - whole_lines)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalCellCategory {
    Whitespace,
    Word,
    Punctuation,
}

fn terminal_word_selection_range(
    snapshot: &TerminalSurfaceSnapshot,
    position: TerminalCellPosition,
) -> Option<TerminalSelectionRange> {
    let line = snapshot.lines.get(position.line)?;
    let clicked_index = line.cells.iter().position(|cell| {
        cell.column <= position.column && position.column < cell.column + cell.width
    })?;
    let clicked_cell = line.cells.get(clicked_index)?;

    let mut start_column = clicked_cell.column;
    let mut end_column = clicked_cell.column + clicked_cell.width - 1;
    let category = terminal_cell_category(clicked_cell);

    let mut left_index = clicked_index;
    while left_index > 0 {
        let candidate = &line.cells[left_index - 1];
        if terminal_cell_category(candidate) != category {
            break;
        }
        start_column = candidate.column;
        left_index -= 1;
    }

    let mut right_index = clicked_index;
    while right_index + 1 < line.cells.len() {
        let candidate = &line.cells[right_index + 1];
        if terminal_cell_category(candidate) != category {
            break;
        }
        end_column = candidate.column + candidate.width - 1;
        right_index += 1;
    }

    Some(TerminalSelectionRange {
        start_line: position.line,
        start_column,
        end_line: position.line,
        end_column,
    })
}

fn terminal_line_selection_range(
    snapshot: &TerminalSurfaceSnapshot,
    position: TerminalCellPosition,
) -> Option<TerminalSelectionRange> {
    if snapshot.columns == 0 || snapshot.lines.get(position.line).is_none() {
        return None;
    }

    Some(TerminalSelectionRange {
        start_line: position.line,
        start_column: 0,
        end_line: position.line,
        end_column: snapshot.columns.saturating_sub(1),
    })
}

pub(crate) fn terminal_open_link_modifier_held(modifiers: gpui::Modifiers) -> bool {
    #[cfg(target_os = "macos")]
    {
        modifiers.platform
    }
    #[cfg(not(target_os = "macos"))]
    {
        modifiers.control && !modifiers.platform
    }
}

fn terminal_link_at_position(
    snapshot: &TerminalSurfaceSnapshot,
    position: TerminalCellPosition,
) -> Option<String> {
    let line = snapshot.lines.get(position.line)?;
    let clicked_cell = line
        .cells
        .iter()
        .find(|cell| cell.column <= position.column && position.column < cell.column + cell.width);

    if let Some(link) = clicked_cell
        .and_then(|cell| cell.hyperlink.as_deref())
        .and_then(normalize_terminal_link)
    {
        return Some(link);
    }

    terminal_text_link_at_column(line, position.column)
}

pub(crate) fn terminal_link_ranges(snapshot: &TerminalSurfaceSnapshot) -> Vec<TerminalLinkRange> {
    let mut ranges = Vec::new();

    for (line_index, line) in snapshot.lines.iter().enumerate() {
        let mut current_explicit: Option<(String, usize, usize)> = None;
        for cell in &line.cells {
            let link = cell.hyperlink.as_deref().and_then(normalize_terminal_link);
            if let (Some((current_link, _, end_column)), Some(link)) =
                (current_explicit.as_mut(), link.as_ref())
            {
                if current_link == link && cell.column <= *end_column {
                    *end_column = (*end_column).max(cell.column + cell.width);
                    continue;
                }
            }

            if let Some((_, start_column, end_column)) = current_explicit.take() {
                ranges.push(TerminalLinkRange {
                    line: line_index,
                    start_column,
                    end_column,
                });
            }
            if let Some(link) = link {
                current_explicit = Some((link, cell.column, cell.column + cell.width));
            }
        }
        if let Some((_, start_column, end_column)) = current_explicit.take() {
            ranges.push(TerminalLinkRange {
                line: line_index,
                start_column,
                end_column,
            });
        }

        ranges.extend(terminal_text_link_ranges(line).into_iter().map(
            |(start_column, end_column)| TerminalLinkRange {
                line: line_index,
                start_column,
                end_column,
            },
        ));
    }

    ranges
}

fn terminal_text_link_at_column(
    line: &crate::terminal_runtime::TerminalLineSnapshot,
    column: usize,
) -> Option<String> {
    let mut text = String::new();
    let mut clicked_byte = None;
    let mut display_column = 0;

    for cell in &line.cells {
        while display_column < cell.column {
            if display_column == column {
                clicked_byte = Some(text.len());
            }
            text.push(' ');
            display_column += 1;
        }

        let start = text.len();
        text.push_str(&cell.copy_text);
        let end = text.len();
        if cell.column <= column && column < cell.column + cell.width {
            clicked_byte = Some(start);
        }
        display_column = cell.column + cell.width;

        if start == end && clicked_byte.is_some() {
            break;
        }
    }

    let clicked_byte = clicked_byte?;
    terminal_text_link_at_byte(&text, clicked_byte)
}

fn terminal_text_link_ranges(
    line: &crate::terminal_runtime::TerminalLineSnapshot,
) -> Vec<(usize, usize)> {
    let mut text = String::new();
    let mut byte_columns = vec![0];
    let mut display_column = 0;

    for cell in &line.cells {
        while display_column < cell.column {
            text.push(' ');
            display_column += 1;
            byte_columns.push(display_column);
        }

        let cell_start = cell.column;
        text.push_str(&cell.copy_text);
        for _ in 0..cell.copy_text.len() {
            byte_columns.push(cell_start);
        }
        display_column = cell.column + cell.width;
        if let Some(last) = byte_columns.last_mut() {
            *last = display_column;
        }
    }

    terminal_text_link_byte_ranges(&text)
        .into_iter()
        .filter_map(|(start, end)| {
            let start_column = byte_columns.get(start).copied()?;
            let end_column = byte_columns.get(end).copied()?;
            (start_column < end_column).then_some((start_column, end_column))
        })
        .collect()
}

fn terminal_text_link_at_byte(text: &str, clicked_byte: usize) -> Option<String> {
    terminal_text_link_byte_ranges(text)
        .into_iter()
        .find_map(|(start, end)| {
            if start <= clicked_byte && clicked_byte < end {
                normalize_terminal_link(&text[start..end])
            } else {
                None
            }
        })
}

fn terminal_text_link_byte_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut search_start = 0;
    while search_start < text.len() {
        let Some((prefix_offset, prefix)) = terminal_link_prefix_after(text, search_start) else {
            break;
        };
        let start = search_start + prefix_offset;
        let mut end = start + prefix.len();
        for (offset, ch) in text[end..].char_indices() {
            if ch.is_whitespace() || ch.is_control() {
                break;
            }
            end = start + prefix.len() + offset + ch.len_utf8();
        }

        let link = trim_terminal_link_suffix(&text[start..end]);
        let link_end = start + link.len();
        if normalize_terminal_link(link).is_some() {
            ranges.push((start, link_end));
        }
        search_start = end.max(start + prefix.len());
    }

    ranges
}

const TERMINAL_LINK_SCHEMES: &[&str] = &[
    "https://",
    "http://",
    "ssh://",
    "sftp://",
    "ftps://",
    "ftp://",
    "file://",
    "git://",
    "vscode://",
    "mailto:",
];

fn terminal_link_prefix_after(text: &str, start: usize) -> Option<(usize, &'static str)> {
    TERMINAL_LINK_SCHEMES
        .iter()
        .copied()
        .filter_map(|prefix| text[start..].find(prefix).map(|offset| (offset, prefix)))
        .min_by_key(|(offset, _)| *offset)
}

fn trim_terminal_link_suffix(link: &str) -> &str {
    link.trim_end_matches(|ch: char| {
        matches!(
            ch,
            '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '"'
        )
    })
}

fn normalize_terminal_link(link: &str) -> Option<String> {
    let link = trim_terminal_link_suffix(link.trim());
    let scheme = TERMINAL_LINK_SCHEMES
        .iter()
        .copied()
        .find(|prefix| link.starts_with(prefix))?;
    let body = &link[scheme.len()..];
    if body.is_empty() || body.chars().any(char::is_whitespace) {
        return None;
    }
    Some(link.to_string())
}

/// Mouse buttons we report to the application. Mirrors xterm's encoding
/// table — left/middle/right map to button bits 0/1/2; wheel up/down ride
/// in the high "extra" range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalMouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
    /// Used for motion events while no button is held (any-motion mode).
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalMouseAction {
    Press,
    Release,
    /// Pointer moved while at least one button is held — encoded with the
    /// motion bit (32) plus the held button.
    Drag,
    /// Pointer moved with no buttons held (only emitted in any-motion mode).
    Motion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct TerminalMouseModifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

/// Encode a mouse event to the byte sequence the running TUI expects on
/// stdin. `col` and `row` are 0-based cell coordinates (we add 1 here per
/// the wire protocol). Returns `None` when the event isn't legal under the
/// negotiated mode (e.g. a motion event in click-only mode).
pub(crate) fn encode_terminal_mouse_event(
    protocol: TerminalMouseProtocol,
    button: TerminalMouseButton,
    action: TerminalMouseAction,
    col: usize,
    row: usize,
    modifiers: TerminalMouseModifiers,
) -> Option<Vec<u8>> {
    match action {
        TerminalMouseAction::Press | TerminalMouseAction::Release => {}
        TerminalMouseAction::Drag => {
            if matches!(protocol.level, TerminalMouseLevel::ClickOnly) {
                return None;
            }
        }
        TerminalMouseAction::Motion => {
            if !matches!(protocol.level, TerminalMouseLevel::AnyMotion) {
                return None;
            }
        }
    }
    if matches!(action, TerminalMouseAction::Release)
        && matches!(protocol.level, TerminalMouseLevel::ClickOnly)
    {
        // X10-style click-only mode never reports releases.
        return None;
    }

    let mut button_code: u32 = match button {
        TerminalMouseButton::Left => 0,
        TerminalMouseButton::Middle => 1,
        TerminalMouseButton::Right => 2,
        TerminalMouseButton::None => 3,
        TerminalMouseButton::WheelUp => 64,
        TerminalMouseButton::WheelDown => 65,
        TerminalMouseButton::WheelLeft => 66,
        TerminalMouseButton::WheelRight => 67,
    };
    if matches!(
        action,
        TerminalMouseAction::Drag | TerminalMouseAction::Motion
    ) {
        button_code += 32;
    }
    if modifiers.shift {
        button_code += 4;
    }
    if modifiers.alt {
        button_code += 8;
    }
    if modifiers.control {
        button_code += 16;
    }

    // Wire columns/rows are 1-based.
    let wire_col = col.saturating_add(1);
    let wire_row = row.saturating_add(1);

    match protocol.encoding {
        TerminalMouseEncoding::Sgr => {
            // SGR signals release with a trailing `m`; the X10
            // "always-button-3" release quirk does NOT apply here — the
            // actual button code is preserved so the app knows which
            // button was released.
            let trailer = if matches!(action, TerminalMouseAction::Release) {
                'm'
            } else {
                'M'
            };
            Some(
                format!(
                    "\u{1b}[<{};{};{}{}",
                    button_code, wire_col, wire_row, trailer
                )
                .into_bytes(),
            )
        }
        TerminalMouseEncoding::Default => {
            // Legacy CSI M payload: bytes are clamped to 32..=255, columns
            // beyond 223 are dropped per xterm.
            let cb = (button_code as u8).saturating_add(32);
            let cx = (wire_col.min(223) as u8).saturating_add(32);
            let cy = (wire_row.min(223) as u8).saturating_add(32);
            let release_cb = if matches!(action, TerminalMouseAction::Release) {
                3u8.saturating_add(32)
            } else {
                cb
            };
            Some(vec![0x1b, b'[', b'M', release_cb, cx, cy])
        }
        TerminalMouseEncoding::Utf8 => {
            // ?1005h: same shape as Default but col/row are encoded as
            // UTF-8 codepoints, so columns up to 2015 are reachable.
            let mut buf = Vec::with_capacity(8);
            buf.extend_from_slice(b"\x1b[M");
            let cb_value = if matches!(action, TerminalMouseAction::Release) {
                3u32 + 32
            } else {
                button_code + 32
            };
            push_utf8_mouse_byte(&mut buf, cb_value);
            push_utf8_mouse_byte(&mut buf, wire_col.min(2015) as u32 + 32);
            push_utf8_mouse_byte(&mut buf, wire_row.min(2015) as u32 + 32);
            Some(buf)
        }
    }
}

fn push_utf8_mouse_byte(buf: &mut Vec<u8>, value: u32) {
    if value < 0x80 {
        buf.push(value as u8);
    } else if let Some(ch) = char::from_u32(value) {
        let mut tmp = [0u8; 4];
        buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
    } else {
        buf.push(b'?');
    }
}

fn terminal_cell_category(
    cell: &crate::terminal_runtime::TerminalCellSnapshot,
) -> TerminalCellCategory {
    let ch = cell.copy_text.chars().next().unwrap_or(' ');
    if ch.is_whitespace() {
        TerminalCellCategory::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        TerminalCellCategory::Word
    } else {
        TerminalCellCategory::Punctuation
    }
}

impl AnotherOneApp {
    fn text_input_target(&self, cx: &App) -> TextInputTarget {
        if self
            .new_task_modal
            .as_ref()
            .is_some_and(|state| state.submitting)
        {
            return TextInputTarget::Blocked;
        }

        if self
            .new_task_modal
            .as_ref()
            .is_some_and(|state| state.task_name_focused)
        {
            return TextInputTarget::NewTaskModal;
        }

        if self
            .new_task_modal
            .as_ref()
            .is_some_and(|state| state.branch_filter_focused)
        {
            return TextInputTarget::NewTaskModalBranchFilter;
        }

        if self.new_task_modal.is_some() {
            return TextInputTarget::Blocked;
        }

        if self
            .create_branch_modal
            .as_ref()
            .is_some_and(|state| state.submitting)
        {
            return TextInputTarget::Blocked;
        }

        if self.create_branch_modal.is_some() {
            return TextInputTarget::CreateBranchModal;
        }

        if self.add_agent_modal.is_some() {
            return TextInputTarget::Blocked;
        }

        if self
            .custom_action_modal
            .as_ref()
            .is_some_and(|state| state.focused_text_value().is_some())
        {
            return TextInputTarget::CustomActionModal;
        }

        if self.custom_action_modal.is_some() {
            return TextInputTarget::Blocked;
        }

        if self
            .workspace_pane
            .read(cx)
            .pinned_tab_close_confirm
            .is_some()
        {
            return TextInputTarget::Blocked;
        }

        if self.sidebar_task_rename.is_some() {
            return TextInputTarget::SidebarTaskRename;
        }

        if self.settings_open {
            if self.settings_section == crate::settings_page::SettingsSection::Agents
                && self.settings_agent_input.focused_agent_id.is_some()
            {
                return TextInputTarget::SettingsAgentInput;
            }
            if self.settings_section == crate::settings_page::SettingsSection::GitActions
                && self.focused_settings_git_action_script_kind().is_some()
            {
                return TextInputTarget::SettingsGitActionScript;
            }
            return TextInputTarget::Blocked;
        }

        let workspace = self.workspace_pane.read(cx);
        if workspace.active_project_page.is_none() && workspace.active_section.is_some() {
            return TextInputTarget::Terminal;
        }

        TextInputTarget::Blocked
    }

    pub(crate) fn action_tooltip_view(label: &'static str, cx: &mut App) -> AnyView {
        cx.new(|_| ActionTooltip {
            label: label.into(),
        })
        .into()
    }

    pub(crate) fn show_success_toast(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.show_toast(ToastKind::Success, message, cx);
    }

    pub(crate) fn show_error_toast(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.show_toast(ToastKind::Error, message, cx);
    }

    pub(crate) fn show_error_details_toast(
        &mut self,
        message: impl Into<SharedString>,
        copy_message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.show_toast_with_copy_message(ToastKind::Error, message, copy_message, cx);
    }

    pub(crate) fn show_anyhow_error_toast(
        &mut self,
        message: impl Into<SharedString>,
        error: &anyhow::Error,
        cx: &mut Context<Self>,
    ) {
        self.show_error_details_toast(message, Self::anyhow_error_details(error), cx);
    }

    pub(crate) fn show_warning_toast(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.show_toast(ToastKind::Warning, message, cx);
    }

    pub(crate) fn enabled_open_in_apps(&self) -> Vec<OpenInAppKind> {
        self.project_store
            .enabled_open_in_apps(&self.available_open_in_apps)
    }

    pub(crate) fn enabled_agents(&self) -> Vec<&'static AgentDef> {
        effective_enabled_agents(self.project_store.ui.enabled_agents.as_ref())
    }

    pub(crate) fn agent_enabled(&self, agent_id: &str) -> bool {
        self.project_store.agent_enabled(agent_id)
    }

    pub(crate) fn default_agent_id(&self) -> Option<&'static str> {
        self.project_store.default_agent_id()
    }

    pub(crate) fn agent_is_default(&self, agent_id: &str) -> bool {
        self.project_store.agent_is_default(agent_id)
    }

    pub(crate) fn preferred_open_in_app(&self) -> Option<OpenInAppKind> {
        self.project_store
            .preferred_open_in_app(&self.available_open_in_apps)
    }

    pub(crate) fn active_open_in_project_id(&self, cx: &App) -> Option<String> {
        let workspace = self.workspace_pane.read(cx);
        workspace.active_project_page.clone().or_else(|| {
            workspace
                .active_section
                .as_ref()
                .map(|section| section.project_id.clone())
        })
    }

    pub(crate) fn commit_page_size_for_project(&self, project_id: &str) -> usize {
        self.commit_page_sizes
            .get(project_id)
            .copied()
            .unwrap_or(RECENT_COMMITS_PAGE_SIZE)
    }

    pub(crate) fn active_branch_commit_state(&self, cx: &App) -> Option<&ProjectBranchCommitState> {
        let project_id = self
            .workspace_pane
            .read(cx)
            .active_section
            .as_ref()?
            .project_id
            .clone();
        self.branch_commit_states.get(&project_id)
    }

    pub(crate) fn active_right_sidebar_mode(&self, _cx: &App) -> RightSidebarMode {
        match self.right_sidebar_mode {
            RightSidebarMode::WorkingTree => RightSidebarMode::WorkingTree,
            RightSidebarMode::Commits => RightSidebarMode::Commits,
            RightSidebarMode::Checks => RightSidebarMode::Checks,
        }
    }

    fn clear_branch_sidebar_states_for_project_group(&mut self, project_id: &str) {
        let Some(repo_id) = self
            .project_store
            .project(project_id)
            .map(|project| project.repo_id.clone())
        else {
            return;
        };

        self.branch_commit_states.retain(|cached_project_id, _| {
            self.project_store
                .project(cached_project_id)
                .is_some_and(|project| project.repo_id != repo_id)
        });
        self.commit_file_changes_states.retain(|key, _| {
            let cached_project_id = key.split(':').next().unwrap_or_default();
            self.project_store
                .project(cached_project_id)
                .is_some_and(|project| project.repo_id != repo_id)
        });
        self.project_check_runs_states.retain(|key, _| {
            let cached_project_id = key.split(':').next().unwrap_or_default();
            self.project_store
                .project(cached_project_id)
                .is_some_and(|project| project.repo_id != repo_id)
        });
        self.project_check_runs_requests.retain(|key| {
            let cached_project_id = key.split(':').next().unwrap_or_default();
            self.project_store
                .project(cached_project_id)
                .is_some_and(|project| project.repo_id != repo_id)
        });
        self.project_check_runs_checked_at.retain(|key, _| {
            let cached_project_id = key.split(':').next().unwrap_or_default();
            self.project_store
                .project(cached_project_id)
                .is_some_and(|project| project.repo_id != repo_id)
        });
    }

    pub(crate) fn active_toolbar_repo_id(&self, cx: &App) -> Option<String> {
        let project_id = self.active_open_in_project_id(cx)?;
        self.project_store
            .project(&project_id)
            .map(|project| project.repo_id.clone())
    }

    fn project_pull_request_lookup_key(project_id: &str, branch_name: &str) -> String {
        format!("{project_id}:{branch_name}")
    }

    fn project_check_runs_lookup_key(project_id: &str, branch_name: &str) -> String {
        format!("{project_id}:{branch_name}")
    }

    pub(crate) fn prefetch_section_pull_request_and_checks(
        &mut self,
        section_id: &SectionId,
        project_path: &std::path::Path,
    ) {
        self.request_project_pull_request_lookup_for(
            &section_id.project_id,
            &section_id.branch_name,
            project_path,
        );

        if let Some(pull_request) = self
            .project_pull_request(&section_id.project_id, &section_id.branch_name)
            .cloned()
        {
            let lookup_key = Self::project_check_runs_lookup_key(
                &section_id.project_id,
                &section_id.branch_name,
            );
            self.request_project_check_runs_lookup(
                &lookup_key,
                project_path,
                Some(pull_request.number),
            );
        }
    }

    fn request_active_project_github_link_lookup(&mut self, cx: &App) {
        let Some(project_id) = self.active_open_in_project_id(cx) else {
            return;
        };
        let Some(project_path) = self.project_path(&project_id) else {
            return;
        };

        self.request_project_github_link_lookup(&project_id, &project_path);
    }

    fn project_pull_request_lookup_is_fresh(&self, lookup_key: &str) -> bool {
        self.project_pull_request_checked_at
            .get(lookup_key)
            .is_some_and(|checked_at| checked_at.elapsed() < PULL_REQUEST_LOOKUP_TTL)
    }

    fn project_check_runs_lookup_ttl(&self, lookup_key: &str) -> Duration {
        match self.project_check_runs_states.get(lookup_key) {
            Some(ProjectCheckRunsState::Loaded(checks))
                if checks.iter().any(|check| {
                    check.bucket == crate::git_actions::PullRequestCheckBucket::Pending
                }) =>
            {
                PENDING_CHECK_RUNS_LOOKUP_TTL
            }
            _ => CHECK_RUNS_LOOKUP_TTL,
        }
    }

    fn project_check_runs_lookup_is_fresh(&self, lookup_key: &str) -> bool {
        self.project_check_runs_checked_at
            .get(lookup_key)
            .is_some_and(|checked_at| {
                checked_at.elapsed() < self.project_check_runs_lookup_ttl(lookup_key)
            })
    }

    fn active_project_pull_request_context(
        &self,
        cx: &App,
    ) -> Option<(String, String, std::path::PathBuf)> {
        let project_id = self.active_open_in_project_id(cx)?;
        let branch_name = self.project_store.current_branch_name(&project_id)?;
        let project_path = self.project_path(&project_id)?;
        Some((project_id, branch_name, project_path))
    }

    fn active_project_check_runs_context(
        &self,
        cx: &App,
    ) -> Option<(String, String, std::path::PathBuf, Option<u64>)> {
        let section = self.workspace_pane.read(cx).active_section.clone()?;
        let project_path = self.project_path(&section.project_id)?;
        let pull_request_number = self
            .project_pull_request(&section.project_id, &section.branch_name)
            .map(|pull_request| pull_request.number);
        Some((
            section.project_id,
            section.branch_name,
            project_path,
            pull_request_number,
        ))
    }

    pub(crate) fn active_project_pull_request_lookup_checked(&self, cx: &App) -> bool {
        let Some((project_id, branch_name, _)) = self.active_project_pull_request_context(cx)
        else {
            return false;
        };
        let lookup_key = Self::project_pull_request_lookup_key(&project_id, &branch_name);
        self.project_pull_request_checked.contains(&lookup_key)
            && self.project_pull_request_lookup_is_fresh(&lookup_key)
    }

    pub(crate) fn active_project_pull_request_url(&self, cx: &App) -> Option<String> {
        let pull_request = self.active_project_pull_request(cx)?;
        (pull_request.state == crate::git_actions::PullRequestState::Open)
            .then_some(pull_request.url.clone())
    }

    pub(crate) fn active_project_pull_request(
        &self,
        cx: &App,
    ) -> Option<&crate::git_actions::PullRequestStatus> {
        let (project_id, branch_name, _) = self.active_project_pull_request_context(cx)?;
        let lookup_key = Self::project_pull_request_lookup_key(&project_id, &branch_name);
        self.project_pull_requests.get(&lookup_key)
    }

    pub(crate) fn project_pull_request(
        &self,
        project_id: &str,
        branch_name: &str,
    ) -> Option<&crate::git_actions::PullRequestStatus> {
        let lookup_key = Self::project_pull_request_lookup_key(project_id, branch_name);
        self.project_pull_requests.get(&lookup_key)
    }

    fn invalidate_project_pull_request_lookup(&mut self, project_id: &str, branch_name: &str) {
        let lookup_key = Self::project_pull_request_lookup_key(project_id, branch_name);
        self.project_pull_request_requests.remove(&lookup_key);
        self.project_pull_request_checked.remove(&lookup_key);
        self.project_pull_request_checked_at.remove(&lookup_key);
        self.project_pull_requests.remove(&lookup_key);
    }

    fn invalidate_project_check_runs_lookup(&mut self, project_id: &str, branch_name: &str) {
        let lookup_key = Self::project_check_runs_lookup_key(project_id, branch_name);
        self.project_check_runs_requests.remove(&lookup_key);
        self.project_check_runs_checked_at.remove(&lookup_key);
        self.project_check_runs_states.remove(&lookup_key);
    }

    pub(crate) fn active_project_check_runs_state(
        &self,
        cx: &App,
    ) -> Option<&ProjectCheckRunsState> {
        let (project_id, branch_name, _, _) = self.active_project_check_runs_context(cx)?;
        let lookup_key = Self::project_check_runs_lookup_key(&project_id, &branch_name);
        self.project_check_runs_states.get(&lookup_key)
    }

    pub(crate) fn request_project_pull_request_lookup_for(
        &mut self,
        project_id: &str,
        branch_name: &str,
        project_path: &std::path::Path,
    ) {
        let lookup_key = Self::project_pull_request_lookup_key(project_id, branch_name);
        self.request_project_pull_request_lookup(&lookup_key, branch_name, project_path);
    }

    pub(crate) fn refresh_active_project_pull_request_lookup(&mut self, cx: &App) {
        let Some((project_id, branch_name, project_path)) =
            self.active_project_pull_request_context(cx)
        else {
            return;
        };
        let lookup_key = Self::project_pull_request_lookup_key(&project_id, &branch_name);
        self.request_project_pull_request_lookup(&lookup_key, &branch_name, &project_path);
    }

    pub(crate) fn request_active_project_check_runs_lookup(&mut self, cx: &App) {
        let Some((project_id, branch_name, project_path, pull_request_number)) =
            self.active_project_check_runs_context(cx)
        else {
            return;
        };
        if pull_request_number.is_none() && !self.active_project_pull_request_lookup_checked(cx) {
            self.refresh_active_project_pull_request_lookup(cx);
            return;
        }
        let lookup_key = Self::project_check_runs_lookup_key(&project_id, &branch_name);
        self.request_project_check_runs_lookup(&lookup_key, &project_path, pull_request_number);
    }

    pub(crate) fn active_project_ahead_count(&self, cx: &App) -> usize {
        let workspace = self.workspace_pane.read(cx);

        if let Some(section) = workspace.active_section.as_ref() {
            return self
                .project_store
                .branch_view(&section.project_id, &section.branch_name)
                .map(|branch| branch.ahead_count)
                .unwrap_or(0);
        }

        workspace
            .active_project_page
            .as_deref()
            .and_then(|project_id| {
                self.project_store
                    .primary_branch_for_project(project_id, false)
            })
            .map(|branch| branch.ahead_count)
            .unwrap_or(0)
    }

    pub(crate) fn active_project_behind_count(&self, cx: &App) -> usize {
        let workspace = self.workspace_pane.read(cx);

        if let Some(section) = workspace.active_section.as_ref() {
            return self
                .project_store
                .branch_view(&section.project_id, &section.branch_name)
                .map(|branch| branch.behind_count)
                .unwrap_or(0);
        }

        workspace
            .active_project_page
            .as_deref()
            .and_then(|project_id| {
                self.project_store
                    .primary_branch_for_project(project_id, false)
            })
            .map(|branch| branch.behind_count)
            .unwrap_or(0)
    }

    pub(crate) fn open_in_app_enabled(&self, app: OpenInAppKind) -> bool {
        self.project_store
            .open_in_app_enabled(app, &self.available_open_in_apps)
    }

    pub(crate) fn set_theme_mode(
        &mut self,
        mode: crate::project_store::ThemeMode,
        cx: &mut Context<Self>,
    ) {
        self.project_store.set_theme_mode(mode);
        self.sync_registry_project_store();
        // Republish a best-guess resolved theme immediately so the
        // alacritty cell renderer picks up the new defaults before the
        // next frame. The render path will refine this with the actual
        // OS appearance once it has a Window in hand.
        let resolved = match mode {
            crate::project_store::ThemeMode::Light => crate::theme::ResolvedTheme::Light,
            crate::project_store::ThemeMode::Dark => crate::theme::ResolvedTheme::Dark,
            crate::project_store::ThemeMode::System => crate::theme::current_terminal_theme(),
        };
        crate::theme::set_terminal_theme(resolved);
        // Rebuild cached terminal surface snapshots against the new default
        // fg/bg. Alacritty's grid does not change when the app theme changes,
        // so the runtime cache must be explicitly invalidated.
        self.terminal_surface_snapshots.clear();
        for (key, runtime) in &mut self.live_terminal_runtimes {
            runtime.invalidate_snapshot();
            self.terminal_surface_snapshots
                .insert(key.clone(), runtime.snapshot());
        }
        cx.notify();
    }

    pub(crate) fn open_settings_section(
        &mut self,
        section: crate::settings_page::SettingsSection,
        cx: &mut Context<Self>,
    ) {
        self.settings_open = true;
        self.settings_section = section;
        self.shortcut_capture_action = None;
        self.settings_agent_input.focused_agent_id = None;
        self.settings_agent_input.selection_anchor = None;
        self.settings_git_commit_script_input.focused = false;
        self.settings_git_commit_script_input.selection_anchor = None;
        self.settings_git_pr_script_input.focused = false;
        self.settings_git_pr_script_input.selection_anchor = None;
        if section == crate::settings_page::SettingsSection::GitActions {
            self.sync_settings_git_action_script_from_store(SettingsGitActionScriptKind::Commit);
            self.sync_settings_git_action_script_from_store(
                SettingsGitActionScriptKind::PullRequest,
            );
        }
        self.dismiss_titlebar_dropdowns();
        cx.stop_propagation();
        cx.notify();
    }

    fn agent_launch_args_for_launch_config(
        &self,
        launch_config: &TerminalLaunchConfig,
    ) -> Vec<String> {
        if !launch_config.use_agent_launch_args {
            return Vec::new();
        }

        launch_config
            .provider
            .and_then(agent_id_for_provider)
            .map(|agent_id| self.project_store.agent_launch_args(agent_id).to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn dismiss_titlebar_dropdowns(&mut self) -> bool {
        let had_open_in = self.project_page_open_in_menu_project_id.take().is_some();
        let had_git_actions = self.git_actions_menu_open;
        let had_custom_actions = self.custom_actions_menu_open;
        self.git_actions_menu_open = false;
        self.custom_actions_menu_open = false;
        let had_project_config = self.project_page_config_dropdown.take().is_some();
        if had_project_config {
            self.project_page_config_panel_targeted = false;
        }
        had_open_in || had_git_actions || had_custom_actions || had_project_config
    }

    pub(crate) fn toggle_project_page_config_panel(&mut self, cx: &mut Context<Self>) {
        self.project_page_config_panel_expanded = !self.project_page_config_panel_expanded;
        if !self.project_page_config_panel_expanded {
            self.project_page_config_dropdown = None;
            self.project_page_config_panel_targeted = false;
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn toggle_project_page_config_dropdown(
        &mut self,
        field: ProjectBranchSettingField,
        cx: &mut Context<Self>,
    ) {
        self.project_page_open_in_menu_project_id = None;
        self.git_actions_menu_open = false;
        self.custom_actions_menu_open = false;
        self.project_page_config_panel_expanded = true;
        self.project_page_config_panel_targeted = false;
        self.project_page_config_dropdown = if self.project_page_config_dropdown == Some(field) {
            None
        } else {
            Some(field)
        };
        cx.stop_propagation();
        cx.notify();
    }

    fn show_invalid_project_branch_setting_toast(
        &mut self,
        invalid: InvalidProjectBranchSetting,
        cx: &mut Context<Self>,
    ) {
        match invalid.field {
            ProjectBranchSettingField::DefaultBranch => {
                if let Some(fallback_branch) = invalid.fallback_branch {
                    self.show_warning_toast(
                        format!(
                            "Saved default branch {} is no longer available. Falling back to {}.",
                            invalid.branch_name, fallback_branch
                        ),
                        cx,
                    );
                } else {
                    self.show_warning_toast(
                        format!(
                            "Saved default branch {} is no longer available. Falling back to automatic branch detection.",
                            invalid.branch_name
                        ),
                        cx,
                    );
                }
            }
            ProjectBranchSettingField::DefaultTargetBranch => {
                self.show_warning_toast(
                    format!(
                        "Saved default target branch {} is no longer available. Choose a new target branch to use it for PRs.",
                        invalid.branch_name
                    ),
                    cx,
                );
            }
        }
    }

    fn handle_invalid_project_branch_settings(
        &mut self,
        project_id: &str,
        invalid_settings: Vec<InvalidProjectBranchSetting>,
        cx: &mut Context<Self>,
    ) -> bool {
        if invalid_settings.is_empty() {
            return false;
        }

        self.clear_branch_sidebar_states_for_project_group(project_id);
        let mut changed = false;
        for invalid in invalid_settings {
            self.show_invalid_project_branch_setting_toast(invalid, cx);
            changed = true;
        }
        changed
    }

    pub(crate) fn update_project_page_branch_setting(
        &mut self,
        project_id: &str,
        field: ProjectBranchSettingField,
        branch_name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let field_id = match field {
            ProjectBranchSettingField::DefaultBranch => "default-branch",
            ProjectBranchSettingField::DefaultTargetBranch => "default-target-branch",
        };
        let session = self.session_handle();
        let project_id_owned = project_id.to_string();
        let branch_clone = branch_name.clone();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetBranchSetting {
                project_id: project_id_owned,
                field: field_id.to_string(),
                branch_name: branch_clone,
            },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetBranchSetting failed: {err}");
                }
            },
        );
        // Dispatch is fire-and-forget; the daemon validates and
        // applies. UI feedback is optimistic — close the dropdown
        // and toast success. If the daemon rejects (e.g. unknown
        // branch name), the broadcast push leaves state unchanged
        // so the dropdown's selected value will revert on the next
        // re-render. Validation errors that used to surface
        // synchronously now surface as "no visible state change" —
        // good enough for now; a typed reply ack would be cleaner.
        self.project_page_config_dropdown = None;
        self.project_page_config_panel_targeted = false;
        self.clear_branch_sidebar_states_for_project_group(project_id);
        self.mark_git_refresh_stale();
        match (field, branch_name.as_deref()) {
            (ProjectBranchSettingField::DefaultBranch, Some(branch_name)) => {
                self.show_success_toast(format!("Default branch set to {}.", branch_name), cx);
            }
            (ProjectBranchSettingField::DefaultBranch, None) => {
                self.show_success_toast(
                    "Default branch cleared. New tasks will use automatic branch detection.",
                    cx,
                );
            }
            (ProjectBranchSettingField::DefaultTargetBranch, Some(branch_name)) => {
                self.show_success_toast(
                    format!("Default target branch set to {}.", branch_name),
                    cx,
                );
            }
            (ProjectBranchSettingField::DefaultTargetBranch, None) => {
                self.show_success_toast("Default target branch cleared.", cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn set_right_sidebar_mode(
        &mut self,
        mode: RightSidebarMode,
        cx: &mut Context<Self>,
    ) {
        if mode == RightSidebarMode::Commits {
            if let Some(project_id) = self
                .workspace_pane
                .read(cx)
                .active_section
                .as_ref()
                .map(|section| section.project_id.clone())
            {
                self.commit_page_sizes
                    .entry(project_id)
                    .or_insert(RECENT_COMMITS_PAGE_SIZE);
            }
        }

        if self.right_sidebar_mode == mode {
            return;
        }

        self.right_sidebar_mode = mode;
        if mode == RightSidebarMode::Commits {
            self.mark_git_refresh_stale();
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn load_more_commits(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let next_limit = self
            .commit_page_size_for_project(project_id)
            .saturating_add(RECENT_COMMITS_PAGE_SIZE);
        self.commit_page_sizes
            .insert(project_id.to_string(), next_limit);
        self.mark_git_refresh_stale();
        cx.notify();
    }

    fn commit_file_changes_key(project_id: &str, commit_id: &str) -> String {
        format!("{project_id}:{commit_id}")
    }

    pub(crate) fn commit_row_expanded(&self, project_id: &str, commit_id: &str) -> bool {
        self.expanded_commit_rows
            .contains(&Self::commit_file_changes_key(project_id, commit_id))
    }

    pub(crate) fn commit_file_changes_state(
        &self,
        project_id: &str,
        commit_id: &str,
    ) -> Option<&CommitFileChangesState> {
        self.commit_file_changes_states
            .get(&Self::commit_file_changes_key(project_id, commit_id))
    }

    fn request_commit_file_changes(&mut self, project_id: &str, commit_id: &str) {
        let key = Self::commit_file_changes_key(project_id, commit_id);
        match self.commit_file_changes_states.get(&key) {
            Some(CommitFileChangesState::Loading) | Some(CommitFileChangesState::Loaded(_)) => {
                return
            }
            Some(CommitFileChangesState::Failed(_)) | None => {}
        }

        let Some(project_path) = self
            .project_store
            .project(project_id)
            .map(|project| project.path.clone())
        else {
            return;
        };

        self.commit_file_changes_states
            .insert(key, CommitFileChangesState::Loading);

        let tx = self.commit_file_changes_sender.clone();
        let project_id = project_id.to_string();
        let commit_id = commit_id.to_string();
        std::thread::spawn(move || {
            let result =
                crate::project_store::read_project_commit_file_changes(&project_path, &commit_id)
                    .map(|state| state.files);
            let _ = tx.send(CommitFileChangesReply {
                project_id,
                commit_id,
                result,
            });
        });
    }

    pub(crate) fn toggle_commit_row_expanded(
        &mut self,
        project_id: &str,
        commit_id: &str,
        cx: &mut Context<Self>,
    ) {
        let key = Self::commit_file_changes_key(project_id, commit_id);
        if !self.expanded_commit_rows.insert(key.clone()) {
            self.expanded_commit_rows.remove(&key);
        } else {
            self.request_commit_file_changes(project_id, commit_id);
        }
        cx.notify();
    }

    pub(crate) fn toggle_project_page_open_in_menu(
        &mut self,
        project_id: &str,
        cx: &mut Context<Self>,
    ) {
        if self.enabled_open_in_apps().is_empty() {
            self.open_settings_section(crate::settings_page::SettingsSection::OpenIn, cx);
            return;
        }

        if self.project_page_open_in_menu_project_id.as_deref() == Some(project_id) {
            self.project_page_open_in_menu_project_id = None;
        } else {
            self.project_page_open_in_menu_project_id = Some(project_id.to_string());
        }
        self.git_actions_menu_open = false;
        self.custom_actions_menu_open = false;

        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn set_open_in_app_enabled(
        &mut self,
        app: OpenInAppKind,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.project_store
            .set_open_in_app_enabled(app, enabled, &self.available_open_in_apps);
        self.project_page_open_in_menu_project_id = None;
        cx.notify();
    }

    pub(crate) fn set_sidebar_git_metadata_visible(
        &mut self,
        visible: bool,
        _cx: &mut Context<Self>,
    ) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetSidebarGitMetadataVisible { visible },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetSidebarGitMetadataVisible failed: {err}");
                }
            },
        );
    }

    pub(crate) fn set_agent_enabled(
        &mut self,
        agent_id: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        // Dispatch-only — broadcast push will install the new
        // enabled-set; the prewarm sync re-runs on every render
        // tick (after the projection lands) so it picks up the
        // change without needing a synchronous local mutation here.
        self.dispatch_set_agent_enabled(agent_id.to_string(), enabled);
        cx.notify();
    }

    pub(crate) fn set_default_agent(&mut self, agent_id: &str, _cx: &mut Context<Self>) {
        self.dispatch_set_default_agent(agent_id.to_string());
    }

    pub(crate) fn open_project_open_in_target_in_app(
        &mut self,
        project_id: &str,
        app: OpenInAppKind,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project_store.project(project_id) else {
            self.show_error_toast("Could not find the selected project.", cx);
            return;
        };

        let active_git_diff = self.workspace_pane.read(cx).active_git_diff.clone();
        let target_path =
            open_in_target_path_for_project(project_id, &project.path, active_git_diff.as_ref());
        self.dismiss_titlebar_dropdowns();
        if let Err(err) = open_path_in_app(&target_path, app) {
            self.show_error_toast(err, cx);
        } else {
            self.project_store
                .set_preferred_open_in_app(app, &self.available_open_in_apps);
            cx.notify();
        }
    }

    pub(crate) fn open_active_open_in_target_in_default_app(&mut self, cx: &mut Context<Self>) {
        let Some(project_id) = self.active_open_in_project_id(cx) else {
            return;
        };

        let Some(app) = self.preferred_open_in_app() else {
            self.open_settings_section(crate::settings_page::SettingsSection::OpenIn, cx);
            return;
        };

        self.open_project_open_in_target_in_app(&project_id, app, cx);
    }

    pub(crate) fn show_info_toast(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.show_toast(ToastKind::Info, message, cx);
    }

    pub(crate) fn show_toast(
        &mut self,
        kind: ToastKind,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let message = message.into();
        let now = Instant::now();
        self.push_toast(AppToast::new(
            self.next_toast_id,
            kind,
            message,
            now,
            now + Self::toast_lifetime(kind),
        ));

        cx.notify();
    }

    fn show_toast_with_copy_message(
        &mut self,
        kind: ToastKind,
        message: impl Into<SharedString>,
        copy_message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let now = Instant::now();
        let toast = AppToast::with_copy_message(
            self.next_toast_id,
            kind,
            message,
            copy_message,
            now,
            now + Self::toast_lifetime(kind),
        );
        self.push_toast(toast);

        cx.notify();
    }

    fn push_toast(&mut self, toast: AppToast) {
        self.next_toast_id += 1;
        self.toasts.push(toast);

        if self.toasts.len() > TOAST_STACK_LIMIT {
            let excess = self.toasts.len() - TOAST_STACK_LIMIT;
            self.toasts.drain(0..excess);
        }
    }

    fn toast_lifetime(kind: ToastKind) -> Duration {
        match kind {
            ToastKind::Error => TOAST_LIFETIME + TOAST_ERROR_EXTRA_LIFETIME,
            ToastKind::Success | ToastKind::Warning | ToastKind::Info => TOAST_LIFETIME,
        }
    }

    fn anyhow_error_details(error: &anyhow::Error) -> String {
        format!("{error:?}")
    }

    pub(crate) fn project_expand_progress(&self, project_id: &str) -> f32 {
        self.project_expand_animations
            .get(project_id)
            .map(|animation| animation.progress)
            .unwrap_or_else(|| {
                if self.expanded_projects.contains(project_id) {
                    1.0
                } else {
                    0.0
                }
            })
    }

    pub(crate) fn project_expand_target(&self, project_id: &str) -> bool {
        self.project_expand_animations
            .get(project_id)
            .map(|animation| animation.target_expanded)
            .unwrap_or_else(|| self.expanded_projects.contains(project_id))
    }

    pub(crate) fn toggle_project_expansion(
        &mut self,
        project_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let from = self.project_expand_progress(project_id);
        let target_expanded = !self.project_expand_target(project_id);
        let to = if target_expanded { 1.0 } else { 0.0 };
        let mut persisted_expanded_projects = self.expanded_projects.clone();
        if target_expanded {
            persisted_expanded_projects.insert(project_id.to_string());
        } else {
            persisted_expanded_projects.remove(project_id);
        }
        self.project_store
            .set_expanded_projects(&persisted_expanded_projects);

        if (from - to).abs() < 0.001 {
            if target_expanded {
                self.expanded_projects.insert(project_id.to_string());
            } else {
                self.expanded_projects.remove(project_id);
            }
            self.project_expand_animations.remove(project_id);
            cx.notify();
            return;
        }

        if target_expanded {
            self.expanded_projects.insert(project_id.to_string());
        }

        let project_id = project_id.to_string();
        let generation = self.next_project_expand_animation_id;
        self.next_project_expand_animation_id += 1;
        self.project_expand_animations.insert(
            project_id.clone(),
            SidebarProjectExpandAnimation {
                progress: from,
                target_expanded,
                generation,
            },
        );

        let handle = cx.entity().clone();
        window
            .spawn(cx, async move |async_cx| {
                let steps = ((PROJECT_EXPAND_ANIMATION_DURATION.as_secs_f32()
                    / PROJECT_EXPAND_ANIMATION_STEP.as_secs_f32())
                .ceil() as i32)
                    .max(1);

                for step in 0..=steps {
                    let t = step as f32 / steps as f32;
                    let eased = t * t * (3.0 - 2.0 * t);
                    let progress = from + (to - from) * eased;
                    let _ = handle.update(async_cx, |this, cx| {
                        let Some(animation) = this.project_expand_animations.get_mut(&project_id)
                        else {
                            return;
                        };
                        if animation.generation != generation {
                            return;
                        }
                        animation.progress = progress;
                        cx.notify();
                    });
                    async_cx
                        .background_executor()
                        .timer(PROJECT_EXPAND_ANIMATION_STEP)
                        .await;
                }

                let _ = handle.update(async_cx, |this, cx| {
                    let Some(animation) = this.project_expand_animations.get(&project_id) else {
                        return;
                    };
                    if animation.generation != generation {
                        return;
                    }

                    if target_expanded {
                        this.expanded_projects.insert(project_id.clone());
                    } else {
                        this.expanded_projects.remove(&project_id);
                    }
                    this.project_expand_animations.remove(&project_id);
                    cx.notify();
                });
            })
            .detach();

        cx.notify();
    }

    #[hotpath::measure]
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = ProjectStore::load();
        theme::set_terminal_theme(theme::resolve_theme(window, store.ui.theme_mode));
        let registry_state = Arc::new(Mutex::new(crate::daemon_host::RegistryState::new(
            store.clone(),
        )));
        // The bus is owned by the app (lock-free emit path) and
        // cloned into the daemon thread for the MCP orchestrator's
        // per-session subscriptions.
        //
        // Capacity is sized for `ClientEvent::Output`, which fires
        // per PTY chunk and dominates throughput. A chatty TUI at
        // ~1 MiB/s with 8 KiB chunks produces ~125 events/sec; at
        // 4096 capacity a slow consumer can fall ~30 s behind
        // before lagging — long enough that any reasonable consumer
        // catches up, but bounded so the buffer doesn't grow without
        // limit. Other events (TaskOpened/TabOpened/etc.) are
        // sub-1 Hz under normal use and trivially fit.
        //
        // A future per-tab Output channel would isolate one chatty
        // terminal from others, but that requires reshaping the
        // subscriber API. Tracked as a follow-up.
        let event_bus = tokio::sync::broadcast::channel(4096).0;
        // Subscribe the GUI's own client-event receiver before any
        // event can be emitted, so we don't miss bus traffic during
        // startup. Drained on each render tick by `drain_gui_events`
        // to surface peer-driven changes (MCP, mobile) as toasts.
        let gui_event_receiver = Some(event_bus.subscribe());
        // The embedded daemon-host is a *desktop* concern — it spins up
        // an iroh endpoint that mobile clients connect into. On Android
        // we want to be the client, not the host, so the daemon-host
        // doesn't start. (Pairing-as-client is wired separately; see
        // the QR-scan flow follow-up.) Skipping the spawn also avoids
        // the `HOME is unset` startup error since `daemon-host`'s
        // config-dir lookup uses `dirs::config_dir()`.
        // Build the daemon-host thread (desktop) or stand up a
        // `NoSession` placeholder (mobile, until QR pair lands an
        // `IrohSession`). The `session` field is the only thing
        // call sites read — they do not care which platform branch
        // produced it.
        #[cfg(not(target_os = "android"))]
        let (daemon_handle_rx, initial_session) = {
            let handles = crate::daemon_host::spawn(registry_state.clone(), event_bus.clone());
            (Some(handles.endpoint_rx), handles.session)
        };
        #[cfg(target_os = "android")]
        let (daemon_handle_rx, initial_session): (
            Option<mpsc::Receiver<anyhow::Result<daemon::EndpointHandle>>>,
            Arc<dyn daemon_transport::Session>,
        ) = (
            None,
            Arc::new(crate::session_host::NoSession::new(
                "mobile session not paired yet — scan the desktop's QR code",
            )),
        );
        // Spawn the session-events pump on the boot session BEFORE
        // moving it into the Mutex, so the pump's stream subscription
        // doesn't have to take the lock. PTY bytes (and any future
        // server pushes) flow through `session_events_rx`, drained on
        // the GPUI render tick by `drain_session_events`. Desktop's
        // in-memory pair will never push events here (no `AttachTab`
        // issued from desktop) — the channel just stays quiet. Mobile
        // gets a fresh pump spawned on the wrapped iroh session via
        // `replace_session` after the QR pair completes.
        let (session_events_tx, session_events_rx) =
            tokio::sync::mpsc::unbounded_channel::<daemon_transport::SessionEvent>();
        {
            let tx = session_events_tx.clone();
            crate::session_host::spawn_event_pump(initial_session.events(), move |event| {
                let _ = tx.send(event);
            });
        }
        let session_events_rx = Some(session_events_rx);
        let session = Arc::new(Mutex::new(initial_session));
        let left_sidebar_open = store.ui.left_sidebar_open;
        // Broadcast capacity is a per-channel ring buffer size. 64 is
        // well above the realistic backlog for these low-rate worker
        // replies (render tick drains every 16 ms) but leaves enough
        // headroom that a future daemon/mobile subscriber can briefly
        // pause without triggering `Lagged`.
        let (project_github_link_sender, project_github_link_receiver) = broadcast::channel(64);
        let (project_pull_request_sender, project_pull_request_receiver) = broadcast::channel(64);
        let (project_page_pull_requests_sender, project_page_pull_requests_receiver) =
            broadcast::channel(64);
        let (project_check_runs_sender, project_check_runs_receiver) = broadcast::channel(64);
        let (commit_file_changes_sender, commit_file_changes_receiver) = mpsc::channel();
        let (worktree_deletion_sender, worktree_deletion_receiver) = mpsc::channel();
        let (changed_files_git_mutation_sender, changed_files_git_mutation_receiver) =
            broadcast::channel(64);
        let (terminal_launch_sender, terminal_launch_receiver) =
            mpsc::sync_channel(TERMINAL_LAUNCH_QUEUE_CAP);
        let (warm_terminal_launch_sender, warm_terminal_launch_receiver) =
            mpsc::sync_channel(WARM_TERMINAL_LAUNCH_QUEUE_CAP);
        let expanded = if store.ui.expanded_repo_ids.is_empty() {
            let mut expanded = HashSet::new();
            if let Some(first) = store.projects.first() {
                expanded.insert(first.repo_id.clone());
            }
            expanded
        } else {
            store.ui.expanded_repo_ids.clone()
        };
        let mut section_states = store
            .terminal_sections
            .iter()
            .filter_map(|(section_key, persisted)| {
                let section_id = SectionId::from_store_key(section_key)?;
                let fallback_cwd = persisted.cwd.clone().or_else(|| {
                    store
                        .projects
                        .iter()
                        .find(|project| project.id == section_id.project_id)
                        .map(|project| project.path.clone())
                });
                Some((
                    section_id,
                    SectionState::from_persisted(persisted.clone(), fallback_cwd),
                ))
            })
            .collect::<HashMap<_, _>>();
        let initial_section = choose_initial_section(
            &store.projects,
            &section_states,
            store.ui.last_active_section_id.as_deref(),
        );
        if let Some(ref sid) = initial_section {
            let cwd = store
                .projects
                .iter()
                .find(|project| project.id == sid.project_id)
                .map(|project| project.path.clone());
            section_states
                .entry(sid.clone())
                .or_insert_with(|| SectionState::with_cwd(cwd));
        }
        let focus_handle = cx.focus_handle();
        let initial_sidebar_w = if left_sidebar_open {
            280.
        } else {
            SIDEBAR_COLLAPSED
        };
        let initial_right_w = 460.;
        let initial_font_size = 13.0;
        let app_entity = cx.weak_entity();
        let available_open_in_apps = detect_available_open_in_apps();
        let git_commit_generation_script = store.git_commit_generation_script().to_string();
        let git_pr_generation_script = store.git_pr_generation_script().to_string();
        let workspace_pane = cx.new(|_| {
            WorkspacePane::new(
                app_entity.clone(),
                focus_handle.clone(),
                initial_sidebar_w,
                initial_right_w,
                initial_font_size,
                initial_section.clone(),
                section_states,
            )
        });

        let app = Self {
            sidebar_w: initial_sidebar_w,
            sidebar_saved: 280.,
            right_w: initial_right_w,
            right_saved: initial_right_w,
            drag: None,
            animating: false,
            mobile_view: MobileView::Home,
            mobile_nav_stack: Vec::new(),
            project_store: store,
            project_github_links: HashMap::new(),
            expanded_projects: expanded,
            project_expand_animations: HashMap::new(),
            next_project_expand_animation_id: 1,
            project_menu_project: None,
            sidebar_task_menu: None,
            collapsed_change_sections: HashSet::new(),
            expanded_commit_rows: HashSet::new(),
            git_actions_menu_open: false,
            custom_actions_menu_open: false,
            last_used_custom_action_id: None,
            toasts: Vec::new(),
            next_toast_id: 1,
            toast_drag: None,
            copied_toast: None,
            pasted_image_preview: None,
            discard_confirm: None,
            project_remove_confirm: None,
            sidebar_task_delete_confirm: None,
            new_task_modal: None,
            sidebar_task_rename: None,
            workspace_pane,
            changed_files: HashMap::new(),
            changed_files_list_snapshots: HashMap::new(),
            focus_handle,
            titlebar_drag_pending: false,
            refresh_timer_started: false,
            active_git_actions: HashMap::new(),
            pending_changed_files_git_mutations: HashMap::new(),
            changed_files_git_mutation_sender,
            changed_files_git_mutation_receiver,
            git_diff_state: None,
            git_diff_receiver: None,
            git_refresh_operation: BroadcastOperation::default(),
            new_task_branch_refresh_operation: BroadcastOperation::default(),
            task_creation_receiver: None,
            branch_creation_receiver: None,
            pending_task_launch: None,
            project_add_receiver: None,
            worktree_deletion_sender,
            worktree_deletion_receiver,
            commit_file_changes_sender,
            commit_file_changes_receiver,
            project_github_link_sender,
            project_github_link_receiver,
            project_pull_requests: HashMap::new(),
            project_pull_request_sender,
            project_pull_request_receiver,
            project_page_pull_requests_sender,
            project_page_pull_requests_receiver,
            project_check_runs_sender,
            project_check_runs_receiver,
            terminal_launch_sender,
            terminal_launch_receiver,
            warm_terminal_launch_sender,
            warm_terminal_launch_receiver,
            live_terminal_runtimes: HashMap::new(),
            last_terminal_activity: Instant::now(),
            pending_post_launch_input: HashMap::new(),
            terminal_manager: another_one_core::terminal_manager::TerminalManager::new(),
            terminal_surface_snapshots: HashMap::new(),
            terminal_scroll_remainder_lines: HashMap::new(),
            terminal_selection: None,
            terminal_search: None,
            terminal_bell_at: HashMap::new(),
            client_focus: HashMap::new(),
            last_observed_gui_focus: Focus::None,
            pending_worktree_jobs: HashMap::new(),
            gui_event_receiver,
            event_bus,
            prewarmed_terminal_launches: HashMap::new(),
            prewarmed_terminal_processes: HashMap::new(),
            canceled_prewarmed_launch_ids: HashSet::new(),
            active_add_agent_warm_launch_id: None,
            active_new_task_warm_launch_id: None,
            next_prewarmed_launch_id: 1,
            project_github_link_requests: HashSet::new(),
            project_github_link_checked: HashSet::new(),
            project_pull_request_requests: HashSet::new(),
            project_page_pull_requests: HashMap::new(),
            project_page_pull_requests_loading: HashSet::new(),
            project_page_pull_requests_errors: HashMap::new(),
            project_pull_request_checked: HashSet::new(),
            project_pull_request_checked_at: HashMap::new(),
            project_check_runs_states: HashMap::new(),
            project_check_runs_requests: HashSet::new(),
            project_check_runs_checked_at: HashMap::new(),
            settings_open: false,
            settings_section: crate::settings_page::SettingsSection::General,
            mcp_registry: {
                let mut reg = another_one_core::mcp::registry::McpRegistry::load();
                // Re-register the daemon MCP entry on every
                // startup: preserves the user's per-harness
                // `enabled_for` set across app upgrades while
                // letting us refresh the generated command/env
                // if the shim binary moves or the socket path
                // convention changes. If we can't locate the
                // shim (current_exe failed, or the packaging
                // didn't bundle it), skip registration rather
                // than write a broken command into harness
                // configs.
                if let Some(shim_bin) = shim_binary_path() {
                    let socket_path = daemon::transport_mcp::default_socket_path();
                    let prior_transport = reg
                        .entries
                        .iter()
                        .find(|e| e.id == another_one_core::mcp::catalog::DAEMON_MCP_ID)
                        .map(|e| e.transport.clone());
                    let entry = another_one_core::mcp::catalog::daemon_catalog_entry(
                        &shim_bin,
                        &socket_path,
                    );
                    let new_transport = entry.transport.clone();
                    reg.ensure_builtin(entry);
                    // Transport-fingerprint sync: if the daemon
                    // entry had a prior transport and it changed
                    // (shim path moved across versions, socket
                    // convention changed), downstream harness
                    // config files still point at the old
                    // command. Rerun sync so they're re-written
                    // with the current transport. Errors are
                    // logged only — they surface again through
                    // the MCP page's toast path once the user
                    // visits it.
                    if let Some(prev) = prior_transport {
                        if prev != new_transport {
                            let _report = reg.sync_all();
                        }
                    }
                    if let Err(err) = reg.save() {
                        log::warn!("failed to persist MCP registry at startup: {err}");
                    }
                } else {
                    log::warn!(
                        "could not locate another-one-mcp-shim next to current exe; \
                         skipping daemon MCP catalog registration"
                    );
                }
                reg
            },
            mcp_last_sync_errors: std::collections::HashSet::new(),
            available_open_in_apps,
            project_page_open_in_menu_project_id: None,
            project_page_config_panel_expanded: true,
            project_page_config_panel_targeted: false,
            project_page_config_dropdown: None,
            shortcut_capture_action: None,
            settings_agent_input: SettingsAgentInputState::default(),
            settings_git_commit_script_input: SettingsGitActionScriptInputState {
                draft: git_commit_generation_script,
                ..Default::default()
            },
            settings_git_pr_script_input: SettingsGitActionScriptInputState {
                draft: git_pr_generation_script,
                ..Default::default()
            },
            settings_git_commit_script_layout: Vec::new(),
            settings_git_pr_script_layout: Vec::new(),
            settings_git_commit_script_drag_anchor: None,
            settings_git_pr_script_drag_anchor: None,
            settings_git_action_llm_dropdown: None,
            right_sidebar_mode: RightSidebarMode::WorkingTree,
            commit_page_sizes: HashMap::new(),
            branch_commit_states: HashMap::new(),
            commit_file_changes_states: HashMap::new(),
            marked_text: None,
            add_agent_modal: None,
            create_branch_modal: None,
            custom_action_modal: None,
            sidebar_task_last_click: None,
            font_size: initial_font_size,
            last_viewport_size: window.viewport_size(),
            git_workspace: GitWorkspace::new_stale(
                Instant::now(),
                ACTIVE_GIT_STATUS_REFRESH_INTERVAL,
                ACTIVE_GIT_METADATA_REFRESH_INTERVAL,
            ),
            resource_indicator_open: false,
            pair_mobile_modal_open: false,
            pair_mobile_reset_pending: false,
            registry_state,
            daemon_handle_rx,
            daemon_handle: None,
            session,
            session_events_tx,
            session_events_rx,
            resource_collapsed_nodes: HashSet::new(),
            resource_usage: ResourceUsageSnapshot::default(),
            resource_usage_sampler: ResourceUsageSampler::default(),
            last_resource_usage_refresh: Instant::now() - RESOURCE_REFRESH_INTERVAL_CLOSED,
            updater: crate::updater::UpdaterHandle::spawn(crate::updater::BuildIdentity::current()),
            updater_state: crate::updater::UpdateState::Idle,
        };

        let mut app = app;
        app.refresh_resource_usage();

        cx.observe_window_bounds(window, |this, window, cx| {
            let viewport_size = window.viewport_size();
            if this.last_viewport_size == viewport_size {
                return;
            }

            this.last_viewport_size = viewport_size;
            this.clamp_layout(window);
            this.sync_workspace_layout(cx);
            this.ensure_active_terminal_runtime(window, cx);
            cx.notify();
        })
        .detach();

        app
    }

    fn resource_session_key(key: &TerminalRuntimeKey) -> String {
        format!("session:{}:{}", key.section_id.store_key(), key.tab_id)
    }

    fn resource_session_icon_path(launch_config: &TerminalLaunchConfig) -> &'static str {
        launch_config
            .provider
            .and_then(|provider| {
                AGENTS
                    .iter()
                    .find(|agent| agent.provider == Some(provider))
                    .map(|agent| agent.icon)
            })
            .unwrap_or("assets/icons/icons__terminal.svg")
    }

    fn resource_group_for_key(&self, key: &TerminalRuntimeKey) -> (String, String, String, String) {
        if let Some(task_id) = key.section_id.task_id.as_deref() {
            if let Some(task) = self.project_store.task(task_id) {
                let project_id = task.root_project_id.clone();
                let project_label = self
                    .project_store
                    .project(&project_id)
                    .map(|project| project.name.clone())
                    .unwrap_or_else(|| project_id.clone());
                let task_label = if task.name.trim().is_empty() {
                    task.branch_name.clone()
                } else {
                    task.name.clone()
                };
                return (
                    format!("resource-project:{project_id}"),
                    project_label,
                    format!("resource-task:{}", task.id),
                    task_label,
                );
            }
        }

        let project_id = key.section_id.project_id.clone();
        let project = self.project_store.project(&project_id);
        let project_label = project
            .map(|project| project.name.clone())
            .unwrap_or_else(|| project_id.clone());
        let task_label = project
            .and_then(|project| project.worktree_name.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| key.section_id.branch_name.clone());

        (
            format!("resource-project:{project_id}"),
            project_label,
            format!("resource-task:{}", key.section_id.store_key()),
            task_label,
        )
    }

    fn tracked_process_for_tab(
        &self,
        key: &TerminalRuntimeKey,
        launch_config: &TerminalLaunchConfig,
        process_id: u32,
    ) -> TrackedProcess {
        let (project_key, project_label, task_key, task_label) = self.resource_group_for_key(key);
        TrackedProcess {
            pid: process_id,
            key: Self::resource_session_key(key),
            label: launch_config.default_title(),
            project_key,
            project_label,
            task_key,
            task_label,
            icon_path: Self::resource_session_icon_path(launch_config),
        }
    }

    fn tracked_process_for_prewarmed(
        &self,
        launch_config: &TerminalLaunchConfig,
        process_id: u32,
    ) -> TrackedProcess {
        TrackedProcess {
            pid: process_id,
            key: format!("resource-session:prewarmed:{process_id}"),
            label: launch_config.default_title(),
            project_key: "resource-project:prewarmed".to_string(),
            project_label: "Prewarmed Launches".to_string(),
            task_key: "resource-task:prewarmed".to_string(),
            task_label: "Pending".to_string(),
            icon_path: Self::resource_session_icon_path(launch_config),
        }
    }

    pub(crate) fn refresh_resource_usage(&mut self) -> bool {
        let tracked_processes = self
            .terminal_manager
            .processes
            .values()
            .cloned()
            .chain(self.prewarmed_terminal_processes.values().cloned())
            .collect::<Vec<_>>();
        let snapshot = self
            .resource_usage_sampler
            .sample(std::process::id(), &tracked_processes);
        let changed = self.resource_usage != snapshot;
        self.resource_usage = snapshot;
        self.last_resource_usage_refresh = Instant::now();
        changed
    }

    fn tick_resource_usage(&mut self) -> bool {
        let refresh_interval = if self.resource_indicator_open {
            RESOURCE_REFRESH_INTERVAL_OPEN
        } else {
            RESOURCE_REFRESH_INTERVAL_CLOSED
        };

        if self.last_resource_usage_refresh.elapsed() < refresh_interval {
            return false;
        }

        self.refresh_resource_usage()
    }

    fn set_last_active_section_key(&mut self, section_key: Option<String>) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetLastActiveSection {
                section_id: section_key,
            },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetLastActiveSection failed: {err}");
                }
            },
        );
    }

    fn persist_section_state(&mut self, section_id: &SectionId, persisted: PersistedSectionState) {
        let store_key = section_id.store_key();
        let Ok(value) = serde_json::to_value(&persisted) else {
            log::warn!("persist_section_state: failed to serialise PersistedSectionState");
            return;
        };
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::PersistSectionState {
                section_id: store_key,
                persisted: value,
            },
            |result| {
                if let Err(err) = result {
                    log::warn!("PersistSectionState failed: {err}");
                }
            },
        );
    }

    fn update_terminal_tab(
        &mut self,
        key: &TerminalRuntimeKey,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut TerminalTab),
    ) -> bool {
        let section_id = key.section_id.clone();
        let tab_id = key.tab_id.clone();
        let mut update = Some(update);

        self.workspace_pane.update(cx, |workspace, cx| {
            let Some(tab) = workspace
                .section_states
                .get_mut(&section_id)
                .and_then(|state| state.tabs.iter_mut().find(|tab| tab.id == tab_id))
            else {
                return false;
            };

            if let Some(update) = update.take() {
                update(tab);
            }

            workspace.persist_section_state(&section_id, cx);
            cx.notify();
            true
        })
    }

    fn terminal_request_for_key(
        &self,
        key: &TerminalRuntimeKey,
        cx: &App,
    ) -> Option<TerminalRuntimeRequest> {
        let workspace = self.workspace_pane.read(cx);
        let state = workspace.section_states.get(&key.section_id)?;
        let tab = state.tabs.iter().find(|tab| tab.id == key.tab_id)?;
        let cwd = state.cwd.clone().or_else(|| {
            self.project_store
                .project(&key.section_id.project_id)
                .map(|project| project.path.clone())
        })?;

        Some(TerminalRuntimeRequest {
            key: key.clone(),
            cwd,
            launch_config: tab.launch_config.clone(),
            restore_status: tab.restore_status,
            agent_launch_args: self.agent_launch_args_for_launch_config(&tab.launch_config),
            size: TerminalGridSize::default(),
        })
    }

    fn append_terminal_recent_output(&mut self, key: &TerminalRuntimeKey, bytes: &[u8]) {
        self.terminal_manager.append_recent_output(key, bytes);
    }

    fn clear_terminal_recent_output(&mut self, key: &TerminalRuntimeKey) {
        self.terminal_manager.clear_recent_output(key);
    }

    fn terminal_failure_details(status: &str, recent_output: Option<&str>) -> String {
        let Some(recent_output) = recent_output
            .map(str::trim)
            .filter(|output| !output.is_empty())
        else {
            return status.to_string();
        };

        format!("{status}\n\nRecent terminal output:\n{recent_output}")
    }

    fn maybe_retry_claude_restore(
        &mut self,
        key: &TerminalRuntimeKey,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.terminal_manager.pending_launches.contains(key) {
            return false;
        }

        let Some(request) = self.terminal_request_for_key(key, cx) else {
            return false;
        };
        let Some(provider) = request.launch_config.provider else {
            return false;
        };
        if request.launch_config.session.is_none() {
            return false;
        }

        let recent_output = self
            .terminal_manager
            .recent_output
            .get(key)
            .map(String::as_str)
            .unwrap_or_default();
        if !agent_output_indicates_missing_session(provider, recent_output) {
            return false;
        }

        let launch_config = request.launch_config.with_session(None);
        self.terminal_manager.mark_launch_started(key.clone());
        self.update_terminal_tab(key, cx, |tab| {
            tab.launch_config = launch_config.clone();
            tab.restore_status = TerminalRestoreStatus::Launching;
            tab.title = launch_config.default_title();
        });
        spawn_terminal_launch(
            self.terminal_launch_sender.clone(),
            key.clone(),
            Some(request.cwd),
            launch_config,
            request.agent_launch_args,
            request.size,
        );
        true
    }

    fn active_terminal_request(&self, window: &Window, cx: &App) -> Option<TerminalRuntimeRequest> {
        let workspace = self.workspace_pane.read(cx);
        let section_id = workspace.active_section.clone()?;
        let state = workspace.section_states.get(&section_id)?;
        let tab = state.tabs.get(state.active_tab)?;
        let cwd = state.cwd.clone().or_else(|| {
            self.project_store
                .project(&section_id.project_id)
                .map(|project| project.path.clone())
        })?;

        Some(TerminalRuntimeRequest {
            key: TerminalRuntimeKey {
                section_id,
                tab_id: tab.id.clone(),
            },
            cwd,
            launch_config: tab.launch_config.clone(),
            restore_status: tab.restore_status,
            agent_launch_args: self.agent_launch_args_for_launch_config(&tab.launch_config),
            size: self.terminal_panel_size(window),
        })
    }

    fn terminal_panel_size(&self, window: &Window) -> TerminalGridSize {
        let viewport = window.viewport_size();
        let titlebar_height = if crate::platform::CurrentPlatform::supports_custom_chrome(window) {
            TITLEBAR_CHROME_H
        } else {
            0.0
        };
        // Narrow (mobile) layouts give the terminal the entire viewport
        // width minus padding — no sidebars to subtract. On rotate, the
        // viewport flips dimensions and this recomputes against the
        // new bounds, which then drives a `master.resize(...)` call in
        // `drain_pending_tab_resizes` (TIOCSWINSZ → SIGWINCH).
        let (width, height) = if self.is_narrow(window) {
            let width = (f32::from(viewport.width) - TERMINAL_VIEW_PADDING * 2.0).max(MIN_MAIN);
            let height = (f32::from(viewport.height)
                - titlebar_height
                - TERMINAL_TAB_BAR_H
                - TERMINAL_VIEW_PADDING * 2.0)
                .max(120.0);
            (width, height)
        } else {
            let width = (f32::from(viewport.width)
                - self.sidebar_w
                - self.right_w
                - GUTTER * 2.0
                - TERMINAL_VIEW_PADDING * 2.0)
                .max(MIN_MAIN);
            let height = (f32::from(viewport.height)
                - FOOTER_H
                - titlebar_height
                - TERMINAL_TAB_BAR_H
                - MAIN_PANE_BOTTOM_PAD
                - TERMINAL_VIEW_PADDING * 2.0)
                .max(120.0);
            (width, height)
        };
        TerminalGridSize::from_panel_size(width, height, self.font_size)
    }

    fn cwd_for_section(&self, section_id: &SectionId, cx: &App) -> Option<std::path::PathBuf> {
        self.workspace_pane
            .read(cx)
            .section_states
            .get(section_id)
            .and_then(|state| state.cwd.clone())
            .or_else(|| {
                self.project_store
                    .project(&section_id.project_id)
                    .map(|project| project.path.clone())
            })
    }

    fn start_prewarmed_terminal_launch(
        &mut self,
        cwd: std::path::PathBuf,
        launch_config: TerminalLaunchConfig,
    ) -> u64 {
        let launch_id = self.next_prewarmed_launch_id;
        self.next_prewarmed_launch_id += 1;
        self.prewarmed_terminal_launches.insert(
            launch_id,
            PrewarmedTerminalLaunch {
                cwd: cwd.clone(),
                launch_config: launch_config.clone(),
                attached_tab: None,
                runtime: None,
            },
        );
        spawn_warm_terminal_launch(
            self.warm_terminal_launch_sender.clone(),
            launch_id,
            Some(cwd),
            launch_config.clone(),
            self.agent_launch_args_for_launch_config(&launch_config),
            TerminalGridSize::default(),
        );
        launch_id
    }

    fn attach_or_start_prewarmed_terminal(
        &mut self,
        launch_id: Option<u64>,
        key: TerminalRuntimeKey,
        cwd: std::path::PathBuf,
        launch_config: TerminalLaunchConfig,
        cx: &mut Context<Self>,
    ) {
        if let Some(launch_id) = launch_id {
            if self.attach_prewarmed_launch_to_tab(launch_id, key.clone(), cx) {
                return;
            }
            self.cancel_prewarmed_launch(launch_id);
        }

        let launch_id = self.start_prewarmed_terminal_launch(cwd, launch_config);
        if !self.attach_prewarmed_launch_to_tab(launch_id, key, cx) {
            self.cancel_prewarmed_launch(launch_id);
        }
    }

    fn new_task_modal_prewarm_request(
        &mut self,
        _cx: &App,
    ) -> Option<(std::path::PathBuf, TerminalLaunchConfig)> {
        self.sanitize_new_task_modal_selected_agents();

        let state = self.new_task_modal.as_ref()?;
        if state.submitting || state.worktree_mode {
            return None;
        }

        let project_path = self
            .project_store
            .project(&state.project_id)
            .map(|project| project.path.clone())?;

        Some((
            project_path,
            terminal_launch_config_for_selected_agents(&state.selected_agents),
        ))
    }

    pub(crate) fn sync_add_agent_modal_prewarm(&mut self, cx: &mut Context<Self>) {
        self.sanitize_add_agent_modal_selection();
        let Some(state) = self.add_agent_modal.as_ref() else {
            return;
        };
        let section_id = state.section_id.clone();
        let selected_agent_id = state.selected_agent_id.clone();
        let Some(launch_config) =
            terminal_launch_config_for_selected_agent(selected_agent_id.as_deref())
        else {
            self.cancel_active_add_agent_prewarm();
            return;
        };
        let Some(cwd) = self.cwd_for_section(&section_id, cx) else {
            self.cancel_active_add_agent_prewarm();
            return;
        };

        if let Some(launch_id) = self.active_add_agent_warm_launch_id {
            if let Some(existing) = self.prewarmed_terminal_launches.get(&launch_id) {
                if existing.cwd == cwd && existing.launch_config == launch_config {
                    return;
                }
            }
        }

        self.cancel_active_add_agent_prewarm();
        self.active_add_agent_warm_launch_id =
            Some(self.start_prewarmed_terminal_launch(cwd, launch_config));
    }

    pub(crate) fn cancel_active_add_agent_prewarm(&mut self) {
        if let Some(launch_id) = self.active_add_agent_warm_launch_id.take() {
            self.cancel_prewarmed_launch(launch_id);
        }
    }

    pub(crate) fn sync_new_task_modal_prewarm(&mut self, cx: &mut Context<Self>) {
        let Some((cwd, launch_config)) = self.new_task_modal_prewarm_request(cx) else {
            self.cancel_active_new_task_prewarm();
            return;
        };

        if let Some(launch_id) = self.active_new_task_warm_launch_id {
            if let Some(existing) = self.prewarmed_terminal_launches.get(&launch_id) {
                if existing.cwd == cwd && existing.launch_config == launch_config {
                    return;
                }
            }
        }

        self.cancel_active_new_task_prewarm();
        self.active_new_task_warm_launch_id =
            Some(self.start_prewarmed_terminal_launch(cwd, launch_config));
    }

    pub(crate) fn cancel_active_new_task_prewarm(&mut self) {
        if let Some(launch_id) = self.active_new_task_warm_launch_id.take() {
            self.cancel_prewarmed_launch(launch_id);
        }
    }

    pub(crate) fn cancel_prewarmed_launch(&mut self, launch_id: u64) {
        let Some(launch) = self.prewarmed_terminal_launches.remove(&launch_id) else {
            return;
        };

        self.prewarmed_terminal_processes.remove(&launch_id);
        if let Some(key) = launch.attached_tab {
            self.terminal_manager.pending_launches.remove(&key);
            self.terminal_manager.processes.remove(&key);
        }
        if let Some(mut runtime) = launch.runtime {
            runtime.kill();
        }
        self.canceled_prewarmed_launch_ids.insert(launch_id);
    }

    fn cancel_prewarmed_launch_for_tab(&mut self, key: &TerminalRuntimeKey) {
        let launch_id = self
            .prewarmed_terminal_launches
            .iter()
            .find_map(|(launch_id, launch)| {
                (launch.attached_tab.as_ref() == Some(key)).then_some(*launch_id)
            });
        if let Some(launch_id) = launch_id {
            if self.active_add_agent_warm_launch_id == Some(launch_id) {
                self.active_add_agent_warm_launch_id = None;
            }
            self.cancel_prewarmed_launch(launch_id);
        }
    }

    pub(crate) fn attach_prewarmed_launch_to_tab(
        &mut self,
        launch_id: u64,
        key: TerminalRuntimeKey,
        cx: &mut Context<Self>,
    ) -> bool {
        let (project_key, project_label, task_key, task_label) = self.resource_group_for_key(&key);
        let Some(launch) = self.prewarmed_terminal_launches.get_mut(&launch_id) else {
            return false;
        };

        launch.attached_tab = Some(key.clone());
        if let Some(process) = self.prewarmed_terminal_processes.remove(&launch_id) {
            self.terminal_manager.processes.insert(
                key.clone(),
                TrackedProcess {
                    pid: process.pid,
                    key: Self::resource_session_key(&key),
                    label: launch.launch_config.default_title(),
                    project_key,
                    project_label,
                    task_key,
                    task_label,
                    icon_path: Self::resource_session_icon_path(&launch.launch_config),
                },
            );
        }

        let taken_runtime = launch.runtime.take();
        let launch_config = launch.launch_config.clone();
        if let Some(mut runtime) = taken_runtime {
            self.terminal_manager.pending_launches.remove(&key);
            self.terminal_manager.errors.remove(&key);
            if let (Some(broadcast), Some(writer)) =
                (runtime.output_broadcast(), runtime.writer_handle())
            {
                self.register_tab_with_registry(&key, broadcast, writer);
            }
            self.terminal_surface_snapshots
                .insert(key.clone(), runtime.snapshot());
            self.live_terminal_runtimes.insert(key.clone(), runtime);
            self.send_pending_post_launch_input(&key);
            self.update_terminal_tab(&key, cx, |tab| {
                tab.launch_config = launch_config.clone();
                tab.restore_status = TerminalRestoreStatus::Ready;
            });
        } else {
            self.terminal_manager.pending_launches.insert(key.clone());
            self.update_terminal_tab(&key, cx, |tab| {
                tab.launch_config = launch_config.clone();
                tab.restore_status = TerminalRestoreStatus::Launching;
            });
        }

        true
    }

    fn ensure_active_terminal_runtime(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(request) = self.active_terminal_request(window, cx) else {
            return;
        };

        if self.live_terminal_runtimes.contains_key(&request.key) {
            if self.animating {
                return;
            }
            // Don't resize directly — announce the desktop's viewport
            // size to the registry and let
            // `drain_pending_tab_resizes` apply the effective min
            // across all current viewers. When a phone attaches at
            // a smaller viewport, the PTY + local grid both follow
            // the phone's size so lines wrap consistently.
            use crate::daemon_host::DESKTOP_LOCAL_VIEWER_ID;
            #[cfg(target_os = "android")]
            let mut focus_changed = false;
            if let Ok(mut state) = self.registry_state.lock() {
                // Switching focused tabs on desktop: drop the prior
                // tab's desktop-local entry (same semantics as a
                // mobile detach/reattach).
                if let Some(old_key) = state.viewer_focus.get(DESKTOP_LOCAL_VIEWER_ID).cloned() {
                    if old_key != request.key {
                        #[cfg(target_os = "android")]
                        {
                            focus_changed = true;
                        }
                        if let Some(map) = state.active_viewers.get_mut(&old_key) {
                            map.remove(DESKTOP_LOCAL_VIEWER_ID);
                            if map.is_empty() {
                                state.active_viewers.remove(&old_key);
                                state.effective_sizes.remove(&old_key);
                            }
                        }
                        state.recompute_effective_size(&old_key);
                    }
                } else {
                    #[cfg(target_os = "android")]
                    {
                        focus_changed = true;
                    }
                }
                state
                    .active_viewers
                    .entry(request.key.clone())
                    .or_default()
                    .insert(
                        DESKTOP_LOCAL_VIEWER_ID.to_string(),
                        (request.size.cols, request.size.rows),
                    );
                state
                    .viewer_focus
                    .insert(DESKTOP_LOCAL_VIEWER_ID.to_string(), request.key.clone());
                state.recompute_effective_size(&request.key);
            }
            #[cfg(target_os = "android")]
            if focus_changed {
                // Re-key the session attach to the focused tab so
                // the daemon's forwarder pushes bytes for *this* key
                // into our `SessionEvent::PtyBytes` stream.
                let session = self.session_handle();
                let section_id = request.key.section_id.store_key();
                let tab_id = request.key.tab_id.clone();
                crate::session_host::dispatch_fire_and_forget(
                    session,
                    daemon_proto::Control::AttachTab {
                        section_id,
                        tab_id,
                    },
                    |result| {
                        if let Err(err) = result {
                            log::warn!("focus-change AttachTab failed: {err}");
                        }
                    },
                );
            }
            return;
        }

        if self
            .terminal_manager
            .pending_launches
            .contains(&request.key)
        {
            return;
        }

        if request.restore_status == TerminalRestoreStatus::Failed {
            return;
        }

        self.terminal_manager
            .pending_launches
            .insert(request.key.clone());
        // Same in-flight guard as `drain_pending_tab_launches`:
        // populate the shared set so a mobile LaunchTab arriving
        // between this spawn + its Launched reply doesn't queue a
        // duplicate. Both paths (desktop click + mobile LaunchTab)
        // must write to the same set for the dedupe to hold.
        if let Ok(mut state) = self.registry_state.lock() {
            state.in_flight_launches.insert(request.key.clone());
        }
        self.update_terminal_tab(&request.key, cx, |tab| {
            tab.restore_status = TerminalRestoreStatus::Launching;
        });

        #[cfg(target_os = "android")]
        {
            // Mobile: don't spawn a phone-local PTY. Stand up a
            // viewer-only runtime that renders bytes pushed via
            // `SessionEvent::PtyBytes`, then ask the daemon to
            // launch + attach the real PTY on the desktop side.
            log::info!(
                "android: ensure_active_terminal_runtime → viewer-only \
                 section={} tab={} size={}x{}",
                request.key.section_id.store_key(),
                request.key.tab_id,
                request.size.cols,
                request.size.rows
            );
            let mut runtime = LiveTerminalRuntime::from_remote(request.size);
            self.terminal_surface_snapshots
                .insert(request.key.clone(), runtime.snapshot());
            self.live_terminal_runtimes
                .insert(request.key.clone(), runtime);
            self.update_terminal_tab(&request.key, cx, |tab| {
                tab.restore_status = TerminalRestoreStatus::Ready;
            });

            let session = self.session_handle();
            let section_id = request.key.section_id.store_key();
            let tab_id = request.key.tab_id.clone();
            crate::session_host::dispatch_fire_and_forget(
                session.clone(),
                daemon_proto::Control::LaunchTab {
                    section_id: section_id.clone(),
                    tab_id: tab_id.clone(),
                },
                |result| {
                    if let Err(err) = result {
                        log::warn!("session LaunchTab failed: {err}");
                    }
                },
            );
            // Daemon's handle_attach returns without installing a
            // forwarder if it arrives before the desktop's render
            // tick has spawned the PTY (the registry says "no live
            // runtime yet"). Re-fire AttachTab a few times with
            // backoff so the forwarder lands once the broadcast is
            // registered. The daemon ack's each AttachTab with
            // WorkerReply::Empty so .await resolves cleanly.
            crate::session_host::runtime_handle().spawn(async move {
                for (i, delay_ms) in [0_u64, 200, 500, 1000, 2000].iter().enumerate() {
                    if *delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(*delay_ms)).await;
                    }
                    log::info!(
                        "AttachTab attempt {} (delay={}ms) section={} tab={}",
                        i + 1,
                        delay_ms,
                        section_id,
                        tab_id
                    );
                    match session
                        .call(daemon_proto::Control::AttachTab {
                            section_id: section_id.clone(),
                            tab_id: tab_id.clone(),
                        })
                        .await
                    {
                        Ok(reply) => log::info!(
                            "AttachTab attempt {} ack: {:?}",
                            i + 1,
                            std::mem::discriminant(&reply)
                        ),
                        Err(err) => {
                            log::warn!("session AttachTab failed: {err}");
                            return;
                        }
                    }
                }
            });
            return;
        }

        #[cfg(not(target_os = "android"))]
        spawn_terminal_launch(
            self.terminal_launch_sender.clone(),
            request.key,
            Some(request.cwd),
            request.launch_config,
            request.agent_launch_args,
            request.size,
        );
    }

    /// Drain any session handoff queued by the QR-pair dial task and
    /// install it as the active `Session`. `replace_session`
    /// re-spawns the events pump so PTY bytes from the new session
    /// land in `session_events_rx`. No-op when the queue is empty
    /// (every render tick on desktop, every tick on mobile after
    /// the post-pair handoff has already happened).
    pub(crate) fn drain_pending_session_handoff(&mut self) -> bool {
        if let Some(session) = crate::iroh_client::take_pending_session() {
            log::info!("installing freshly-paired session via replace_session");
            self.replace_session(session);
            true
        } else {
            false
        }
    }

    /// Drain `SessionEvent`s the session pump pushed onto
    /// `session_events_rx`. Mobile gets PTY bytes here once paired;
    /// desktop never issues `AttachTab` over its in-memory pair so
    /// this drain stays quiet on desktop.
    ///
    /// Mirrors the work the `TerminalLaunchReply::Output` arm does
    /// (apply bytes to the live VT, mark the tab for snapshot
    /// rebuild, mirror via `ClientEvent::Output`, retry the
    /// claude-restore probe when the runtime hasn't materialised
    /// yet) but keyed by the wire `(section_id, tab_id)` strings the
    /// session emits rather than a local `TerminalRuntimeKey`.
    fn drain_session_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut updated = false;
        let mut output_dirty_keys: HashSet<TerminalRuntimeKey> = HashSet::new();
        loop {
            let event = match self.session_events_rx.as_mut() {
                Some(rx) => match rx.try_recv() {
                    Ok(event) => event,
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        // Should not occur in practice — `session_events_tx`
                        // is held as a struct field for the app's
                        // lifetime, so the channel can't fully close.
                        log::warn!("session events channel unexpectedly disconnected");
                        break;
                    }
                },
                None => break,
            };
            match event {
                daemon_transport::SessionEvent::PtyBytes {
                    section_id,
                    tab_id,
                    bytes,
                } => {
                    log::info!(
                        "drain_session_events PtyBytes section={} tab={} bytes={}",
                        section_id,
                        tab_id,
                        bytes.len()
                    );
                    let Some(section_id) = SectionId::from_store_key(&section_id) else {
                        log::warn!("session event PtyBytes with malformed section_id");
                        continue;
                    };
                    let key = TerminalRuntimeKey { section_id, tab_id };
                    crate::leakscope::note_drain_output(bytes.len());
                    self.append_terminal_recent_output(&key, &bytes);
                    if let Some(runtime) = self.live_terminal_runtimes.get_mut(&key) {
                        let terminal_update = runtime.apply_output(&bytes);
                        output_dirty_keys.insert(key.clone());
                        if terminal_update.bell {
                            self.terminal_bell_at.insert(key.clone(), Instant::now());
                        }
                        self.update_terminal_tab(&key, cx, |tab| {
                            apply_terminal_title_update(tab, &terminal_update);
                        });
                        self.emit_client_event(ClientEvent::Output {
                            tab_id: key.tab_id.clone(),
                            bytes: bytes.clone(),
                        });
                        updated = true;
                    } else if self.maybe_retry_claude_restore(&key, cx) {
                        updated = true;
                    }
                }
                daemon_transport::SessionEvent::Push(reply) => {
                    // Daemon broadcasts the registry's projection
                    // through `serve_session_with_attach`'s push
                    // pump on every state change (and once at
                    // session connect). Both clients absorb here —
                    // same path mobile uses for the legacy iroh
                    // worker-reply queue, just sourced from the
                    // events stream instead.
                    match reply {
                        daemon_proto::WorkerReply::ProjectList {
                            projects,
                            repos,
                            ui,
                        } => {
                            log::info!(
                                "drain_session_events: absorbed projection ({} projects, {} repos, ui pinned={} expanded={})",
                                projects.len(),
                                repos.len(),
                                ui.pinned_task_ids.len(),
                                ui.expanded_repo_ids.len(),
                            );
                            self.project_store.absorb_projection(projects, repos, ui);
                            for project in &self.project_store.projects {
                                if self
                                    .project_store
                                    .tasks
                                    .get(&project.id)
                                    .is_some_and(|tasks| !tasks.is_empty())
                                {
                                    self.expanded_projects.insert(project.repo_id.clone());
                                }
                            }
                            updated = true;
                        }
                        other => {
                            log::debug!(
                                "session pushed unsolicited reply: {:?}",
                                std::mem::discriminant(&other)
                            );
                        }
                    }
                }
                daemon_transport::SessionEvent::Lagged { skipped } => {
                    log::warn!("session events lagged — skipped {skipped} events");
                }
                daemon_transport::SessionEvent::Closed { reason } => {
                    log::info!("session events stream closed (reason: {reason:?})");
                    // Don't drop the receiver — the next
                    // `replace_session` will spawn a fresh pump.
                    break;
                }
            }
        }
        if !output_dirty_keys.is_empty() {
            for key in output_dirty_keys {
                if let Some(runtime) = self.live_terminal_runtimes.get_mut(&key) {
                    let snap = runtime.snapshot();
                    self.terminal_surface_snapshots.insert(key.clone(), snap);
                }
            }
            updated = true;
        }
        updated
    }

    fn drain_terminal_launch_replies(&mut self, cx: &mut Context<Self>) -> bool {
        // RAII guard: increments drain count, times the body, and
        // bumps the watchdog heartbeat on drop. See issue #125 —
        // this is the signal that distinguishes drain-starvation
        // from a true deadlock when the GUI appears frozen.
        let _drain_guard = crate::leakscope::drain_tick_guard();
        let mut updated = false;
        // Tracks tabs that accumulated VT output during this drain tick. We
        // used to rebuild + clone each tab's surface snapshot on *every*
        // `Output` reply, so a burst of 10 chunks paid the full-grid
        // rebuild cost 10 times before GPUI even had a chance to repaint.
        // Defer the rebuild to once per tab per tick: collect the keys
        // that got output, drain the whole queue, then rebuild snapshots
        // once at the end. At 60 Hz drain frequency this caps snapshot
        // work at 60 rebuilds/sec even under sustained output storms.
        let mut output_dirty_keys: HashSet<TerminalRuntimeKey> = HashSet::new();
        // Bounded per-tick output budget. See
        // [`DRAIN_OUTPUT_BYTE_CAP`] for the why; once we've parsed
        // this many `Output` bytes we break out of the greedy
        // `try_recv` loop and let GPUI repaint before the next
        // tick picks up the remainder.
        let mut drained_output_bytes: usize = 0;

        loop {
            match self.terminal_launch_receiver.try_recv() {
                Ok(TerminalLaunchReply::Launched {
                    key,
                    runtime,
                    launch_config,
                    process_id,
                }) => {
                    let process = process_id.map(|process_id| {
                        self.tracked_process_for_tab(&key, &launch_config, process_id)
                    });
                    self.terminal_manager
                        .mark_launch_succeeded(key.clone(), process);
                    self.clear_terminal_recent_output(&key);

                    let mut runtime = LiveTerminalRuntime::from_prepared(runtime);
                    // Tee the PTY into the embedded daemon's registry
                    // so a mobile `AttachTab` subscriber sees the same
                    // bytes the desktop renders.
                    if let (Some(broadcast), Some(writer)) =
                        (runtime.output_broadcast(), runtime.writer_handle())
                    {
                        self.register_tab_with_registry(&key, broadcast, writer);
                    }
                    self.terminal_surface_snapshots
                        .insert(key.clone(), runtime.snapshot());
                    self.live_terminal_runtimes.insert(key.clone(), runtime);
                    self.send_pending_post_launch_input(&key);
                    self.update_terminal_tab(&key, cx, |tab| {
                        tab.launch_config = launch_config.clone();
                        tab.restore_status = TerminalRestoreStatus::Ready;
                    });
                    updated = true;
                }
                Ok(TerminalLaunchReply::Output { key, bytes }) => {
                    crate::leakscope::note_drain_output(bytes.len());
                    drained_output_bytes = drained_output_bytes.saturating_add(bytes.len());
                    self.append_terminal_recent_output(&key, &bytes);
                    if let Some(runtime) = self.live_terminal_runtimes.get_mut(&key) {
                        let terminal_update = runtime.apply_output(&bytes);
                        output_dirty_keys.insert(key.clone());
                        if terminal_update.bell {
                            self.terminal_bell_at.insert(key.clone(), Instant::now());
                        }
                        self.update_terminal_tab(&key, cx, |tab| {
                            apply_terminal_title_update(tab, &terminal_update);
                        });
                        // Mirror the chunk to MCP subscribers. Each
                        // session has its own broadcast::Receiver so
                        // a slow consumer doesn't stall the daemon's
                        // own output processing. `maybe_emit_tab_output`
                        // short-circuits when no one's listening — the
                        // common case — to avoid a per-chunk clone on
                        // the GPUI thread. See #127.
                        self.maybe_emit_tab_output(&key.tab_id, &bytes);
                        updated = true;
                    } else if self.maybe_retry_claude_restore(&key, cx) {
                        updated = true;
                    }
                    if drained_output_bytes >= DRAIN_OUTPUT_BYTE_CAP {
                        // Yield back to the GPUI run loop. Remaining
                        // replies stay in the bounded channel and
                        // will be picked up on the next drain tick.
                        break;
                    }
                }
                Ok(TerminalLaunchReply::SessionDiscovered { key, session }) => {
                    let section_id = key.section_id.clone();
                    let applied = self.workspace_pane.update(cx, |workspace, cx| {
                        if !apply_terminal_session_backfill(
                            &mut workspace.section_states,
                            &key,
                            session.clone(),
                        ) {
                            return false;
                        }
                        workspace.persist_section_state(&section_id, cx);
                        cx.notify();
                        true
                    });
                    updated |= applied;
                }
                Ok(TerminalLaunchReply::Exited { key, status }) => {
                    if self.maybe_retry_claude_restore(&key, cx) {
                        self.terminal_manager.processes.remove(&key);
                        self.live_terminal_runtimes.remove(&key);
                        self.terminal_surface_snapshots.remove(&key);
                        self.unregister_tab_from_registry(&key);
                        updated = true;
                        continue;
                    }
                    self.terminal_surface_snapshots.remove(&key);
                    let details = Self::terminal_failure_details(
                        &status,
                        self.terminal_manager
                            .recent_output
                            .get(&key)
                            .map(String::as_str),
                    );
                    self.terminal_manager
                        .mark_launch_failed(key.clone(), details);
                    self.clear_terminal_recent_output(&key);
                    self.live_terminal_runtimes.remove(&key);
                    self.unregister_tab_from_registry(&key);
                    self.update_terminal_tab(&key, cx, |tab| {
                        tab.restore_status = TerminalRestoreStatus::Failed;
                    });
                    updated = true;
                }
                Ok(TerminalLaunchReply::Failed {
                    key,
                    message,
                    details,
                }) => {
                    self.terminal_manager
                        .mark_launch_failed(key.clone(), details.clone());
                    self.live_terminal_runtimes.remove(&key);
                    self.terminal_surface_snapshots.remove(&key);
                    self.unregister_tab_from_registry(&key);
                    self.clear_terminal_recent_output(&key);
                    self.update_terminal_tab(&key, cx, |tab| {
                        tab.restore_status = TerminalRestoreStatus::Failed;
                    });
                    self.show_error_details_toast(message, details, cx);
                    updated = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        // Coalesced per-tab snapshot rebuild (see comment at the top of
        // this method). On top of coalescing, skip rebuilds for
        // non-focused tabs entirely — backgrounded tabs still accumulate
        // VT parser state via `apply_output`, but their GPUI-facing
        // snapshot isn't being painted, so there's no need to pay the
        // grid-rebuild cost until the user brings them to the front.
        //
        // The catch-up step below covers "tab just became focused": its
        // dirty flag is still set from the backgrounded period, so we
        // rebuild once on the first drain tick after the switch.
        let focused_key = self.active_terminal_key(cx);
        for key in output_dirty_keys {
            if focused_key.as_ref() != Some(&key) {
                continue;
            }
            if let Some(runtime) = self.live_terminal_runtimes.get_mut(&key) {
                let snapshot = runtime.snapshot();
                self.terminal_surface_snapshots.insert(key, snapshot);
            }
        }
        if let Some(key) = focused_key {
            if let Some(runtime) = self.live_terminal_runtimes.get_mut(&key) {
                if runtime.is_dirty() {
                    let snapshot = runtime.snapshot();
                    self.terminal_surface_snapshots.insert(key, snapshot);
                    updated = true;
                }
            }
        }

        crate::leakscope::set_live_counts(
            self.live_terminal_runtimes.len(),
            self.terminal_surface_snapshots.len(),
        );

        if updated {
            self.last_terminal_activity = Instant::now();
        }

        updated
    }

    fn drain_warm_terminal_launch_replies(&mut self, cx: &mut Context<Self>) -> bool {
        // Same guard as the hot drain above — warm-launch traffic
        // lands in its own bounded channel but shares the GPUI main
        // thread, so it contributes to lockup diagnostics too.
        let _drain_guard = crate::leakscope::drain_tick_guard();
        let mut updated = false;
        // Same byte budget as the hot drain — see
        // [`DRAIN_OUTPUT_BYTE_CAP`]. Warm and hot drains both run
        // on the GPUI main thread so they share the same frame-time
        // budget regardless of which channel a chunk arrived on.
        let mut drained_output_bytes: usize = 0;

        loop {
            match self.warm_terminal_launch_receiver.try_recv() {
                Ok(WarmTerminalLaunchReply::Launched {
                    launch_id,
                    runtime,
                    launch_config,
                    process_id,
                }) => {
                    if self.canceled_prewarmed_launch_ids.contains(&launch_id) {
                        let mut runtime = LiveTerminalRuntime::from_prepared(runtime);
                        runtime.kill();
                        continue;
                    }

                    let mut runtime = LiveTerminalRuntime::from_prepared(runtime);
                    let attached_key = {
                        let Some(launch) = self.prewarmed_terminal_launches.get_mut(&launch_id)
                        else {
                            runtime.kill();
                            continue;
                        };
                        launch.launch_config = launch_config.clone();
                        launch.attached_tab.clone()
                    };

                    if let Some(key) = attached_key {
                        if let Some(process_id) = process_id {
                            self.terminal_manager.processes.insert(
                                key.clone(),
                                self.tracked_process_for_tab(&key, &launch_config, process_id),
                            );
                        }
                        self.terminal_manager.pending_launches.remove(&key);
                        self.clear_terminal_recent_output(&key);
                        self.terminal_manager.errors.remove(&key);
                        if let (Some(broadcast), Some(writer)) =
                            (runtime.output_broadcast(), runtime.writer_handle())
                        {
                            self.register_tab_with_registry(&key, broadcast, writer);
                        }
                        self.terminal_surface_snapshots
                            .insert(key.clone(), runtime.snapshot());
                        self.live_terminal_runtimes.insert(key.clone(), runtime);
                        self.send_pending_post_launch_input(&key);
                        self.update_terminal_tab(&key, cx, |tab| {
                            tab.launch_config = launch_config.clone();
                            tab.restore_status = TerminalRestoreStatus::Ready;
                        });
                        updated = true;
                    } else {
                        if let Some(process_id) = process_id {
                            self.prewarmed_terminal_processes.insert(
                                launch_id,
                                self.tracked_process_for_prewarmed(&launch_config, process_id),
                            );
                        }
                        if let Some(launch) = self.prewarmed_terminal_launches.get_mut(&launch_id) {
                            launch.runtime = Some(runtime);
                        } else {
                            runtime.kill();
                        }
                    }
                }
                Ok(WarmTerminalLaunchReply::Output { launch_id, bytes }) => {
                    crate::leakscope::note_drain_output(bytes.len());
                    drained_output_bytes = drained_output_bytes.saturating_add(bytes.len());
                    let attached_key = self
                        .prewarmed_terminal_launches
                        .get(&launch_id)
                        .and_then(|launch| launch.attached_tab.clone());

                    if let Some(key) = attached_key {
                        self.append_terminal_recent_output(&key, &bytes);
                        if let Some(runtime) = self.live_terminal_runtimes.get_mut(&key) {
                            let terminal_update = runtime.apply_output(&bytes);
                            self.terminal_surface_snapshots
                                .insert(key.clone(), runtime.snapshot());
                            if terminal_update.bell {
                                self.terminal_bell_at.insert(key.clone(), Instant::now());
                            }
                            self.update_terminal_tab(&key, cx, |tab| {
                                apply_terminal_title_update(tab, &terminal_update);
                            });
                            // Same Output broadcast as the cold-path
                            // drain — warm-prewarm tabs (MCP spawn,
                            // GUI new-task fast path) also surface
                            // their bytes to MCP subscribers. Guarded
                            // on receiver_count so the no-subscriber
                            // case skips the per-chunk clone.
                            self.maybe_emit_tab_output(&key.tab_id, &bytes);
                            updated = true;
                        } else if self.maybe_retry_claude_restore(&key, cx) {
                            updated = true;
                        }
                        if drained_output_bytes >= DRAIN_OUTPUT_BYTE_CAP {
                            break;
                        }
                        continue;
                    }

                    if let Some(launch) = self.prewarmed_terminal_launches.get_mut(&launch_id) {
                        if let Some(runtime) = launch.runtime.as_mut() {
                            let _ = runtime.apply_output(&bytes);
                        }
                    }
                    if drained_output_bytes >= DRAIN_OUTPUT_BYTE_CAP {
                        break;
                    }
                }
                Ok(WarmTerminalLaunchReply::SessionDiscovered { launch_id, session }) => {
                    let Some(launch) = self.prewarmed_terminal_launches.get_mut(&launch_id) else {
                        continue;
                    };

                    launch.launch_config = launch
                        .launch_config
                        .clone()
                        .with_session(Some(session.clone()));

                    if let Some(key) = launch.attached_tab.clone() {
                        let section_id = key.section_id.clone();
                        let launch_config = launch.launch_config.clone();
                        let applied = self.workspace_pane.update(cx, |workspace, cx| {
                            let Some(tab) =
                                workspace.section_states.get_mut(&key.section_id).and_then(
                                    |state| state.tabs.iter_mut().find(|tab| tab.id == key.tab_id),
                                )
                            else {
                                return false;
                            };

                            tab.launch_config = launch_config.clone();
                            workspace.persist_section_state(&section_id, cx);
                            cx.notify();
                            true
                        });
                        updated |= applied;
                    }
                }
                Ok(WarmTerminalLaunchReply::Exited { launch_id, status }) => {
                    let attached_key = self
                        .prewarmed_terminal_launches
                        .get(&launch_id)
                        .and_then(|launch| launch.attached_tab.clone());

                    self.prewarmed_terminal_launches.remove(&launch_id);
                    self.prewarmed_terminal_processes.remove(&launch_id);
                    self.canceled_prewarmed_launch_ids.remove(&launch_id);

                    if let Some(key) = attached_key {
                        if self.maybe_retry_claude_restore(&key, cx) {
                            self.terminal_manager.processes.remove(&key);
                            self.live_terminal_runtimes.remove(&key);
                            self.terminal_surface_snapshots.remove(&key);
                            self.unregister_tab_from_registry(&key);
                            updated = true;
                            continue;
                        }
                        self.terminal_manager.pending_launches.remove(&key);
                        self.terminal_manager.processes.remove(&key);
                        self.terminal_surface_snapshots.remove(&key);
                        let details = Self::terminal_failure_details(
                            &status,
                            self.terminal_manager
                                .recent_output
                                .get(&key)
                                .map(String::as_str),
                        );
                        self.terminal_manager.errors.insert(key.clone(), details);
                        self.clear_terminal_recent_output(&key);
                        self.live_terminal_runtimes.remove(&key);
                        self.unregister_tab_from_registry(&key);
                        self.update_terminal_tab(&key, cx, |tab| {
                            tab.restore_status = TerminalRestoreStatus::Failed;
                        });
                        updated = true;
                    }
                }
                Ok(WarmTerminalLaunchReply::Failed {
                    launch_id,
                    message,
                    details,
                }) => {
                    let attached_key = self
                        .prewarmed_terminal_launches
                        .get(&launch_id)
                        .and_then(|launch| launch.attached_tab.clone());

                    self.prewarmed_terminal_launches.remove(&launch_id);
                    self.prewarmed_terminal_processes.remove(&launch_id);
                    self.canceled_prewarmed_launch_ids.remove(&launch_id);
                    if self.active_add_agent_warm_launch_id == Some(launch_id) {
                        self.active_add_agent_warm_launch_id = None;
                    }

                    if let Some(key) = attached_key {
                        self.terminal_manager.pending_launches.remove(&key);
                        self.terminal_manager.processes.remove(&key);
                        self.live_terminal_runtimes.remove(&key);
                        self.terminal_surface_snapshots.remove(&key);
                        self.unregister_tab_from_registry(&key);
                        self.terminal_manager
                            .errors
                            .insert(key.clone(), details.clone());
                        self.clear_terminal_recent_output(&key);
                        self.update_terminal_tab(&key, cx, |tab| {
                            tab.restore_status = TerminalRestoreStatus::Failed;
                        });
                        self.show_error_details_toast(message, details, cx);
                        updated = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        if updated {
            self.last_terminal_activity = Instant::now();
        }

        updated
    }

    /// Expose a just-launched tab to the embedded daemon. Broadcast
    /// sender = PTY output tee (cloned from `LiveTerminalRuntime`);
    /// writer = `Arc<Mutex<…>>` shared with the runtime's own stdin
    /// path. Overwrites any prior entry for `key` — a relaunch swaps
    /// its sender but mobile clients keep their session open.
    pub(crate) fn register_tab_with_registry(
        &self,
        key: &TerminalRuntimeKey,
        broadcast_sender: tokio::sync::broadcast::Sender<Vec<u8>>,
        writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    ) {
        if let Ok(mut state) = self.registry_state.lock() {
            state.broadcasts.insert(key.clone(), broadcast_sender);
            state.writers.insert(key.clone(), writer);
            // Launch completed — clear the "in flight" guard so a
            // follow-up LaunchTab that arrives after a tab exits
            // isn't silently dropped as "already being spawned".
            state.in_flight_launches.remove(key);
        }
    }

    /// Drop a tab's entries from the registry. Called when the
    /// associated runtime exits, fails, or its tab is closed — a
    /// mobile client's future `AttachTab` for this key will report
    /// the tab as not running.
    ///
    /// Also cleans up viewport bookkeeping (active_viewers /
    /// effective_sizes) and evicts any viewer_focus pointer still
    /// aimed at this key. Without this, killed-tab keys linger in
    /// the viewport registry and recompute_effective_size enqueues
    /// no-op TabResize requests against a runtime that no longer
    /// exists.
    pub(crate) fn unregister_tab_from_registry(&self, key: &TerminalRuntimeKey) {
        if let Ok(mut state) = self.registry_state.lock() {
            state.broadcasts.remove(key);
            state.writers.remove(key);
            state.active_viewers.remove(key);
            state.effective_sizes.remove(key);
            state.in_flight_launches.remove(key);
            // Any viewer still focused on this key has a dangling
            // pointer — clear it so the next TabResize from that
            // viewer doesn't take the "drop old focus" branch
            // against a ghost key.
            state.viewer_focus.retain(|_, focus_key| focus_key != key);
        }
    }

    /// Mirror the latest `ProjectStore` into the registry so mobile
    /// `ListProjects` responses reflect recent renames / new tasks /
    /// tab changes. Cheap (full clone) but called only after state
    /// mutations, not per frame.
    /// Fire `Control::SetShortcutBinding` over the active session.
    /// Settings-page handlers and any future shortcut-mutating UI use
    /// this so the daemon's `set_shortcut_binding` mutator owns the
    /// write — desktop's local `project_store.ui` updates on the
    /// broadcast push that follows. Replaces direct
    /// `self.project_store.set_shortcut_binding` calls.
    pub(crate) fn dispatch_set_shortcut_binding(
        &self,
        action: another_one_core::shortcuts::ShortcutAction,
        binding: String,
    ) {
        let action_id = crate::daemon_host::shortcut_action_id(action).to_string();
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetShortcutBinding {
                action_id,
                binding,
            },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetShortcutBinding failed: {err}");
                }
            },
        );
    }

    /// Like [`Self::dispatch_set_shortcut_binding`] but clears the
    /// binding (empty string == "no override; fall back to default").
    pub(crate) fn dispatch_clear_shortcut_binding(
        &self,
        action: another_one_core::shortcuts::ShortcutAction,
    ) {
        // Clearing is encoded as an empty binding on the wire — the
        // daemon's handler routes it to `clear_shortcut_binding`.
        self.dispatch_set_shortcut_binding(action, String::new());
    }

    /// Reset a single shortcut to its default. The daemon's
    /// handler uses `reset_shortcut_binding` which restores the
    /// hardcoded default rather than the empty/cleared state.
    pub(crate) fn dispatch_reset_shortcut_binding(
        &self,
        action: another_one_core::shortcuts::ShortcutAction,
    ) {
        let action_id = crate::daemon_host::shortcut_action_id(action).to_string();
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::ResetShortcutBinding { action_id },
            |result| {
                if let Err(err) = result {
                    log::warn!("ResetShortcutBinding failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::SetGitCommitScript` over the active session.
    pub(crate) fn dispatch_set_git_commit_script(&self, script: String) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetGitCommitScript { script },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetGitCommitScript failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::ResetGitCommitScript` over the active session.
    pub(crate) fn dispatch_reset_git_commit_script(&self) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::ResetGitCommitScript,
            |result| {
                if let Err(err) = result {
                    log::warn!("ResetGitCommitScript failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::SetGitPrScript` over the active session.
    pub(crate) fn dispatch_set_git_pr_script(&self, script: String) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetGitPrScript { script },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetGitPrScript failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::ResetGitPrScript` over the active session.
    pub(crate) fn dispatch_reset_git_pr_script(&self) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::ResetGitPrScript,
            |result| {
                if let Err(err) = result {
                    log::warn!("ResetGitPrScript failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::SetGitCommitLlm` over the active session.
    pub(crate) fn dispatch_set_git_commit_llm(&self, settings: serde_json::Value) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetGitCommitLlm { settings },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetGitCommitLlm failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::SetGitPrLlm` over the active session.
    pub(crate) fn dispatch_set_git_pr_llm(&self, settings: serde_json::Value) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetGitPrLlm { settings },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetGitPrLlm failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::SetAgentEnabled` over the active session.
    pub(crate) fn dispatch_set_agent_enabled(&self, agent_id: String, enabled: bool) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetAgentEnabled { agent_id, enabled },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetAgentEnabled failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::SetDefaultAgent` over the active session.
    pub(crate) fn dispatch_set_default_agent(&self, agent_id: String) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetDefaultAgent { agent_id },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetDefaultAgent failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::SetAgentLaunchArgs` over the active session.
    pub(crate) fn dispatch_set_agent_launch_args(&self, agent_id: String, args: Vec<String>) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetAgentLaunchArgs { agent_id, args },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetAgentLaunchArgs failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::RemoveProject` over the active session.
    pub(crate) fn dispatch_remove_project(&self, project_id: String) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::RemoveProject { project_id },
            |result| {
                if let Err(err) = result {
                    log::warn!("RemoveProject failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::SetTaskPinned` over the active session.
    pub(crate) fn dispatch_set_task_pinned(&self, task_id: String, pinned: bool) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetTaskPinned { task_id, pinned },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetTaskPinned failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::RenameTask` over the active session.
    pub(crate) fn dispatch_rename_task(&self, task_id: String, new_name: String) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::RenameTask { task_id, new_name },
            |result| {
                if let Err(err) = result {
                    log::warn!("RenameTask failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::SetRepoDefaultCommitAction` over the active
    /// session. `action` is the variant id (`"commit"` or
    /// `"commit-and-push"`) — encoded as a string so daemon-proto
    /// stays free of the `RepoDefaultCommitAction` enum shape.
    pub(crate) fn dispatch_set_repo_default_commit_action(
        &self,
        repo_id: String,
        action: String,
    ) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::SetRepoDefaultCommitAction { repo_id, action },
            |result| {
                if let Err(err) = result {
                    log::warn!("SetRepoDefaultCommitAction failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::UpdateTaskBranch` over the active session.
    pub(crate) fn dispatch_update_task_branch(
        &self,
        task_id: String,
        target_project_id: String,
        branch_name: String,
    ) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::UpdateTaskBranch {
                task_id,
                target_project_id,
                branch_name,
            },
            |result| {
                if let Err(err) = result {
                    log::warn!("UpdateTaskBranch failed: {err}");
                }
            },
        );
    }

    /// Fire `Control::RemoveTask` over the active session.
    pub(crate) fn dispatch_remove_task(&self, project_id: String, task_id: String) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::RemoveTask {
                project_id,
                task_id,
            },
            |result| {
                if let Err(err) = result {
                    log::warn!("RemoveTask failed: {err}");
                }
            },
        );
    }

    /// Populate `workspace_pane.section_states[section_id]` from the
    /// matching task's persisted tabs in `project_store.tasks` if it
    /// isn't already there. Called before `activate_section` on tap
    /// handlers so the freshly-activated workspace pane uses the
    /// daemon's tab IDs instead of synthesising a new UUID via
    /// `SectionState::with_cwd`.
    ///
    /// No-op when the section already has a local state (the user
    /// tapped a task they've already activated this session) or when
    /// the project_store doesn't carry tabs for that section
    /// (project page sections, standalone shells).
    pub(crate) fn hydrate_section_state_from_store(
        &mut self,
        section_id: &SectionId,
        cx: &mut Context<Self>,
    ) {
        let already_present = self
            .workspace_pane
            .read(cx)
            .section_states
            .contains_key(section_id);
        if already_present {
            return;
        }
        let section_store_key = section_id.store_key();
        let task = self
            .project_store
            .tasks
            .values()
            .flatten()
            .find(|t| t.section_id == section_store_key);
        let Some(task) = task else {
            return;
        };
        if task.tabs.is_empty() {
            return;
        }
        let persisted = another_one_core::project_store::PersistedSectionState {
            active_tab_id: task.active_tab_id.clone(),
            next_tab_id: task.next_tab_id,
            cwd: task.cwd.clone(),
            tabs: task.tabs.clone(),
        };
        let fallback_cwd = self
            .project_store
            .project(&section_id.project_id)
            .map(|p| p.path.clone());
        let section_id_for_insert = section_id.clone();
        self.workspace_pane.update(cx, |workspace, _cx| {
            workspace
                .section_states
                .insert(
                    section_id_for_insert,
                    SectionState::from_persisted(persisted, fallback_cwd),
                );
        });
    }

    pub(crate) fn sync_registry_project_store(&self) {
        if let Ok(mut state) = self.registry_state.lock() {
            state.project_store = self.project_store.clone();
            // Fire the broadcast tick so any connected mobile session
            // pushes a fresh `WorkerReply::ProjectList` to its peer.
            // `send` errs only when there are no receivers — fine,
            // we're just announcing.
            let _ = state.state_change_tx.send(());
        }
    }

    /// Persist + sync after a direct GUI mutation. Replaces the
    /// scattered `self.project_store.save();
    /// self.sync_registry_project_store();` pattern with a single
    /// call so no callsite forgets the second half (which leaves
    /// mobile clients seeing stale state until something else
    /// triggers a sync).
    ///
    /// Use this for any direct `self.project_store.<mutator>` write
    /// site that hasn't yet been migrated to a `Control::*` verb;
    /// migrated sites flow through `dispatch_fire_and_forget` →
    /// daemon → `with_store_mut` → save+broadcast and don't need
    /// this.
    pub(crate) fn commit_local_mutation(&self) {
        self.project_store.save();
        self.sync_registry_project_store();
    }

    /// Poll the daemon-host thread for the `EndpointHandle`. Called
    /// on the render tick until it resolves — after that,
    /// `daemon_handle_rx` is `None` and this is a no-op.
    pub(crate) fn drain_daemon_handle(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = self.daemon_handle_rx.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(handle)) => {
                self.daemon_handle = Some(handle);
                self.daemon_handle_rx = None;
                true
            }
            Ok(Err(e)) => {
                log::warn!("daemon-host failed to start: {e:?}");
                self.daemon_handle_rx = None;
                // Surface once — the pair-mobile modal will show its
                // empty state anyway, but a toast tells the user
                // what's wrong if they try to open it.
                self.show_anyhow_error_toast(format!("Mobile daemon failed to start: {e}"), &e, cx);
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.daemon_handle_rx = None;
                false
            }
        }
    }

    /// Drain any resize requests enqueued by the daemon thread via
    /// `DesktopTerminalRegistry::tab_resize` and apply them through
    /// the existing `LiveTerminalRuntime::resize` path. Runs on the
    /// GPUI tick, where mutable runtime access is safe.
    /// Drain `RegistryState.pending_tab_launches` and call the same
    /// `spawn_terminal_launch` path a sidebar click would trigger.
    /// This makes mobile's `Control::LaunchTab` a first-class launch
    /// initiator — the desktop GUI no longer has to be the "master"
    /// that clicks a task before mobile can see its terminal.
    pub(crate) fn drain_pending_tab_launches(&mut self, cx: &mut Context<Self>) -> bool {
        let pending: Vec<crate::daemon_host::TabLaunchRequest> = {
            let Ok(mut state) = self.registry_state.lock() else {
                return false;
            };
            if state.pending_tab_launches.is_empty() {
                return false;
            }
            std::mem::take(&mut state.pending_tab_launches)
        };
        let mut changed = false;
        for request in pending {
            // Routed through the same client-trait verb a future
            // privileged MCP "wake this tab" call would use. Mobile
            // attach is just one driver of `client_attach_tab`.
            // We don't have a stable mobile-endpoint id here yet —
            // attribute the event to a generic mobile client until
            // `pending_tab_launches` carries the originating peer.
            let attach = AttachTabRequest {
                client_id: ClientId("mobile:attach".to_string()),
                section_id: request.key.section_id.clone(),
                tab_id: request.key.tab_id.clone(),
            };
            match self.client_attach_tab(attach, cx) {
                Ok(resp) if resp.launched => changed = true,
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(?request.key, %err, "attach_tab declined; mobile launch dropped");
                }
            }
        }
        if changed {
            cx.notify();
        }
        changed
    }

    /// Drain `RegistryState.pending_spawn_terminals` (MCP
    /// `spawn_terminal` asks routed through the daemon). Each entry
    /// carries a sync responder; we resolve the project/task it
    /// targets, ensure the section exists, add a fresh shell tab
    /// with default `TerminalLaunchConfig`, queue the PTY launch on
    /// the existing `pending_tab_launches` path so the next drain
    /// pass actually starts the shell, and reply with the new tab
    /// id. Errors are sent back as strings — bubbling up to the MCP
    /// caller as a JSON-RPC error.
    pub(crate) fn drain_pending_spawn_terminals(&mut self, cx: &mut Context<Self>) -> bool {
        let pending: Vec<crate::daemon_host::PendingSpawnTerminal> = {
            let Ok(mut state) = self.registry_state.lock() else {
                return false;
            };
            if state.pending_spawn_terminals.is_empty() {
                return false;
            }
            std::mem::take(&mut state.pending_spawn_terminals)
        };
        let mut changed = false;
        for req in pending {
            let result = self.fulfill_spawn_terminal(&req, cx);
            // The receiver may have hung up if the MCP caller's
            // recv_timeout fired; treat the send failure as a
            // benign drop, not a panic.
            let _ = req.responder.send(result);
            changed = true;
        }
        if changed {
            cx.notify();
        }
        changed
    }

    /// Drain the GUI's own `ClientEvent` receiver and surface peer-
    /// driven state changes (events whose `originator` is *not* the
    /// GUI) as info toasts. Volume control: `Output` events are
    /// skipped — bytes flow at a rate that would spam the toast
    /// surface useless. `FocusChanged` is also skipped when the
    /// target is the GUI: that's the daemon settling our own focus,
    /// already visible in the workspace.
    pub(crate) fn drain_gui_events(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = self.gui_event_receiver.as_mut() else {
            return false;
        };
        let gui = ClientId::gui_desktop();
        // Only mark `changed` when we actually have a toast to push,
        // so a chatty terminal's `Output` event stream doesn't force
        // a `cx.notify()` on every render tick. Drains 32 events per
        // tick; if the bus carries only Output events for that
        // window, the function quietly returns false.
        let mut changed = false;
        let mut toasts: Vec<String> = Vec::new();
        for _ in 0..32 {
            use tokio::sync::broadcast::error::TryRecvError;
            match rx.try_recv() {
                Ok(ev) => {
                    let toast = match &ev {
                        ClientEvent::Output { .. } => None,
                        ClientEvent::TaskOpened {
                            originator,
                            task_id,
                            ..
                        } if originator != &gui => Some(format!(
                            "{originator} opened a new task ({}…)",
                            &task_id[..task_id.len().min(8)]
                        )),
                        ClientEvent::TabOpened {
                            originator, tab_id, ..
                        } if originator != &gui => Some(format!(
                            "{originator} opened a new tab ({}…)",
                            &tab_id[..tab_id.len().min(8)]
                        )),
                        ClientEvent::TabClosed {
                            originator, tab_id, ..
                        } if originator != &gui => Some(format!(
                            "{originator} closed tab {}…",
                            &tab_id[..tab_id.len().min(8)]
                        )),
                        ClientEvent::TaskOpenStarted {
                            originator,
                            project_id,
                            ..
                        } if originator != &gui => Some(format!(
                            "{originator} is creating a worktree task in project {}…",
                            &project_id[..project_id.len().min(8)]
                        )),
                        ClientEvent::TaskOpenFailed {
                            originator, error, ..
                        } if originator != &gui => {
                            Some(format!("{originator} task creation failed: {error}"))
                        }
                        ClientEvent::FocusChanged {
                            originator, target, ..
                        } if originator != &gui && target == &gui => {
                            Some(format!("{originator} moved your view"))
                        }
                        _ => None,
                    };
                    if let Some(text) = toast {
                        toasts.push(text);
                        changed = true;
                    }
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Closed) => break,
                Err(TryRecvError::Lagged(skipped)) => {
                    toasts.push(format!(
                        "(missed {skipped} client events — slow-consumer buffer overflow)"
                    ));
                    changed = true;
                }
            }
        }
        for text in toasts {
            self.show_info_toast(text, cx);
        }
        changed
    }

    /// Probe the workspace for GUI-driven focus changes (mouse clicks
    /// on the sidebar, tab switches, project-page activations) and
    /// emit a `FocusChanged` event when the workspace's active
    /// section/tab differs from `last_observed_gui_focus`. Called on
    /// every drain tick so MCP `poll_events` sees what the human just
    /// did, attributed to `gui:desktop`.
    pub(crate) fn observe_gui_focus(&mut self, cx: &App) -> bool {
        let workspace = self.workspace_pane.read(cx);
        let current = if let Some(section_id) = workspace.active_section.clone() {
            let active_tab = workspace
                .section_states
                .get(&section_id)
                .and_then(|state| state.tabs.get(state.active_tab))
                .map(|tab| tab.id.clone());
            match active_tab {
                Some(tab_id) => Focus::Tab {
                    project_id: section_id.project_id.clone(),
                    task_id: section_id.task_id.clone(),
                    section_id: section_id.clone(),
                    tab_id,
                },
                None => Focus::Task {
                    project_id: section_id.project_id.clone(),
                    task_id: section_id.task_id.clone().unwrap_or_default(),
                },
            }
        } else if let Some(project_id) = workspace.active_project_page.clone() {
            Focus::Project { project_id }
        } else {
            Focus::None
        };
        let _ = workspace;
        if current == self.last_observed_gui_focus {
            return false;
        }
        self.last_observed_gui_focus = current.clone();
        let gui = ClientId::gui_desktop();
        self.client_focus.insert(gui.clone(), current.clone());
        self.emit_client_event(ClientEvent::FocusChanged {
            originator: gui.clone(),
            target: gui,
            focus: current,
        });
        true
    }

    /// Emit a `ClientEvent` on the daemon-side broadcast bus. The
    /// sender is owned directly so emits don't take the registry
    /// mutex — important because `ClientEvent::Output` fires per
    /// PTY chunk and would otherwise contend with daemon tokio
    /// tasks holding the registry lock for unrelated work. Each
    /// MCP session subscribes its own `broadcast::Receiver`.
    fn emit_client_event(&self, event: ClientEvent) {
        let _ = self.event_bus.send(event);
    }

    /// Specialized [`emit_client_event`] for `ClientEvent::Output`.
    /// The hot-path drains fire this once per PTY chunk; constructing
    /// the event cost a `tab_id.clone()` plus a `bytes.clone()` (8 KiB
    /// each) on the GPUI main thread even when nobody was subscribed.
    /// Most runs of the desktop app have zero MCP sessions attached,
    /// so skipping construction when `receiver_count == 0` is a
    /// straight main-thread win. See #127.
    fn maybe_emit_tab_output(&self, tab_id: &str, bytes: &[u8]) {
        if self.event_bus.receiver_count() == 0 {
            return;
        }
        let _ = self.event_bus.send(ClientEvent::Output {
            tab_id: tab_id.to_string(),
            bytes: bytes.to_vec(),
        });
    }

    /// `DaemonClient::open_task` for AnotherOneApp. Single source of
    /// truth that both the GUI new-task modal and the MCP
    /// `spawn_terminal` flow go through. Resolves project/branch
    /// defaults, calls `insert_and_open_task` (which adds to
    /// `project_store.tasks`, expands the project in the sidebar,
    /// activates the section, and starts the PTY), syncs the
    /// daemon's project-store snapshot, and emits a `TaskOpened`
    /// event on the bus.
    pub(crate) fn client_open_task(
        &mut self,
        req: OpenTaskRequest,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<OpenTaskResponse> {
        let project = self
            .project_store
            .project(&req.project_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown project_id {}", req.project_id))?;
        let branch_name = req.branch_name.clone().unwrap_or_else(|| {
            another_one_core::project_store::current_branch(&project.path)
                .or_else(|| self.project_store.current_branch_name(&project.id))
                .unwrap_or_default()
        });
        let task_name = req
            .task_name
            .as_ref()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(crate::new_task_modal::generate_task_name);
        let project_path = req.cwd.clone().unwrap_or_else(|| project.path.clone());

        // `client_open_task` is the synchronous task-create verb
        // and only fits the `Direct` kind. Worktree creation runs
        // a background `project_service::spawn_task_creation` and
        // doesn't fit a sync request/response — the right verb is
        // the (forthcoming) async `create_worktree_task` MCP tool,
        // which returns a `JobId` and emits `TaskOpenStarted` /
        // `TaskOpened` / `TaskOpenFailed` on the bus. Reject here
        // so callers don't misinterpret a plain `client_open_task`
        // call as supporting worktrees.
        match req.kind {
            another_one_core::project_store::TaskKind::Direct => {}
            other => anyhow::bail!(
                "client_open_task only supports TaskKind::Direct ({:?} requires the \
                 async `create_worktree_task` verb — observe `TaskOpenStarted` / \
                 `TaskOpened` events to track completion)",
                other
            ),
        }

        let (task_id, _section) = self.insert_and_open_task(
            project.id.clone(),
            project.id.clone(),
            req.kind,
            task_name.clone(),
            branch_name,
            None,
            project_path,
            Some(req.launch_config.clone()),
            req.warm_launch_hint,
            req.client_id.clone(),
            cx,
        );

        // The new task always becomes the active section by virtue of
        // `insert_and_open_task`'s `activate_section` call, so the
        // active terminal key is the just-added tab.
        let key = self
            .active_terminal_key(cx)
            .ok_or_else(|| anyhow::anyhow!("task created but no active terminal key resolved"))?;

        if req.focus_after_open {
            self.client_focus.insert(
                req.client_id.clone(),
                Focus::Tab {
                    project_id: project.id.clone(),
                    task_id: Some(task_id.clone()),
                    section_id: key.section_id.clone(),
                    tab_id: key.tab_id.clone(),
                },
            );
        }

        // The TaskOpened event was already emitted from
        // `insert_and_open_task` with this client's originator.
        self.sync_registry_project_store();

        Ok(OpenTaskResponse {
            task_id,
            section_id: key.section_id,
            tab_id: key.tab_id,
        })
    }

    /// `DaemonClient::open_tab` — append a tab to an existing
    /// section. Used by MCP when the caller already supplied a
    /// `task_id` (so they don't want a brand-new task) and by the
    /// GUI's add-agent modal for the same case.
    pub(crate) fn client_open_tab(
        &mut self,
        req: OpenTabRequest,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<OpenTabResponse> {
        let section_id = req.section_id.clone();
        let project_path = self
            .project_store
            .project(&section_id.project_id)
            .map(|p| p.path.clone());
        let originator_for_workspace = req.client_id.clone();
        let added_tab_id = self.workspace_pane.update(cx, |workspace, cx| {
            if req.focus_after_open {
                workspace.activate_section(
                    section_id.clone(),
                    project_path.clone(),
                    Some(req.launch_config.clone()),
                    cx,
                );
            } else {
                workspace.ensure_section(
                    section_id.clone(),
                    project_path.clone(),
                    Some(req.launch_config.clone()),
                    cx,
                );
            }
            workspace.add_tab_with_launch_config_attributed(
                &section_id,
                req.launch_config.clone(),
                None,
                originator_for_workspace,
                cx,
            )
        });
        let tab_id = added_tab_id.ok_or_else(|| {
            anyhow::anyhow!("could not add tab to section {}", section_id.store_key())
        })?;

        if req.focus_after_open {
            self.client_focus.insert(
                req.client_id.clone(),
                Focus::Tab {
                    project_id: section_id.project_id.clone(),
                    task_id: section_id.task_id.clone(),
                    section_id: section_id.clone(),
                    tab_id: tab_id.clone(),
                },
            );
        }

        // If a warm launch was prewarmed (GUI fast path), attach it
        // to the new tab key. Cold path (no warm hint) lets the
        // existing render-tick `ensure_active_terminal_runtime`
        // spawn the PTY when the section becomes active.
        if let Some(warm_id) = req.warm_launch_hint {
            let key = TerminalRuntimeKey {
                section_id: section_id.clone(),
                tab_id: tab_id.clone(),
            };
            if !self.attach_prewarmed_launch_to_tab(warm_id, key, cx) {
                self.cancel_prewarmed_launch(warm_id);
            }
        }

        // The TabOpened event was already deferred from
        // `add_tab_with_launch_config_attributed` with this client's
        // originator — no manual emit needed here.
        let _ = section_id;

        self.sync_registry_project_store();
        Ok(OpenTabResponse { tab_id })
    }

    /// `DaemonClient::attach_tab` — make sure the tab's PTY is
    /// live and start broadcasting its bytes. Idempotent: a follow-
    /// up attach for an already-running tab returns
    /// `launched: false` without spawning a duplicate PTY. Used by
    /// mobile clients connecting in via iroh; in principle MCP
    /// could also call this to "wake" a persisted-but-cold tab,
    /// though MCP today expects to drive its own tabs.
    pub(crate) fn client_attach_tab(
        &mut self,
        req: AttachTabRequest,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<AttachTabResponse> {
        let key = TerminalRuntimeKey {
            section_id: req.section_id.clone(),
            tab_id: req.tab_id.clone(),
        };

        if self.live_terminal_runtimes.contains_key(&key)
            || self.terminal_manager.pending_launches.contains(&key)
        {
            return Ok(AttachTabResponse { launched: false });
        }

        let section_store_key = req.section_id.store_key();
        // First try the task-bound path: workspace tabs that live
        // under a task carry their `cwd` + `target_project_id` on the
        // task. Fall back to the global `terminal_sections` store for
        // sections that aren't owned by a task (project pages,
        // standalone shells) — without this fallback, a `LaunchTab` /
        // `AttachTab` issued via `Session::call` for a global section
        // gets rejected even though the desktop sidebar would happily
        // launch the same tab via the direct `spawn_terminal_launch`
        // path. See another-one-cwn for the migration this enables.
        let task_match = self
            .project_store
            .tasks
            .values()
            .flatten()
            .find_map(|t| {
                if t.section_id != section_store_key {
                    return None;
                }
                t.tabs
                    .iter()
                    .find(|pt| pt.id == req.tab_id)
                    .map(|pt| (t.clone(), pt.clone()))
            });
        let (persisted_tab, cwd) = match task_match {
            Some((task, persisted_tab)) => {
                let cwd = task
                    .cwd
                    .clone()
                    .or_else(|| self.project_path(&task.target_project_id));
                (persisted_tab, cwd)
            }
            None => {
                let section = self
                    .project_store
                    .terminal_sections
                    .get(&section_store_key)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "no persisted tab for section {} / tab {}",
                            section_store_key,
                            req.tab_id
                        )
                    })?;
                let persisted_tab = section
                    .tabs
                    .iter()
                    .find(|pt| pt.id == req.tab_id)
                    .cloned()
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "no persisted tab for section {} / tab {}",
                            section_store_key,
                            req.tab_id
                        )
                    })?;
                (persisted_tab, section.cwd.clone())
            }
        };
        let launch_config = persisted_tab
            .launch_config
            .clone()
            .ok_or_else(|| anyhow::anyhow!("persisted tab {} has no launch_config", req.tab_id))?;
        let agent_launch_args = self.agent_launch_args_for_launch_config(&launch_config);
        // Default grid; the attaching viewer will follow up with a
        // resize to its actual viewport. Min-across-viewers logic
        // in `RegistryState::recompute_effective_size` drives the
        // PTY to whatever's actually being displayed.
        let size = TerminalGridSize {
            cols: 100,
            rows: 30,
            pixel_width: 0,
            pixel_height: 0,
        };

        self.terminal_manager.pending_launches.insert(key.clone());
        if let Ok(mut state) = self.registry_state.lock() {
            state.in_flight_launches.insert(key.clone());
        }
        self.update_terminal_tab(&key, cx, |tab| {
            tab.restore_status = TerminalRestoreStatus::Launching;
        });
        spawn_terminal_launch(
            self.terminal_launch_sender.clone(),
            key.clone(),
            cwd,
            launch_config,
            agent_launch_args,
            size,
        );
        self.last_terminal_activity = Instant::now();
        self.emit_client_event(ClientEvent::TabOpened {
            originator: req.client_id,
            section_id: req.section_id,
            tab_id: req.tab_id,
        });
        Ok(AttachTabResponse { launched: true })
    }

    /// `DaemonClient::close_tab` — close a tab by id (any client).
    pub(crate) fn client_close_tab(
        &mut self,
        req: CloseTabRequest,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let target =
            self.workspace_pane
                .read(cx)
                .section_states
                .iter()
                .find_map(|(section_id, state)| {
                    state
                        .tabs
                        .iter()
                        .position(|t| t.id == req.tab_id)
                        .map(|idx| (section_id.clone(), idx))
                });
        let Some((section_id, tab_index)) = target else {
            anyhow::bail!("tab {} not found", req.tab_id);
        };
        let removed = self.workspace_pane.update(cx, |workspace, cx| {
            workspace.close_tab(&section_id, tab_index, cx)
        });
        if removed.is_none() {
            anyhow::bail!("close_tab: workspace declined to remove tab {}", req.tab_id);
        }
        self.emit_client_event(ClientEvent::TabClosed {
            originator: req.client_id,
            tab_id: req.tab_id.clone(),
        });
        self.sync_registry_project_store();
        Ok(())
    }

    /// `DaemonClient::select` — update the calling client's focus.
    /// When the caller is the GUI we *also* activate the section so
    /// the user actually sees the change. For other clients it's
    /// just a bookkeeping update — privileged clients use
    /// `client_select_for` to drive a peer's view.
    pub(crate) fn client_select(
        &mut self,
        req: SelectRequest,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.client_focus
            .insert(req.client_id.clone(), req.focus.clone());
        if req.client_id == ClientId::gui_desktop() {
            self.apply_focus_to_workspace(&req.focus, cx);
        }
        self.emit_client_event(ClientEvent::FocusChanged {
            originator: req.client_id.clone(),
            target: req.client_id,
            focus: req.focus,
        });
        Ok(())
    }

    /// `PrivilegedClient::select_for` — drive a peer client's focus.
    /// Used by MCP to scroll the GUI's view to a tab the harness
    /// just spawned.
    pub(crate) fn client_select_for(
        &mut self,
        actor: ClientId,
        target: ClientId,
        focus: Focus,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.client_focus.insert(target.clone(), focus.clone());
        if target == ClientId::gui_desktop() {
            self.apply_focus_to_workspace(&focus, cx);
        }
        self.emit_client_event(ClientEvent::FocusChanged {
            originator: actor,
            target,
            focus,
        });
        Ok(())
    }

    fn apply_focus_to_workspace(&mut self, focus: &Focus, cx: &mut Context<Self>) {
        match focus {
            Focus::None => {}
            Focus::Project { project_id } => {
                self.workspace_pane.update(cx, |workspace, cx| {
                    workspace.activate_project_page(project_id.clone(), cx);
                });
            }
            Focus::Task {
                project_id: _,
                task_id: _,
            } => {
                // Tasks-without-tab is rare; the GUI doesn't have a
                // dedicated affordance, so we no-op for now. v2 can
                // extend this once the trait stabilises.
            }
            Focus::Tab {
                section_id,
                tab_id,
                project_id: _,
                task_id: _,
            } => {
                let section_id = section_id.clone();
                let target_tab_id = tab_id.clone();
                self.workspace_pane.update(cx, |workspace, cx| {
                    workspace.activate_section(section_id.clone(), None, None, cx);
                    if let Some(state) = workspace.section_states.get_mut(&section_id) {
                        if let Some(idx) = state.tabs.iter().position(|t| t.id == target_tab_id) {
                            state.active_tab = idx;
                        }
                    }
                    cx.notify();
                });
            }
        }
    }

    fn fulfill_spawn_terminal(
        &mut self,
        req: &crate::daemon_host::PendingSpawnTerminal,
        cx: &mut Context<Self>,
    ) -> Result<another_one_core::mcp::orchestrator::SpawnTerminalResponse, String> {
        // MCP `spawn_terminal` is a thin wrapper: turn the
        // wire-format request into the `OpenTask{,Tab}Request`
        // vocabulary and delegate to the trait surface. The GUI's
        // new-task path goes through the same surface, so this
        // function intentionally has no domain logic of its own.
        let client_id = ClientId::mcp(req.client_handle.as_deref().unwrap_or("anonymous"));
        let launch_config = crate::agents::TerminalLaunchConfig::default();

        if let Some(task_id) = req.task_id.clone() {
            // "Add a tab to an existing task" — attach to the task's
            // section.
            let task = self
                .project_store
                .task(&task_id)
                .cloned()
                .ok_or_else(|| format!("unknown task_id {task_id}"))?;
            let section_id = SectionId::from_store_key(&task.section_id)
                .ok_or_else(|| format!("malformed section_id on task {task_id}"))?;
            let response = self
                .client_open_tab(
                    OpenTabRequest {
                        client_id,
                        section_id,
                        launch_config,
                        focus_after_open: true,
                        warm_launch_hint: None,
                    },
                    cx,
                )
                .map_err(|e| e.to_string())?;
            return Ok(another_one_core::mcp::orchestrator::SpawnTerminalResponse {
                tab_id: response.tab_id,
            });
        }

        let project_id = req
            .project_id
            .clone()
            .ok_or_else(|| "project_id required when task_id is absent".to_string())?;
        let response = self
            .client_open_task(
                OpenTaskRequest {
                    client_id,
                    project_id,
                    task_name: None,
                    branch_name: None,
                    kind: crate::project_store::TaskKind::Direct,
                    launch_config,
                    cwd: req.cwd.as_ref().map(std::path::PathBuf::from),
                    focus_after_open: true,
                    warm_launch_hint: None,
                },
                cx,
            )
            .map_err(|e| e.to_string())?;
        Ok(another_one_core::mcp::orchestrator::SpawnTerminalResponse {
            tab_id: response.tab_id,
        })
    }

    /// Apply a `UiAction` to desktop-only ephemeral state. Single
    /// match site for everything that the GUI's GPUI Action handlers
    /// also flip — opening / closing overlays, zoom, focus moves,
    /// etc. MCP routes here too via `drain_pending_ui_actions`, so
    /// the same code path runs whether a button click or an MCP tool
    /// call triggered the action.
    ///
    /// New `UiAction` variants land here alongside the GUI handler
    /// that constructs them. Add to `another_one_core::mcp::orchestrator::UiAction`
    /// (so MCP can name it on the wire), match here, then refactor
    /// the legacy GUI handler to construct the `UiAction` and call
    /// this method.
    pub(crate) fn dispatch_ui_action(
        &mut self,
        action: another_one_core::mcp::orchestrator::UiAction,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        use another_one_core::mcp::orchestrator::UiAction;
        match action {
            UiAction::OpenPairMobile => {
                if !self.pair_mobile_modal_open {
                    self.pair_mobile_modal_open = true;
                    self.pair_mobile_reset_pending = false;
                    cx.notify();
                }
                Ok(())
            }
            UiAction::ClosePairMobile => {
                if self.pair_mobile_modal_open {
                    self.pair_mobile_modal_open = false;
                    cx.notify();
                }
                Ok(())
            }
        }
    }

    /// Drain MCP-routed `UiAction` requests onto the GPUI thread.
    /// Same render-tick pattern as `drain_pending_select_focus` etc.
    pub(crate) fn drain_pending_ui_actions(&mut self, cx: &mut Context<Self>) -> bool {
        let pending: Vec<crate::daemon_host::PendingUiAction> = {
            let Ok(mut state) = self.registry_state.lock() else {
                return false;
            };
            if state.pending_ui_actions.is_empty() {
                return false;
            }
            std::mem::take(&mut state.pending_ui_actions)
        };
        let mut changed = false;
        for req in pending {
            let result = self.dispatch_ui_action(req.action, cx);
            let _ = req.responder.send(result);
            changed = true;
        }
        changed
    }

    pub(crate) fn drain_pending_select_focus(&mut self, cx: &mut Context<Self>) -> bool {
        let pending: Vec<crate::daemon_host::PendingSelectFocus> = {
            let Ok(mut state) = self.registry_state.lock() else {
                return false;
            };
            if state.pending_select_focus.is_empty() {
                return false;
            }
            std::mem::take(&mut state.pending_select_focus)
        };
        let mut changed = false;
        for req in pending {
            let actor = ClientId::mcp(req.client_handle.as_deref().unwrap_or("anonymous"));
            // No `for_client` (or `for_client == self`) is the
            // non-privileged "set my own focus" — routes through
            // `client_select`. With an explicit target it's the
            // privileged cross-client variant.
            let result = match req.for_client {
                Some(ref target) if target != &actor => self
                    .client_select_for(actor, target.clone(), req.focus, cx)
                    .map_err(|e| e.to_string()),
                _ => self
                    .client_select(
                        SelectRequest {
                            client_id: actor,
                            focus: req.focus,
                        },
                        cx,
                    )
                    .map_err(|e| e.to_string()),
            };
            let _ = req.responder.send(result);
            changed = true;
        }
        if changed {
            cx.notify();
        }
        changed
    }

    pub(crate) fn drain_pending_close_tabs(&mut self, cx: &mut Context<Self>) -> bool {
        let pending: Vec<crate::daemon_host::PendingCloseTab> = {
            let Ok(mut state) = self.registry_state.lock() else {
                return false;
            };
            if state.pending_close_tabs.is_empty() {
                return false;
            }
            std::mem::take(&mut state.pending_close_tabs)
        };
        let mut changed = false;
        for req in pending {
            let client_id = ClientId::mcp(req.client_handle.as_deref().unwrap_or("anonymous"));
            let result = self
                .client_close_tab(
                    CloseTabRequest {
                        client_id,
                        tab_id: req.tab_id,
                    },
                    cx,
                )
                .map_err(|e| e.to_string());
            let _ = req.responder.send(result);
            changed = true;
        }
        if changed {
            cx.notify();
        }
        changed
    }

    pub(crate) fn drain_pending_tab_resizes(&mut self, cx: &mut Context<Self>) -> bool {
        let pending: Vec<crate::daemon_host::TabResizeRequest> = {
            let Ok(mut state) = self.registry_state.lock() else {
                return false;
            };
            if state.pending_resizes.is_empty() {
                return false;
            }
            std::mem::take(&mut state.pending_resizes)
        };
        let mut changed = false;
        for request in pending {
            let Some(runtime) = self.live_terminal_runtimes.get_mut(&request.key) else {
                continue;
            };
            let size = TerminalGridSize {
                cols: request.cols,
                rows: request.rows,
                pixel_width: 0,
                pixel_height: 0,
            };
            match runtime.resize(size) {
                Ok(true) => {
                    // Agents that paint via the alternate screen
                    // buffer (Claude, Codex, etc.) don't repaint on
                    // their own after a SIGWINCH — nudge them with
                    // a soft form-feed so the reshaped grid fills
                    // with content rather than tearing the last
                    // paint.
                    if runtime.is_alternate_screen() {
                        let _ = runtime.request_soft_redraw();
                    }
                    self.terminal_surface_snapshots
                        .insert(request.key.clone(), runtime.snapshot());
                    changed = true;
                }
                Ok(false) => {}
                Err(error) => {
                    self.terminal_manager
                        .errors
                        .insert(request.key.clone(), error.to_string());
                }
            }
        }
        if changed {
            self.last_terminal_activity = Instant::now();
            cx.notify();
        }
        changed
    }

    pub(crate) fn terminal_snapshot_for(
        &self,
        key: &TerminalRuntimeKey,
    ) -> Option<TerminalSurfaceSnapshot> {
        self.terminal_surface_snapshots.get(key).cloned()
    }

    pub(crate) fn terminal_error_for(&self, key: &TerminalRuntimeKey) -> Option<&str> {
        self.terminal_manager.errors.get(key).map(String::as_str)
    }

    pub(crate) fn terminal_is_pending(&self, key: &TerminalRuntimeKey) -> bool {
        self.terminal_manager.pending_launches.contains(key)
    }

    pub(crate) fn active_terminal_key(&self, cx: &App) -> Option<TerminalRuntimeKey> {
        let workspace = self.workspace_pane.read(cx);
        let section_id = workspace.active_section.clone()?;
        let state = workspace.section_states.get(&section_id)?;
        let tab = state.tabs.get(state.active_tab)?;
        Some(TerminalRuntimeKey {
            section_id,
            tab_id: tab.id.clone(),
        })
    }

    pub(crate) fn write_active_terminal_input(&mut self, cx: &App, bytes: &[u8]) -> bool {
        let Some(key) = self.active_terminal_key(cx) else {
            return false;
        };
        let Some(runtime) = self.live_terminal_runtimes.get(&key) else {
            return false;
        };
        // Viewer-only runtimes have no local PTY; route input over
        // `Session::push_data` instead so the daemon's writer feeds
        // the real shell.
        if !runtime.has_local_pty() {
            let session = self.session_handle();
            let section_id = key.section_id.store_key();
            let tab_id = key.tab_id.clone();
            let payload = bytes.to_vec();
            crate::session_host::runtime_handle().spawn(async move {
                if let Err(err) = session.push_data(&section_id, &tab_id, &payload).await {
                    log::warn!("session push_data failed: {err}");
                }
            });
            self.last_terminal_activity = Instant::now();
            return true;
        }
        let wrote = runtime.write_input(bytes).is_ok();
        if wrote {
            self.last_terminal_activity = Instant::now();
        }
        wrote
    }

    fn send_pending_post_launch_input(&mut self, key: &TerminalRuntimeKey) -> bool {
        let Some(bytes) = self.pending_post_launch_input.remove(key) else {
            return false;
        };
        let Some(runtime) = self.live_terminal_runtimes.get(key) else {
            self.pending_post_launch_input.insert(key.clone(), bytes);
            return false;
        };
        let wrote = runtime.write_input(&bytes).is_ok();
        if wrote {
            self.last_terminal_activity = Instant::now();
        }
        wrote
    }

    pub(crate) fn run_project_action(
        &mut self,
        action: ProjectAction,
        window: Option<&Window>,
        cx: &mut Context<Self>,
    ) {
        let Some(section_id) = self.workspace_pane.read(cx).active_section.clone() else {
            self.show_error_toast("Custom actions run inside an active task.", cx);
            return;
        };

        let action_id = action.id.clone();
        if let Err(error) = self.run_project_action_in_section(
            &section_id,
            action,
            window.map(|window| self.terminal_panel_size(window)),
            cx,
        ) {
            self.show_error_toast(error, cx);
        } else {
            self.last_used_custom_action_id = Some(action_id);
        }
    }

    fn run_project_action_in_section(
        &mut self,
        section_id: &SectionId,
        action: ProjectAction,
        launch_size: Option<TerminalGridSize>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let cwd = self
            .cwd_for_section(section_id, cx)
            .ok_or_else(|| "Could not find the task worktree for this action.".to_string())?;
        let (launch_config, post_launch_input, fixed_title) = match &action.kind {
            ProjectActionKind::Shell { command } => {
                let command = command.trim();
                if command.is_empty() {
                    return Err("Shell actions need a command before they can run.".to_string());
                }
                (
                    TerminalLaunchConfig::default(),
                    Some(format!("{command}\n").into_bytes()),
                    fixed_title_for_project_action(&action),
                )
            }
            ProjectActionKind::Agent { provider, .. } => {
                let args = crate::project_store::project_action_agent_launch_args(&action)?;
                (
                    TerminalLaunchConfig::for_provider(*provider)
                        .with_extra_args(args)
                        .with_agent_launch_args(false),
                    None,
                    None,
                )
            }
        };

        let tab_id = self
            .workspace_pane
            .update(cx, |workspace, cx| {
                workspace.add_tab_with_launch_config(
                    section_id,
                    launch_config.clone(),
                    fixed_title.clone(),
                    cx,
                )
            })
            .ok_or_else(|| "Could not add an action tab for this task.".to_string())?;
        let key = TerminalRuntimeKey {
            section_id: section_id.clone(),
            tab_id,
        };
        if let Some(input) = post_launch_input {
            self.pending_post_launch_input.insert(key.clone(), input);
        }

        if let Some(size) = launch_size {
            self.terminal_manager.pending_launches.insert(key.clone());
            self.update_terminal_tab(&key, cx, |tab| {
                tab.restore_status = TerminalRestoreStatus::Launching;
            });
            spawn_terminal_launch(
                self.terminal_launch_sender.clone(),
                key,
                Some(cwd),
                launch_config.clone(),
                self.agent_launch_args_for_launch_config(&launch_config),
                size,
            );
        }

        cx.notify();
        Ok(())
    }

    /// Open the Cmd-F scrollback search overlay over the active
    /// terminal. No-op when no terminal is active. If a search is
    /// already open on a different terminal, replaces it.
    /// Compute the link-hover state for a mouse position inside a
    /// terminal pane, without touching `WorkspacePane`. The caller
    /// (panel-side `on_mouse_move`) already holds `&mut WorkspacePane`
    /// — they apply the result directly to `this.terminal_link_hover`
    /// to avoid the nested-update panic that calling
    /// `workspace_pane.read(cx)` from inside an `app.update(cx, …)`
    /// triggers.
    pub(crate) fn compute_terminal_link_hover(
        &self,
        key: &TerminalRuntimeKey,
        mouse_position: gpui::Point<Pixels>,
        window: &mut Window,
    ) -> Option<TerminalLinkHoverState> {
        let metrics = self.terminal_panel_metrics_for_key(key, window)?;
        let cell = terminal_cell_position_from_mouse(mouse_position, &metrics)?;
        let snapshot = self.terminal_surface_snapshots.get(key)?;
        let link = terminal_link_at_position(snapshot, cell)?;
        // Anchor is pane-relative because the tooltip element renders
        // as a child of the (already absolutely-positioned) pane div.
        // Storing window-relative coords here would compose two
        // offsets and paint the tooltip way off to the side.
        Some(TerminalLinkHoverState {
            section_id: key.section_id.clone(),
            tab_id: key.tab_id.clone(),
            link,
            anchor_x: (f32::from(mouse_position.x) - metrics.left) as i32,
            anchor_y: (f32::from(mouse_position.y) - metrics.top) as i32,
        })
    }

    pub(crate) fn open_terminal_search(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(key) = self.active_terminal_key(cx) else {
            return false;
        };
        // If already open on this same terminal, just keep state.
        if self
            .terminal_search
            .as_ref()
            .is_some_and(|state| state.key == key)
        {
            cx.notify();
            return true;
        }
        self.terminal_search = Some(TerminalSearchState {
            key,
            query: String::new(),
            matches: Vec::new(),
            current_index: 0,
        });
        cx.notify();
        true
    }

    pub(crate) fn close_terminal_search(&mut self, cx: &mut Context<Self>) -> bool {
        if self.terminal_search.take().is_some() {
            cx.notify();
            true
        } else {
            false
        }
    }

    /// Re-run the scrollback search with the current `query` and reset
    /// the selected match to the closest one to the visible viewport.
    /// Called whenever the query changes.
    fn refresh_terminal_search_results(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.terminal_search.as_ref() else {
            return;
        };
        let key = state.key.clone();
        let query = state.query.clone();
        let Some(runtime) = self.live_terminal_runtimes.get(&key) else {
            return;
        };
        let mut matches = runtime.search_scrollback(&query);
        // Sort top-to-bottom (most-negative line first), left-to-right
        // so prev/next traversal is deterministic regardless of the
        // scan order chosen by `search_scrollback_in_term`.
        matches.sort_by_key(|m| (m.line, m.start_col));
        // Now pick a starting index against the sorted list — the first
        // match at or below the current viewport top so the initial
        // selection feels local to where the user was looking.
        let display_offset = runtime.display_offset() as i32;
        let top_grid_line = -display_offset - runtime.screen_lines() as i32 + 1;
        let mut current_index = 0;
        for (idx, m) in matches.iter().enumerate() {
            if m.line >= top_grid_line {
                current_index = idx;
                break;
            }
        }
        if let Some(state) = self.terminal_search.as_mut() {
            state.matches = matches;
            state.current_index = current_index.min(state.matches.len().saturating_sub(1));
        }
        self.scroll_to_current_search_match(cx);
        cx.notify();
    }

    fn scroll_to_current_search_match(&mut self, _cx: &mut Context<Self>) {
        let Some(state) = self.terminal_search.as_ref() else {
            return;
        };
        let Some(target) = state.matches.get(state.current_index).copied() else {
            return;
        };
        let key = state.key.clone();
        if let Some(runtime) = self.live_terminal_runtimes.get_mut(&key) {
            if runtime.scroll_to_match(&target) {
                let snapshot = runtime.snapshot();
                self.terminal_surface_snapshots.insert(key, snapshot);
            }
        }
    }

    pub(crate) fn terminal_search_input(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(state) = self.terminal_search.as_mut() else {
            return false;
        };
        state.query.push_str(text);
        self.refresh_terminal_search_results(cx);
        true
    }

    pub(crate) fn terminal_search_backspace(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(state) = self.terminal_search.as_mut() else {
            return false;
        };
        if state.query.pop().is_none() {
            return false;
        }
        self.refresh_terminal_search_results(cx);
        true
    }

    pub(crate) fn terminal_search_advance(
        &mut self,
        forward: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(state) = self.terminal_search.as_mut() else {
            return false;
        };
        if state.matches.is_empty() {
            return false;
        }
        let len = state.matches.len();
        state.current_index = if forward {
            (state.current_index + 1) % len
        } else {
            (state.current_index + len - 1) % len
        };
        self.scroll_to_current_search_match(cx);
        cx.notify();
        true
    }

    /// Project grid-coordinate matches onto the visible viewport.
    /// Returned tuples are `(viewport_line, start_col, end_col, is_current)`
    /// — only matches that overlap the viewport are emitted.
    pub(crate) fn terminal_search_viewport_highlights(
        &self,
        key: &TerminalRuntimeKey,
    ) -> Vec<(usize, usize, usize, bool)> {
        let Some(state) = self.terminal_search.as_ref() else {
            return Vec::new();
        };
        if state.key != *key || state.matches.is_empty() {
            return Vec::new();
        }
        let Some(runtime) = self.live_terminal_runtimes.get(key) else {
            return Vec::new();
        };
        let display_offset = runtime.display_offset() as i32;
        let screen_lines = runtime.screen_lines() as i32;
        let mut out = Vec::new();
        for (idx, m) in state.matches.iter().enumerate() {
            let viewport_row = m.line + screen_lines - 1 + display_offset;
            if viewport_row < 0 || viewport_row >= screen_lines {
                continue;
            }
            out.push((
                viewport_row as usize,
                m.start_col,
                m.end_col,
                idx == state.current_index,
            ));
        }
        out
    }

    /// Returns 0.0..=1.0: 1.0 immediately after the bell rings, fading to
    /// 0 at `BELL_FLASH_DURATION`. Used by the renderer to draw a
    /// translucent overlay.
    pub(crate) fn terminal_bell_intensity(&self, key: &TerminalRuntimeKey) -> f32 {
        let Some(at) = self.terminal_bell_at.get(key) else {
            return 0.0;
        };
        let elapsed = at.elapsed();
        if elapsed >= BELL_FLASH_DURATION {
            return 0.0;
        }
        1.0 - (elapsed.as_millis() as f32 / BELL_FLASH_DURATION.as_millis() as f32)
    }

    pub(crate) fn handle_terminal_search_key_down(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_search.is_none() {
            return;
        }
        cx.stop_propagation();
        let mods = ev.keystroke.modifiers;
        match ev.keystroke.key.as_str() {
            "escape" => {
                self.close_terminal_search(cx);
            }
            "enter" => {
                self.terminal_search_advance(!mods.shift, cx);
            }
            "backspace" => {
                self.terminal_search_backspace(cx);
            }
            "tab" => {}
            _ => {
                if mods.platform && ev.keystroke.key.as_str() == "v" {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        self.terminal_search_input(&text, cx);
                    }
                } else if mods.control || mods.platform || mods.function {
                    // Ignore other modifiers — let the global action
                    // dispatcher handle e.g. Cmd-W.
                } else if let Some(key_char) = ev.keystroke.key_char.as_deref() {
                    self.terminal_search_input(key_char, cx);
                }
            }
        }
    }

    pub(crate) fn paste_into_active_terminal(&mut self, cx: &App, text: &str) -> bool {
        let Some(key) = self.active_terminal_key(cx) else {
            return false;
        };
        let Some(runtime) = self.live_terminal_runtimes.get(&key) else {
            return false;
        };
        runtime.paste_text(text).is_ok()
    }

    pub(crate) fn handle_clipboard_paste(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };

        if let Some(image) = Self::clipboard_image(&item) {
            let pasted_path = Self::write_clipboard_image_to_tempfile(&image).and_then(|path| {
                let path_str = path.to_string_lossy().into_owned();
                self.paste_into_active_terminal(cx, &path_str)
                    .then_some(path_str)
            });

            if pasted_path.is_some() {
                self.show_pasted_image_preview(image, cx);
                cx.stop_propagation();
                cx.notify();
                return true;
            }
        }

        if let Some(text) = item.text() {
            if self.paste_into_active_terminal(cx, &text) {
                cx.stop_propagation();
                return true;
            }
        }

        let Some(image) = Self::clipboard_image(&item) else {
            return false;
        };

        self.show_pasted_image_preview(image, cx);
        cx.stop_propagation();
        cx.notify();
        true
    }

    fn clipboard_image(item: &ClipboardItem) -> Option<Image> {
        item.entries().iter().find_map(|entry| match entry {
            ClipboardEntry::Image(image) => Some(image.clone()),
            ClipboardEntry::String(_) | ClipboardEntry::ExternalPaths(_) => None,
        })
    }

    fn write_clipboard_image_to_tempfile(image: &Image) -> Option<std::path::PathBuf> {
        let extension = Self::image_file_extension(image.format);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        let filename = format!("another-one-paste-{}-{}.{}", nanos, image.id(), extension);
        let path = std::env::temp_dir().join(filename);
        std::fs::write(&path, &image.bytes).ok()?;
        Some(path)
    }

    fn image_file_extension(format: gpui::ImageFormat) -> &'static str {
        match format {
            gpui::ImageFormat::Png => "png",
            gpui::ImageFormat::Jpeg => "jpg",
            gpui::ImageFormat::Webp => "webp",
            gpui::ImageFormat::Gif => "gif",
            gpui::ImageFormat::Svg => "svg",
            gpui::ImageFormat::Bmp => "bmp",
            gpui::ImageFormat::Tiff => "tiff",
            gpui::ImageFormat::Ico => "ico",
        }
    }

    fn show_pasted_image_preview(&mut self, image: Image, cx: &mut App) {
        self.clear_pasted_image_preview(cx);

        let now = Instant::now();
        self.pasted_image_preview = Some(PastedImagePreview {
            image: Arc::new(image),
            shown_at: now,
            dismiss_at: now + PASTED_IMAGE_PREVIEW_LIFETIME,
        });
    }

    pub(crate) fn terminal_selection_for(
        &self,
        key: &TerminalRuntimeKey,
    ) -> Option<TerminalSelectionRange> {
        let selection = self.terminal_selection.as_ref()?;
        if selection.key != *key {
            return None;
        }

        terminal_selection_range(selection.anchor, selection.head)
    }

    fn terminal_panel_metrics_for_key(
        &self,
        key: &TerminalRuntimeKey,
        window: &mut Window,
    ) -> Option<TerminalPanelMetrics> {
        let snapshot = self.terminal_surface_snapshots.get(key)?;
        let titlebar_height = if crate::platform::CurrentPlatform::supports_custom_chrome(window) {
            TITLEBAR_CHROME_H
        } else {
            0.0
        };

        Some(TerminalPanelMetrics {
            key: key.clone(),
            left: self.sidebar_w + GUTTER,
            top: titlebar_height + TERMINAL_TAB_BAR_H,
            padding: TERMINAL_VIEW_PADDING,
            cell_width: f32::from(terminal_cell_width(window, self.font_size)),
            cell_height: (self.font_size * TERMINAL_LINE_HEIGHT_RATIO).max(14.0),
            columns: snapshot.columns,
            rows: snapshot.lines.len(),
        })
    }

    /// Forward a mouse event to the application running in the terminal
    /// when it has enabled xterm mouse-tracking (vim, htop, tmux, …).
    /// Returns `true` if the event was consumed — callers should then skip
    /// native selection / link handling and stop propagation.
    ///
    /// xterm convention: holding `shift` overrides the application's mouse
    /// mode so the user can still drag to select text. We honor that here.
    pub(crate) fn forward_terminal_mouse_event(
        &mut self,
        key: &TerminalRuntimeKey,
        button: TerminalMouseButton,
        action: TerminalMouseAction,
        position: gpui::Point<Pixels>,
        modifiers: gpui::Modifiers,
        window: &mut Window,
    ) -> bool {
        if modifiers.shift {
            return false;
        }
        let Some(runtime) = self.live_terminal_runtimes.get(key) else {
            return false;
        };
        let Some(protocol) = runtime.mouse_protocol() else {
            return false;
        };
        let Some(metrics) = self.terminal_panel_metrics_for_key(key, window) else {
            return false;
        };
        let Some(cell_position) = terminal_cell_position_from_mouse(position, &metrics) else {
            return false;
        };
        if terminal_open_link_modifier_held(modifiers)
            && self
                .terminal_surface_snapshots
                .get(key)
                .and_then(|snapshot| terminal_link_at_position(snapshot, cell_position))
                .is_some()
        {
            return false;
        }
        let mods = TerminalMouseModifiers {
            shift: modifiers.shift,
            alt: modifiers.alt,
            control: modifiers.control,
        };
        let Some(payload) = encode_terminal_mouse_event(
            protocol,
            button,
            action,
            cell_position.column,
            cell_position.line,
            mods,
        ) else {
            return false;
        };
        if let Err(err) = runtime.write_input(&payload) {
            tracing::warn!(
                target: "another_one::terminal",
                error = %err,
                key = ?key,
                "failed to forward mouse event to terminal — falling back to local handling"
            );
            return false;
        }
        true
    }

    pub(crate) fn start_terminal_selection(
        &mut self,
        key: TerminalRuntimeKey,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(metrics) = self.terminal_panel_metrics_for_key(&key, window) else {
            self.terminal_selection = None;
            return false;
        };
        let Some(position) = terminal_cell_position_from_mouse(ev.position, &metrics) else {
            self.terminal_selection = None;
            return false;
        };
        let selection_range = match ev.click_count {
            0 | 1 => None,
            2 => self
                .terminal_surface_snapshots
                .get(&key)
                .and_then(|snapshot| terminal_word_selection_range(snapshot, position)),
            _ => self
                .terminal_surface_snapshots
                .get(&key)
                .and_then(|snapshot| terminal_line_selection_range(snapshot, position)),
        };

        self.terminal_selection = if let Some(selection) = selection_range {
            Some(TerminalSelectionState {
                key: metrics.key,
                anchor: TerminalCellPosition {
                    line: selection.start_line,
                    column: selection.start_column,
                },
                head: TerminalCellPosition {
                    line: selection.end_line,
                    column: selection.end_column,
                },
                dragging: false,
                autoscroll_dir: 0,
            })
        } else {
            Some(TerminalSelectionState {
                key: metrics.key,
                anchor: position,
                head: position,
                dragging: true,
                autoscroll_dir: 0,
            })
        };
        cx.notify();
        true
    }

    /// Open the right-click context menu over the terminal pane. Called
    /// only when `forward_terminal_mouse_event` declined (i.e. mouse mode
    /// off, or shift held). Captures any selection + link target at click
    /// time so the menu items remain meaningful even if the user moves
    /// the pointer before choosing.
    pub(crate) fn open_terminal_context_menu(
        &mut self,
        key: &TerminalRuntimeKey,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let metrics = self.terminal_panel_metrics_for_key(key, window);
        let link = metrics.as_ref().and_then(|metrics| {
            let cell = terminal_cell_position_from_mouse(position, metrics)?;
            self.terminal_surface_snapshots
                .get(key)
                .and_then(|snapshot| terminal_link_at_position(snapshot, cell))
        });
        let selected_text = self.terminal_selection_for(key).and_then(|selection| {
            self.terminal_surface_snapshots
                .get(key)
                .and_then(|snapshot| terminal_selected_text(snapshot, selection))
        });
        let state = TerminalContextMenuState {
            key: key.clone(),
            anchor_x: f32::from(position.x),
            anchor_y: f32::from(position.y),
            link,
            selected_text,
        };
        // The right-click handler runs INSIDE WorkspacePane's update
        // lock (listeners hold their entity locked for the body),
        // and we're called via `this.app.update(cx, …)` reaching
        // back into AnotherOneApp. A direct `workspace_pane.update`
        // here would be a second lock on WorkspacePane and panic
        // GPUI's `double_lease_panic`. `cx.defer` defers the inner
        // update to after the listener's lock releases.
        let workspace_handle = self.workspace_pane.clone();
        cx.defer(move |cx| {
            workspace_handle.update(cx, |workspace, cx| {
                // Mutually exclusive with the tab-pin menu — opening
                // one implicitly dismisses the other so they never
                // stack.
                workspace.terminal_tab_menu = None;
                workspace.terminal_context_menu = Some(state);
                cx.notify();
            });
        });
    }

    /// Dismiss the terminal context menu without taking action.
    pub(crate) fn dismiss_terminal_context_menu(&mut self, cx: &mut Context<Self>) {
        self.workspace_pane.update(cx, |workspace, cx| {
            if workspace.terminal_context_menu.take().is_some() {
                cx.notify();
            }
        });
    }

    /// Copy the captured selection text to the clipboard, then dismiss.
    pub(crate) fn terminal_context_menu_copy(&mut self, cx: &mut Context<Self>) -> bool {
        let text = self
            .workspace_pane
            .read(cx)
            .terminal_context_menu
            .as_ref()
            .and_then(|menu| menu.selected_text.clone());
        let Some(text) = text else {
            self.dismiss_terminal_context_menu(cx);
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.dismiss_terminal_context_menu(cx);
        true
    }

    /// Paste current clipboard text into the terminal that owned the menu.
    pub(crate) fn terminal_context_menu_paste(&mut self, cx: &mut Context<Self>) -> bool {
        let key = self
            .workspace_pane
            .read(cx)
            .terminal_context_menu
            .as_ref()
            .map(|menu| menu.key.clone());
        let Some(key) = key else {
            return false;
        };
        let Some(item) = cx.read_from_clipboard() else {
            self.dismiss_terminal_context_menu(cx);
            return false;
        };
        let Some(text) = item.text() else {
            self.dismiss_terminal_context_menu(cx);
            return false;
        };
        let pasted = self
            .live_terminal_runtimes
            .get(&key)
            .map(|runtime| runtime.paste_text(&text).is_ok())
            .unwrap_or(false);
        self.dismiss_terminal_context_menu(cx);
        pasted
    }

    /// Open the link captured at menu-open time via the OS handler.
    pub(crate) fn terminal_context_menu_open_link(&mut self, cx: &mut Context<Self>) -> bool {
        let link = self
            .workspace_pane
            .read(cx)
            .terminal_context_menu
            .as_ref()
            .and_then(|menu| menu.link.clone());
        let Some(link) = link else {
            self.dismiss_terminal_context_menu(cx);
            return false;
        };
        if let Err(err) = crate::platform::CurrentPlatform::open_external_url(&link) {
            self.show_error_toast(err, cx);
        }
        self.dismiss_terminal_context_menu(cx);
        true
    }

    pub(crate) fn open_terminal_link_at_click(
        &mut self,
        key: &TerminalRuntimeKey,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !terminal_open_link_modifier_held(ev.modifiers) {
            return false;
        }

        let Some(metrics) = self.terminal_panel_metrics_for_key(key, window) else {
            return false;
        };
        let Some(position) = terminal_cell_position_from_mouse(ev.position, &metrics) else {
            return false;
        };
        let Some(link) = self
            .terminal_surface_snapshots
            .get(key)
            .and_then(|snapshot| terminal_link_at_position(snapshot, position))
        else {
            return false;
        };

        self.terminal_selection = None;
        if let Err(err) = crate::platform::CurrentPlatform::open_external_url(&link) {
            self.show_error_toast(err, cx);
        }
        cx.notify();
        true
    }

    fn update_terminal_selection_drag(
        &mut self,
        ev: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(selection) = self.terminal_selection.as_ref() else {
            return false;
        };
        if !selection.dragging || !ev.dragging() {
            return false;
        }

        let selection_key = selection.key.clone();

        let Some(metrics) = self.terminal_panel_metrics_for_key(&selection_key, window) else {
            self.terminal_selection = None;
            cx.notify();
            return true;
        };
        if selection.key != metrics.key {
            if let Some(selection) = self.terminal_selection.as_mut() {
                selection.dragging = false;
                selection.autoscroll_dir = 0;
            }
            return false;
        }

        // Edge auto-scroll: if the pointer is above or below the
        // viewport content area, mark the direction so the refresh
        // tick keeps scrolling while the drag continues — even if
        // the pointer stays still. Inside the viewport we cancel.
        let content_top = metrics.top + metrics.padding;
        let content_bottom = content_top + metrics.cell_height * (metrics.rows as f32);
        let mouse_y = f32::from(ev.position.y);
        // Velocity in lines-per-tick scales with overshoot distance
        // measured in cell heights. 0 cells past = 1 line/tick (just
        // entered the edge), grows roughly linearly, capped so a
        // user dragging way off-screen doesn't fly through 1000
        // lines in a single frame.
        let autoscroll_dir = if mouse_y < content_top {
            let cells_past = ((content_top - mouse_y) / metrics.cell_height.max(1.0)).ceil();
            (cells_past as i32).clamp(1, 16)
        } else if mouse_y > content_bottom {
            let cells_past = ((mouse_y - content_bottom) / metrics.cell_height.max(1.0)).ceil();
            -(cells_past as i32).clamp(1, 16)
        } else {
            0
        };

        let position = terminal_cell_position_from_mouse(ev.position, &metrics);
        let Some(selection) = self.terminal_selection.as_mut() else {
            return false;
        };
        selection.autoscroll_dir = autoscroll_dir;

        // Inside the viewport: track the cursor cell normally.
        // Outside (autoscroll engaged): pin the head to the boundary
        // row using the cursor's clamped column so the highlighted
        // band reaches the edge while the tick scrolls more in.
        let new_head = if autoscroll_dir == 0 {
            match position {
                Some(p) => p,
                None => return true,
            }
        } else {
            let column = position.map(|p| p.column).unwrap_or(0);
            let line = if autoscroll_dir > 0 {
                0
            } else {
                metrics.rows.saturating_sub(1)
            };
            TerminalCellPosition { line, column }
        };
        if selection.head == new_head {
            return true;
        }
        selection.head = new_head;
        cx.notify();
        true
    }

    /// Tick the auto-scroll while the user holds a drag past the
    /// viewport edge. Called from the refresh timer. Scrolls the
    /// terminal one line in the marked direction and shifts the
    /// anchor row by the same amount so the selection keeps tracking
    /// the original grid content. Returns true if anything changed.
    pub(crate) fn drain_terminal_drag_autoscroll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(selection) = self.terminal_selection.as_ref() else {
            return false;
        };
        if !selection.dragging || selection.autoscroll_dir == 0 {
            return false;
        }
        let key = selection.key.clone();
        let velocity = selection.autoscroll_dir;
        let dir = velocity.signum();

        let Some(runtime) = self.live_terminal_runtimes.get_mut(&key) else {
            return false;
        };
        if !runtime.scroll_display(velocity) {
            // Hit the top/bottom of scrollback; nothing more to do
            // this tick. Selection state is unchanged so the next
            // tick will retry — cheap and harmless.
            return false;
        }
        let snapshot = runtime.snapshot();
        self.terminal_surface_snapshots
            .insert(key.clone(), snapshot);

        if let Some(selection) = self.terminal_selection.as_mut() {
            // Same content shifts +1 viewport row when we scroll up
            // by 1, and -1 when scrolling down. Track the anchor by
            // the same delta so the highlighted region keeps the
            // original grid content under it. Clamp to the viewport
            // so off-screen anchors degrade to "selection from the
            // visible edge" rather than panicking.
            let rows = self
                .terminal_surface_snapshots
                .get(&key)
                .map(|snap| snap.lines.len())
                .unwrap_or(0);
            let max_line = rows.saturating_sub(1);
            let shifted = (selection.anchor.line as i32) + velocity;
            selection.anchor.line = shifted.clamp(0, max_line as i32) as usize;
            // Pin head to the leading edge so the band keeps reaching
            // the boundary the user is dragging past.
            selection.head.line = if dir > 0 { 0 } else { max_line };
        }

        cx.notify();
        true
    }

    fn finish_terminal_selection_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(selection) = self.terminal_selection.as_mut() else {
            return false;
        };
        if !selection.dragging {
            return false;
        }

        selection.dragging = false;
        selection.autoscroll_dir = 0;
        if selection.anchor == selection.head {
            self.terminal_selection = None;
        }
        cx.notify();
        true
    }

    pub(crate) fn selected_terminal_text(&self, cx: &App) -> Option<String> {
        let key = self.active_terminal_key(cx)?;
        let selection = self.terminal_selection_for(&key)?;
        let snapshot = self.terminal_surface_snapshots.get(&key)?;
        terminal_selected_text(snapshot, selection)
    }

    pub(crate) fn scroll_terminal(
        &mut self,
        key: &TerminalRuntimeKey,
        delta: ScrollDelta,
        cx: &mut Context<Self>,
    ) -> bool {
        let line_height = px((self.font_size * TERMINAL_LINE_HEIGHT_RATIO).max(14.0));
        let remainder_lines = self
            .terminal_scroll_remainder_lines
            .get(key)
            .copied()
            .unwrap_or(0.0);
        let (lines, remainder_lines) = terminal_scroll_lines(delta, line_height, remainder_lines);

        if lines == 0 {
            self.terminal_scroll_remainder_lines
                .insert(key.clone(), remainder_lines);
            return false;
        }

        self.terminal_scroll_remainder_lines
            .insert(key.clone(), remainder_lines);

        let Some(runtime) = self.live_terminal_runtimes.get_mut(key) else {
            return false;
        };
        if !runtime.scroll_display(lines) {
            return false;
        }

        self.terminal_surface_snapshots
            .insert(key.clone(), runtime.snapshot());
        if self
            .terminal_selection
            .as_ref()
            .is_some_and(|selection| selection.key == *key)
        {
            self.terminal_selection = None;
        }
        cx.notify();
        true
    }

    fn remove_persisted_sections(&mut self, section_ids: &HashSet<SectionId>) {
        let bare_section_keys = section_ids
            .iter()
            .filter(|section_id| section_id.task_id.is_none())
            .map(SectionId::store_key)
            .collect::<HashSet<_>>();
        if !bare_section_keys.is_empty() {
            self.project_store
                .remove_terminal_sections(&bare_section_keys);
        }
    }

    fn cleanup_removed_tab(&mut self, section_id: &SectionId, tab_id: String) {
        // GUI-driven close (Ctrl-W, click X, modal-confirm path) lands
        // here regardless of the originating call site. Fire a
        // ClientEvent::TabClosed attributed to the GUI so MCP
        // observers can mirror tab-close on the bus, parity with
        // closes initiated through the MCP `close_tab` tool.
        self.emit_client_event(ClientEvent::TabClosed {
            originator: ClientId::gui_desktop(),
            tab_id: tab_id.clone(),
        });
        let key = TerminalRuntimeKey {
            section_id: section_id.clone(),
            tab_id,
        };
        if self
            .terminal_selection
            .as_ref()
            .is_some_and(|selection| selection.key == key)
        {
            self.terminal_selection = None;
        }
        self.terminal_scroll_remainder_lines.remove(&key);
        self.terminal_manager.processes.remove(&key);
        self.cancel_prewarmed_launch_for_tab(&key);
        self.unregister_tab_from_registry(&key);
        if let Some(mut runtime) = remove_terminal_runtime_state(
            &mut self.live_terminal_runtimes,
            &mut self.terminal_surface_snapshots,
            &mut self.terminal_manager.pending_launches,
            &mut self.terminal_manager.recent_output,
            &mut self.terminal_manager.errors,
            &key,
        ) {
            runtime.kill();
        }
    }

    fn cleanup_removed_sections(&mut self, section_ids: &HashSet<SectionId>) {
        let runtime_keys = self
            .live_terminal_runtimes
            .keys()
            .filter(|key| section_ids.contains(&key.section_id))
            .cloned()
            .collect::<Vec<_>>();
        for key in runtime_keys {
            self.cleanup_removed_tab(&key.section_id, key.tab_id.clone());
        }
        self.remove_persisted_sections(section_ids);
    }

    pub(crate) fn sync_workspace_layout(&mut self, cx: &mut Context<Self>) {
        let sidebar_w = self.sidebar_w;
        let right_w = self.right_w;
        let font_size = self.font_size;
        self.workspace_pane.update(cx, |workspace, cx| {
            workspace.sync_layout(sidebar_w, right_w, font_size, cx);
        });
    }

    pub(crate) fn mark_git_refresh_stale(&mut self) {
        self.git_workspace.mark_stale(
            ACTIVE_GIT_STATUS_REFRESH_INTERVAL,
            ACTIVE_GIT_METADATA_REFRESH_INTERVAL,
        );
    }

    pub(crate) fn begin_add_project(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        if self.project_add_receiver.is_some() {
            self.show_info_toast("A project is already being added.", cx);
            return;
        }

        let project_label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        self.show_info_toast(format!("Adding {}...", project_label), cx);

        self.project_add_receiver =
            Some(another_one_core::project_service::spawn_project_add(path));
        cx.notify();
    }

    fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.font_size = (self.font_size + 1.0).min(32.0);
        self.sync_workspace_layout(cx);
        cx.notify();
    }

    fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.font_size = (self.font_size - 1.0).max(8.0);
        self.sync_workspace_layout(cx);
        cx.notify();
    }

    fn zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.font_size = 13.0;
        self.sync_workspace_layout(cx);
        cx.notify();
    }

    fn handle_terminal_find(
        &mut self,
        _: &TerminalFind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open_terminal_search(cx) {
            // Pull keyboard focus onto the search overlay so the next
            // keystrokes feed the query input, not the underlying TUI.
            self.focus_handle.focus(window, cx);
            cx.stop_propagation();
        }
    }

    fn handle_terminal_search_close(
        &mut self,
        _: &TerminalSearchClose,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.close_terminal_search(cx) {
            cx.stop_propagation();
        }
    }

    fn handle_terminal_search_next(
        &mut self,
        _: &TerminalSearchNext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_search_advance(true, cx) {
            cx.stop_propagation();
        }
    }

    fn handle_terminal_search_prev(
        &mut self,
        _: &TerminalSearchPrev,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_search_advance(false, cx) {
            cx.stop_propagation();
        }
    }

    fn next_tab(&mut self, _: &NextTab, _: &mut Window, cx: &mut Context<Self>) {
        if self.navigate_tab_shortcut(NavigationDirection::Next, cx) {
            cx.stop_propagation();
        }
    }

    fn previous_tab(&mut self, _: &PreviousTab, _: &mut Window, cx: &mut Context<Self>) {
        if self.navigate_tab_shortcut(NavigationDirection::Previous, cx) {
            cx.stop_propagation();
        }
    }

    fn next_task(&mut self, _: &NextTask, _: &mut Window, cx: &mut Context<Self>) {
        if self.navigate_task_shortcut(NavigationDirection::Next, cx) {
            cx.stop_propagation();
        }
    }

    fn previous_task(&mut self, _: &PreviousTask, _: &mut Window, cx: &mut Context<Self>) {
        if self.navigate_task_shortcut(NavigationDirection::Previous, cx) {
            cx.stop_propagation();
        }
    }

    fn new_tab(&mut self, _: &NewTab, _: &mut Window, cx: &mut Context<Self>) {
        if self.open_new_tab_shortcut(cx) {
            cx.stop_propagation();
        }
    }

    fn new_task(&mut self, _: &NewTask, _: &mut Window, cx: &mut Context<Self>) {
        if self.open_new_task_shortcut(cx) {
            cx.stop_propagation();
        }
    }

    fn next_project(&mut self, _: &NextProject, _: &mut Window, cx: &mut Context<Self>) {
        if self.navigate_project_shortcut(cx) {
            cx.stop_propagation();
        }
    }

    fn navigation_shortcuts_blocked(&self, cx: &App) -> bool {
        self.settings_open
            || self.new_task_modal.is_some()
            || self.add_agent_modal.is_some()
            || self.sidebar_task_rename.is_some()
            || self.sidebar_task_menu.is_some()
            || self.workspace_pane.read(cx).terminal_tab_menu.is_some()
            || self.workspace_pane.read(cx).terminal_context_menu.is_some()
            || self.terminal_search.is_some()
            || self
                .workspace_pane
                .read(cx)
                .pinned_tab_close_confirm
                .is_some()
            || self.sidebar_task_delete_confirm.is_some()
            || self.project_remove_confirm.is_some()
            || self.discard_confirm.is_some()
    }

    pub(crate) fn navigate_tab_shortcut(
        &mut self,
        direction: NavigationDirection,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.navigation_shortcuts_blocked(cx) {
            return false;
        }

        let (previous_section, previous_active_project_page, previous_active_tab, targets) = {
            let workspace = self.workspace_pane.read(cx);
            let active_section = workspace.active_section.clone();
            let active_project_page = workspace.active_project_page.clone();
            let active_tab = active_section.as_ref().and_then(|section_id| {
                workspace
                    .section_states
                    .get(section_id)
                    .map(|state| state.active_tab)
            });
            let targets = global_tab_navigation_targets(
                &self.project_store.projects,
                &self.project_store.tasks,
                &self.project_store.ui.pinned_task_ids,
                &workspace.section_states,
            );

            (active_section, active_project_page, active_tab, targets)
        };

        let Some(target) = next_global_tab_navigation_target(
            &targets,
            &self.project_store.projects,
            previous_section.as_ref(),
            previous_active_project_page.as_deref(),
            previous_active_tab,
            direction,
        )
        .cloned() else {
            return false;
        };

        let activated = self.workspace_pane.update(cx, |workspace, cx| {
            workspace.activate_tab(&target.section_id, target.tab_index, cx)
        });

        if activated && previous_section.as_ref() != Some(&target.section_id) {
            self.mark_git_refresh_stale();
        }

        activated
    }

    pub(crate) fn navigate_task_shortcut(
        &mut self,
        direction: NavigationDirection,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.navigation_shortcuts_blocked(cx) {
            return false;
        }

        let targets = sidebar_task_navigation_targets(
            &self.project_store.projects,
            &self.project_store.tasks,
            &self.project_store.ui.pinned_task_ids,
        );
        let (active_section, active_project_page) = {
            let workspace = self.workspace_pane.read(cx);
            (
                workspace.active_section.clone(),
                workspace.active_project_page.clone(),
            )
        };
        let Some(target) = next_task_navigation_target(
            &targets,
            &self.project_store.projects,
            active_section.as_ref(),
            active_project_page.as_deref(),
            direction,
        )
        .cloned() else {
            return false;
        };

        if active_section
            .as_ref()
            .and_then(|section| section.task_id.as_deref())
            == Some(target.task_id.as_str())
        {
            return false;
        }

        let section_id =
            SectionId::for_task(&target.project_id, &target.branch_name, &target.task_id);
        let project_path = target.project_path.clone();
        self.workspace_pane.update(cx, |workspace, cx| {
            workspace.activate_section(section_id.clone(), Some(project_path.clone()), None, cx);
        });
        self.prefetch_section_pull_request_and_checks(&section_id, &project_path);
        self.mark_git_refresh_stale();
        true
    }

    pub(crate) fn open_new_tab_shortcut(&mut self, cx: &mut Context<Self>) -> bool {
        if self.navigation_shortcuts_blocked(cx) {
            return false;
        }

        let shortcut_target = {
            let workspace = self.workspace_pane.read(cx);
            workspace.active_section.clone().map(|section_id| {
                let selected_agent_id = new_tab_seed_agent_id(
                    workspace.section_states.get(&section_id),
                    self.default_agent_id(),
                );
                (section_id, selected_agent_id)
            })
        };

        let Some((section_id, selected_agent_id)) = shortcut_target else {
            self.show_error_toast("Open a task before creating a new tab.", cx);
            return true;
        };

        self.open_add_agent_modal(section_id, selected_agent_id, cx);
        cx.notify();
        true
    }

    pub(crate) fn open_new_task_shortcut(&mut self, cx: &mut Context<Self>) -> bool {
        if self.navigation_shortcuts_blocked(cx) {
            return false;
        }

        let target = {
            let workspace = self.workspace_pane.read(cx);
            resolve_new_task_shortcut_target(
                workspace.active_section.as_ref(),
                workspace.active_project_page.as_deref(),
                |task_id| {
                    self.project_store
                        .task(task_id)
                        .map(|task| task.root_project_id.clone())
                },
            )
        };

        let Some(target) = target else {
            self.show_error_toast("Open a project or task before creating a new task.", cx);
            return true;
        };

        if let Some(source_branch) = target.source_branch.as_deref() {
            self.open_new_task_modal_with_branch(&target.project_id, source_branch, cx);
        } else {
            self.open_new_task_modal(&target.project_id, cx);
        }
        cx.notify();
        true
    }

    pub(crate) fn close_active_tab_shortcut(&mut self, cx: &mut Context<Self>) -> bool {
        if self.workspace_pane.read(cx).active_git_diff.is_some() {
            self.git_diff_receiver = None;
            self.git_diff_state = None;
            self.workspace_pane.update(cx, |workspace, cx| {
                workspace.active_git_diff = None;
                workspace.keyboard_focus = WorkspaceKeyboardFocus::MainPane;
                cx.notify();
            });
            cx.notify();
            return true;
        }

        if self.navigation_shortcuts_blocked(cx) {
            return false;
        }

        let active_target = {
            let workspace = self.workspace_pane.read(cx);
            workspace.active_section.clone().and_then(|section_id| {
                workspace
                    .section_states
                    .get(&section_id)
                    .map(|state| (section_id, state.active_tab, state.tabs.len()))
            })
        };

        let Some((section_id, active_tab, tab_count)) = active_target else {
            return false;
        };

        if tab_count == 0 {
            return false;
        }

        self.workspace_pane.update(cx, |workspace, cx| {
            workspace.request_close_tab(&section_id, active_tab, cx)
        });
        true
    }

    pub(crate) fn navigate_project_shortcut(&mut self, cx: &mut Context<Self>) -> bool {
        if self.navigation_shortcuts_blocked(cx) {
            return false;
        }

        let target_project_id = {
            let workspace = self.workspace_pane.read(cx);
            next_project_navigation_target(
                &root_project_navigation_targets(&self.project_store.projects),
                &self.project_store.projects,
                workspace.active_section.as_ref(),
                workspace.active_project_page.as_deref(),
            )
            .map(str::to_string)
        };

        let Some(project_id) = target_project_id else {
            self.show_error_toast("Open or add a project before cycling projects.", cx);
            return true;
        };

        self.workspace_pane.update(cx, |workspace, cx| {
            workspace.activate_project_page(project_id.clone(), cx);
        });
        true
    }

    fn git_status_refresh_interval(&self) -> Duration {
        ACTIVE_GIT_STATUS_REFRESH_INTERVAL
    }

    fn apply_project_git_state(&mut self, project_id: &str, state: ProjectGitState) -> bool {
        let mut changed = false;

        let ProjectGitState {
            changed_files,
            ahead_count,
            behind_count,
            metadata,
            current_branch,
        } = state;

        if let Some(metadata) = metadata {
            let repo_id = self
                .project_store
                .project(project_id)
                .map(|project| project.repo_id.clone());
            if let Some(repo_id) = repo_id {
                if let Some(repo) = self.project_store.repo_mut(&repo_id) {
                    if repo.branch_order != metadata.branch_order {
                        repo.branch_order = metadata.branch_order.clone();
                        changed = true;
                    }
                    if repo.branches_by_name != metadata.branches_by_name {
                        repo.branches_by_name = metadata.branches_by_name.clone();
                        changed = true;
                    }
                    if repo.common_dir != metadata.common_dir {
                        repo.common_dir = metadata.common_dir.clone();
                        changed = true;
                    }
                }
            }
            if let Some(project) = self.project_store.project_mut(project_id) {
                if project.kind != metadata.kind {
                    project.kind = metadata.kind;
                    changed = true;
                }
                if project.checkout != metadata.checkout {
                    project.checkout = metadata.checkout;
                    changed = true;
                }
            }
        }

        if self
            .project_store
            .project(project_id)
            .and_then(|project| project.checkout.current_branch.as_deref())
            != current_branch.as_deref()
        {
            if let Some(project) = self.project_store.project_mut(project_id) {
                project.checkout.current_branch = current_branch.clone();
                project.checkout.lines_added = 0;
                project.checkout.lines_removed = 0;
                changed = true;
            }
        }

        let repo_id = self
            .project_store
            .project(project_id)
            .map(|project| project.repo_id.clone());
        if let Some(repo_id) = repo_id {
            if let Some(repo) = self.project_store.repo_mut(&repo_id) {
                if let Some(branch_name) = current_branch.as_deref() {
                    if let Some(branch) = repo.branches_by_name.get_mut(branch_name) {
                        if branch.ahead_count != ahead_count {
                            branch.ahead_count = ahead_count;
                            changed = true;
                        }
                        if branch.behind_count != behind_count {
                            branch.behind_count = behind_count;
                            changed = true;
                        }
                    } else {
                        repo.branches_by_name.insert(
                            branch_name.to_string(),
                            RepoBranchRecord {
                                name: branch_name.to_string(),
                                last_commit_relative: String::new(),
                                is_default: false,
                                ahead_count,
                                behind_count,
                            },
                        );
                        if !repo.branch_order.iter().any(|name| name == branch_name) {
                            repo.branch_order.push(branch_name.to_string());
                        }
                        changed = true;
                    }
                }
            }
        }

        if changed {
            self.project_store.refresh_runtime_views();
        }

        if self
            .changed_files
            .get(project_id)
            .map(|files| files.as_ref())
            != Some(changed_files.as_slice())
        {
            self.changed_files_list_snapshots.remove(project_id);
            self.changed_files
                .insert(project_id.to_string(), Arc::from(changed_files));
            changed = true;
        }

        changed
    }

    fn set_branch_commit_state(
        &mut self,
        project_id: &str,
        state: Option<ProjectBranchCommitState>,
    ) -> bool {
        match state {
            Some(state) => {
                if self.branch_commit_states.get(project_id) == Some(&state) {
                    return false;
                }
                self.branch_commit_states
                    .insert(project_id.to_string(), state);
                true
            }
            None => self.branch_commit_states.remove(project_id).is_some(),
        }
    }

    fn drain_git_refresh(&mut self, cx: &mut Context<Self>) -> bool {
        match self.git_refresh_operation.poll() {
            BroadcastOperationEvent::Ready { id, reply } => {
                if !self.git_refresh_operation.complete_if_current(id) {
                    return false;
                }
                if self
                    .pending_changed_files_git_mutations
                    .contains_key(&reply.project_id)
                {
                    return false;
                }
                self.git_workspace.mark_refreshed(reply.include_metadata);
                let mut changed = self.apply_project_git_state(&reply.project_id, reply.state);
                if reply.include_metadata {
                    let invalid_settings = self
                        .project_store
                        .clear_missing_branch_settings(&reply.project_id);
                    changed |= self.handle_invalid_project_branch_settings(
                        &reply.project_id,
                        invalid_settings,
                        cx,
                    );
                }
                match reply.commit_state {
                    Some(Ok(commit_state)) => {
                        let requested_limit = commit_state.requested_limit;
                        changed |=
                            self.set_branch_commit_state(&reply.project_id, Some(commit_state));
                        if self.commit_page_size_for_project(&reply.project_id) > requested_limit {
                            self.mark_git_refresh_stale();
                        }
                    }
                    Some(Err(error)) => {
                        changed |= self.set_branch_commit_state(&reply.project_id, None);
                        self.show_warning_toast(error, cx);
                    }
                    None => {
                        changed |= self.set_branch_commit_state(&reply.project_id, None);
                    }
                }
                changed
            }
            BroadcastOperationEvent::Lagged { skipped } => {
                log::warn!("git_refresh drain lagged {skipped} messages");
                false
            }
            BroadcastOperationEvent::Idle
            | BroadcastOperationEvent::Empty
            | BroadcastOperationEvent::Closed => false,
        }
    }

    pub(crate) fn start_new_task_branch_refresh(
        &mut self,
        project_id: String,
        project_path: std::path::PathBuf,
    ) {
        self.new_task_branch_refresh_operation.start(
            another_one_core::git_service::spawn_remote_branch_refresh(project_id, project_path),
        );
    }

    fn drain_new_task_branch_refresh(&mut self, cx: &mut Context<Self>) -> bool {
        match self.new_task_branch_refresh_operation.poll() {
            BroadcastOperationEvent::Ready { id, reply } => {
                if !self
                    .new_task_branch_refresh_operation
                    .complete_if_current(id)
                {
                    return false;
                }
                match reply.result {
                    Ok(state) => {
                        let mut changed = self.apply_project_git_state(&reply.project_id, state);
                        let invalid_settings = self
                            .project_store
                            .clear_missing_branch_settings(&reply.project_id);
                        changed |= self.handle_invalid_project_branch_settings(
                            &reply.project_id,
                            invalid_settings,
                            cx,
                        );
                        changed
                    }
                    Err(error) => {
                        self.show_warning_toast(error, cx);
                        true
                    }
                }
            }
            BroadcastOperationEvent::Lagged { skipped } => {
                log::warn!("new task branch refresh drain lagged {skipped} messages");
                false
            }
            BroadcastOperationEvent::Idle
            | BroadcastOperationEvent::Empty
            | BroadcastOperationEvent::Closed => false,
        }
    }

    fn maybe_schedule_active_git_refresh(&mut self, cx: &App) {
        if self.git_refresh_operation.is_in_flight() {
            return;
        }

        let status_due = self
            .git_workspace
            .status_due(self.git_status_refresh_interval());
        let metadata_due = self
            .git_workspace
            .metadata_due(ACTIVE_GIT_METADATA_REFRESH_INTERVAL);
        if !status_due && !metadata_due {
            return;
        }

        let workspace = self.workspace_pane.read(cx);
        let Some((project_id, project_path)) = workspace
            .active_section
            .as_ref()
            .and_then(|section| {
                self.project_store
                    .projects
                    .iter()
                    .find(|project| project.id == section.project_id)
            })
            .or_else(|| {
                workspace
                    .active_project_page
                    .as_ref()
                    .and_then(|project_id| {
                        self.project_store
                            .projects
                            .iter()
                            .find(|project| project.id == *project_id)
                    })
            })
            .or_else(|| self.project_store.projects.first())
            .map(|project| (project.id.clone(), project.path.clone()))
        else {
            return;
        };

        if self
            .pending_changed_files_git_mutations
            .contains_key(&project_id)
        {
            return;
        }

        let commit_limit = if self.right_sidebar_mode == RightSidebarMode::Commits {
            workspace.active_section.as_ref().and_then(|section| {
                if section.project_id == project_id {
                    Some(self.commit_page_size_for_project(&section.project_id))
                } else {
                    None
                }
            })
        } else {
            None
        };

        let include_metadata = metadata_due;
        self.git_refresh_operation
            .start(another_one_core::git_service::spawn_refresh(
                project_id,
                project_path,
                include_metadata,
                commit_limit,
            ));
    }

    #[hotpath::measure]
    pub(crate) fn refresh_project_git_state(&mut self, project_id: &str) -> bool {
        let Some(project_path) = self.project_path(project_id) else {
            return false;
        };

        self.git_refresh_operation
            .start(another_one_core::git_service::spawn_refresh(
                project_id.to_string(),
                project_path,
                true,
                None,
            ));
        true
    }

    fn active_project_context(&self, cx: &App) -> Option<(String, std::path::PathBuf)> {
        self.workspace_pane
            .read(cx)
            .active_section
            .as_ref()
            .and_then(|section| {
                self.project_store
                    .projects
                    .iter()
                    .find(|project| project.id == section.project_id)
                    .map(|project| (project.id.clone(), project.path.clone()))
            })
            .or_else(|| {
                self.workspace_pane
                    .read(cx)
                    .active_project_page
                    .as_ref()
                    .and_then(|project_id| {
                        self.project_store
                            .projects
                            .iter()
                            .find(|project| project.id == *project_id)
                            .map(|project| (project.id.clone(), project.path.clone()))
                    })
            })
    }

    pub(crate) fn launch_task_request(
        &mut self,
        request: TaskLaunchRequest,
        cx: &mut Context<Self>,
    ) {
        match request {
            TaskLaunchRequest::Direct {
                project_id,
                task_name,
                generated_task_name,
                source_branch,
                launch_config,
                warm_launch_id,
            } => {
                // GUI submit funnels through the same client-trait
                // verb that MCP uses; `warm_launch_hint` carries the
                // GUI's prewarm fast-path, MCP leaves it None.
                let resolved_name = resolved_task_name(&task_name, &generated_task_name);
                let req = OpenTaskRequest {
                    client_id: ClientId::gui_desktop(),
                    project_id: project_id.clone(),
                    task_name: Some(resolved_name.clone()),
                    branch_name: if source_branch.is_empty() {
                        None
                    } else {
                        Some(source_branch.clone())
                    },
                    kind: TaskKind::Direct,
                    launch_config,
                    cwd: None,
                    focus_after_open: true,
                    warm_launch_hint: warm_launch_id,
                };
                match self.client_open_task(req, cx) {
                    Ok(_) => {
                        self.new_task_modal = None;
                        let branch_label = self
                            .project_store
                            .project(&project_id)
                            .and_then(|p| another_one_core::project_store::current_branch(&p.path))
                            .or_else(|| self.project_store.current_branch_name(&project_id))
                            .unwrap_or_else(|| source_branch.clone());
                        let success_message = if branch_label.is_empty() {
                            format!("Opened direct task {}.", resolved_name)
                        } else {
                            format!("Opened direct task {} on {}.", resolved_name, branch_label)
                        };
                        self.show_success_toast(success_message, cx);
                    }
                    Err(err) => {
                        self.show_error_toast(format!("{err}"), cx);
                        self.cancel_active_new_task_prewarm();
                        self.new_task_modal = None;
                    }
                }
            }
            TaskLaunchRequest::Worktree {
                project_id,
                task_name,
                generated_task_name,
                branch_mode,
                launch_config,
            } => {
                let Some(project) = self.project_store.project(&project_id).cloned() else {
                    self.show_error_toast("Could not find the selected project.", cx);
                    self.cancel_active_new_task_prewarm();
                    self.new_task_modal = None;
                    return;
                };

                if let Some(state) = self.new_task_modal.as_mut() {
                    state.submitting = true;
                }
                self.cancel_active_new_task_prewarm();
                self.show_info_toast("Creating worktree task...", cx);
                self.pending_task_launch = Some(PendingTaskLaunch::NewTaskModal);
                let job_id = JobId::fresh();
                let originator = ClientId::gui_desktop();
                self.pending_worktree_jobs
                    .insert(job_id.clone(), (originator.clone(), project.id.clone()));
                self.emit_client_event(ClientEvent::TaskOpenStarted {
                    originator,
                    job_id,
                    project_id: project.id.clone(),
                });
                self.task_creation_receiver =
                    Some(another_one_core::project_service::spawn_task_creation(
                        project.id,
                        project.path,
                        project.name,
                        task_name,
                        generated_task_name,
                        branch_mode,
                        launch_config,
                    ));
                cx.notify();
            }
            TaskLaunchRequest::Review {
                project_id,
                pull_request_number,
                pull_request_url,
                head_branch,
                launch_config,
            } => {
                let Some(project) = self.project_store.project(&project_id).cloned() else {
                    self.show_error_toast("Could not find the selected project.", cx);
                    return;
                };
                let task_name = crate::task_launcher::review_task_title(pull_request_number);

                if let Some(existing) = crate::task_launcher::existing_review_worktree_project(
                    &self.project_store.projects,
                    &project,
                    pull_request_number,
                    &head_branch,
                    |project_id| self.project_store.current_branch_name(project_id),
                )
                .cloned()
                {
                    self.insert_and_open_task(
                        project.id,
                        existing.id.clone(),
                        TaskKind::Worktree,
                        task_name.clone(),
                        head_branch,
                        Some(existing.id.clone()),
                        existing.path,
                        None,
                        None,
                        ClientId::gui_desktop(),
                        cx,
                    );
                    self.show_success_toast(format!("Opened {}.", task_name), cx);
                    return;
                }

                self.show_info_toast(
                    format!("Creating review worktree for {}...", pull_request_url),
                    cx,
                );
                self.pending_task_launch = Some(PendingTaskLaunch::Review);
                self.task_creation_receiver = Some(
                    another_one_core::project_service::spawn_review_task_creation(
                        project.id,
                        project.path,
                        task_name,
                        pull_request_number,
                        head_branch,
                        launch_config,
                        false,
                        false,
                    ),
                );
                cx.notify();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_and_open_task(
        &mut self,
        root_project_id: String,
        target_project_id: String,
        kind: TaskKind,
        task_name: String,
        branch_name: String,
        worktree_project_id: Option<String>,
        project_path: std::path::PathBuf,
        launch_config: Option<TerminalLaunchConfig>,
        warm_launch_id: Option<u64>,
        originator: ClientId,
        cx: &mut Context<Self>,
    ) -> (String, SectionId) {
        let task_id = uuid::Uuid::new_v4().to_string();
        self.project_store.insert_task(Task {
            id: task_id.clone(),
            name: task_name,
            kind,
            root_project_id,
            target_project_id: target_project_id.clone(),
            branch_name: branch_name.clone(),
            section_id: SectionId::for_task(&target_project_id, &branch_name, &task_id).store_key(),
            worktree_project_id,
            tabs: Vec::new(),
            active_tab_id: String::new(),
            next_tab_id: 0,
            cwd: None,
        });
        self.commit_local_mutation();

        if let Some(project) = self.project_store.project(&target_project_id) {
            self.expanded_projects.insert(project.repo_id.clone());
            self.project_store
                .set_expanded_projects(&self.expanded_projects);
        }

        let section_id = SectionId::for_task(&target_project_id, &branch_name, &task_id);
        self.workspace_pane.update(cx, |workspace, cx| {
            workspace.activate_section(
                section_id.clone(),
                Some(project_path.clone()),
                launch_config.clone(),
                cx,
            );
        });
        self.prefetch_section_pull_request_and_checks(&section_id, &project_path);
        if let (Some(key), Some(launch_config)) = (self.active_terminal_key(cx), launch_config) {
            self.attach_or_start_prewarmed_terminal(
                warm_launch_id,
                key,
                project_path,
                launch_config,
                cx,
            );
        }
        self.mark_git_refresh_stale();

        // Single TaskOpened emit covers every synchronous task-create
        // path (GUI Direct via client_open_task, GUI Review-with-
        // existing-worktree, MCP / mobile via the trait verbs). The
        // tab id is the active tab in the just-activated section,
        // or `None` if the section has no tab yet (e.g. when the
        // caller passed `launch_config: None`).
        let tab_id = self.active_terminal_key(cx).map(|k| k.tab_id);
        self.emit_client_event(ClientEvent::TaskOpened {
            originator,
            task_id: task_id.clone(),
            section_id: section_id.clone(),
            tab_id,
        });

        (task_id, section_id)
    }

    pub(crate) fn submit_new_task_modal(&mut self, cx: &mut Context<Self>) {
        self.sanitize_new_task_modal_selected_agents();

        let (
            project_id,
            task_name,
            generated_task_name,
            source_branch,
            branch_mode,
            worktree_mode,
            launch_config,
            warm_launch_id,
        ) = {
            let Some(state) = self.new_task_modal.as_mut() else {
                return;
            };
            if state.submitting {
                return;
            }

            state.branch_dropdown_open = false;
            state.branch_filter_focused = false;
            state.task_name_focused = false;

            (
                state.project_id.clone(),
                state.task_name.trim().to_string(),
                state.generated_task_name.clone(),
                state.source_branch.clone(),
                state.branch_mode,
                state.worktree_mode,
                terminal_launch_config_for_selected_agents(&state.selected_agents),
                self.active_new_task_warm_launch_id,
            )
        };

        if !worktree_mode {
            let warm_launch_id = self
                .active_new_task_warm_launch_id
                .take()
                .or(warm_launch_id);
            self.launch_task_request(
                TaskLaunchRequest::Direct {
                    project_id,
                    task_name,
                    generated_task_name,
                    source_branch,
                    warm_launch_id,
                    launch_config,
                },
                cx,
            );
            return;
        }

        self.launch_task_request(
            TaskLaunchRequest::Worktree {
                project_id,
                task_name,
                generated_task_name,
                branch_mode: match branch_mode {
                    crate::new_task_modal::NewTaskBranchMode::NewBranch => {
                        TaskWorktreeBranchMode::NewBranchFrom { source_branch }
                    }
                    crate::new_task_modal::NewTaskBranchMode::ExistingBranch => {
                        TaskWorktreeBranchMode::ExistingBranch {
                            branch: source_branch,
                        }
                    }
                },
                launch_config,
            },
            cx,
        );
    }

    pub(crate) fn submit_create_branch_modal(&mut self, cx: &mut Context<Self>) {
        if self.branch_creation_receiver.is_some() {
            self.show_info_toast("A branch is already being created.", cx);
            return;
        }

        let Some((project_id, project_path)) = self.active_project_context(cx) else {
            self.show_error_toast("No active project is selected.", cx);
            return;
        };

        let (branch_name, use_current_task, migrate_changes) = {
            let Some(state) = self.create_branch_modal.as_mut() else {
                return;
            };
            if state.submitting {
                return;
            }
            let branch_name = if state.branch_name.trim().is_empty() {
                state.generated_branch_name.clone()
            } else {
                state.branch_name.clone()
            };
            state.submitting = true;
            (
                branch_name,
                state.use_current_task,
                state.migrate_changes || state.use_current_task,
            )
        };

        self.show_info_toast("Creating branch...", cx);
        self.git_actions_menu_open = false;
        self.branch_creation_receiver =
            Some(another_one_core::project_service::spawn_branch_creation(
                project_id,
                project_path,
                branch_name,
                use_current_task,
                migrate_changes,
            ));
        cx.notify();
    }

    fn drain_branch_creation(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(receiver) = self.branch_creation_receiver.as_mut() else {
            return false;
        };

        match receiver.try_recv() {
            Ok(reply) => {
                self.branch_creation_receiver = None;
                if let Some(state) = self.create_branch_modal.as_mut() {
                    state.submitting = false;
                }

                match reply.result {
                    Ok(success) => {
                        if success.use_current_task {
                            self.finish_current_task_branch_creation(&success, cx);
                        } else if let Some(prepared) = success.project.clone() {
                            self.finish_worktree_branch_creation(success, prepared, cx);
                        } else {
                            self.show_error_toast(
                                "The branch was created, but the app could not load the worktree.",
                                cx,
                            );
                        }
                    }
                    Err(error) => {
                        self.mark_git_refresh_stale();
                        self.show_error_toast(error.message, cx);
                    }
                }
                true
            }
            Err(broadcast::error::TryRecvError::Empty) => false,
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                log::warn!("branch_creation drain lagged {n} messages");
                false
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                self.branch_creation_receiver = None;
                if let Some(state) = self.create_branch_modal.as_mut() {
                    state.submitting = false;
                }
                self.mark_git_refresh_stale();
                self.show_error_toast("The branch creation process did not complete.", cx);
                true
            }
        }
    }

    fn finish_worktree_branch_creation(
        &mut self,
        success: another_one_core::project_service::BranchCreationSuccess,
        prepared: another_one_core::project_store::PreparedProject,
        cx: &mut Context<Self>,
    ) {
        let inserted = self.project_store.insert_prepared_project(prepared.clone());
        if !inserted {
            self.show_error_toast(
                "The worktree was created, but the app could not load it.",
                cx,
            );
            return;
        }
        self.commit_local_mutation();

        let Some(project) = self.project_store.project(&prepared.project.id).cloned() else {
            self.show_error_toast(
                "The worktree was created, but the app could not resolve its saved state.",
                cx,
            );
            return;
        };

        let root_project_id = self
            .project_store
            .root_project_id_for_project(&success.original_project_id)
            .unwrap_or_else(|| success.original_project_id.clone());

        self.insert_and_open_task(
            root_project_id,
            project.id.clone(),
            TaskKind::Worktree,
            success.task_name.clone(),
            success.branch_name.clone(),
            Some(project.id.clone()),
            project.path.clone(),
            None,
            None,
            ClientId::gui_desktop(),
            cx,
        );
        self.create_branch_modal = None;
        self.commit_local_mutation();
        self.show_success_toast(
            format!("Created branch {} in a new worktree.", success.branch_name),
            cx,
        );
    }

    fn finish_current_task_branch_creation(
        &mut self,
        success: &another_one_core::project_service::BranchCreationSuccess,
        cx: &mut Context<Self>,
    ) {
        let active_section = self.workspace_pane.read(cx).active_section.clone();
        let Some(section) = active_section else {
            self.refresh_project_git_state(&success.original_project_id);
            self.create_branch_modal = None;
            self.show_success_toast(format!("Created branch {}.", success.branch_name), cx);
            return;
        };

        let Some(task_id) = section.task_id.clone() else {
            self.refresh_project_git_state(&section.project_id);
            self.create_branch_modal = None;
            self.show_success_toast(format!("Created branch {}.", success.branch_name), cx);
            return;
        };

        let new_section = SectionId::for_task(&section.project_id, &success.branch_name, &task_id);
        self.move_active_task_section(&section, &new_section, cx);
        self.dispatch_update_task_branch(
            task_id.clone(),
            section.project_id.clone(),
            success.branch_name.clone(),
        );
        self.refresh_project_git_state(&section.project_id);
        self.create_branch_modal = None;
        self.show_success_toast(format!("Created branch {}.", success.branch_name), cx);
    }

    fn move_active_task_section(
        &mut self,
        old_section: &SectionId,
        new_section: &SectionId,
        cx: &mut Context<Self>,
    ) {
        self.workspace_pane.update(cx, |workspace, cx| {
            if let Some(state) = workspace.section_states.remove(old_section) {
                workspace.section_states.insert(new_section.clone(), state);
            }
            if workspace.active_section.as_ref() == Some(old_section) {
                workspace.active_section = Some(new_section.clone());
                workspace.persist_active_section(cx);
            }
            cx.notify();
        });

        let runtime_keys = self
            .live_terminal_runtimes
            .keys()
            .filter(|key| key.section_id == *old_section)
            .cloned()
            .collect::<Vec<_>>();
        for old_key in runtime_keys {
            let new_key = TerminalRuntimeKey {
                section_id: new_section.clone(),
                tab_id: old_key.tab_id.clone(),
            };
            if let Some(runtime) = self.live_terminal_runtimes.remove(&old_key) {
                self.live_terminal_runtimes.insert(new_key.clone(), runtime);
            }
            if let Some(snapshot) = self.terminal_surface_snapshots.remove(&old_key) {
                self.terminal_surface_snapshots
                    .insert(new_key.clone(), snapshot);
            }
            if let Some(remainder) = self.terminal_scroll_remainder_lines.remove(&old_key) {
                self.terminal_scroll_remainder_lines
                    .insert(new_key.clone(), remainder);
            }
            if let Some(process) = self.terminal_manager.processes.remove(&old_key) {
                self.terminal_manager
                    .processes
                    .insert(new_key.clone(), process);
            }
            if self.terminal_manager.pending_launches.remove(&old_key) {
                self.terminal_manager
                    .pending_launches
                    .insert(new_key.clone());
            }
            if let Some(output) = self.terminal_manager.recent_output.remove(&old_key) {
                self.terminal_manager
                    .recent_output
                    .insert(new_key.clone(), output);
            }
            if let Some(error) = self.terminal_manager.errors.remove(&old_key) {
                self.terminal_manager.errors.insert(new_key.clone(), error);
            }
        }
        if let Some(selection) = self.terminal_selection.as_mut() {
            if selection.key.section_id == *old_section {
                selection.key.section_id = new_section.clone();
            }
        }
    }

    pub(crate) fn active_changed_files(&self, cx: &App) -> Arc<[ChangedFile]> {
        self.active_open_in_project_id(cx)
            .as_deref()
            .and_then(|project_id| self.changed_files.get(project_id))
            .cloned()
            .unwrap_or_else(Self::empty_changed_files)
    }

    fn set_changed_files_snapshot(
        &mut self,
        project_id: &str,
        changed_files: Arc<[ChangedFile]>,
    ) -> bool {
        if self.changed_files.get(project_id) == Some(&changed_files) {
            return false;
        }

        self.changed_files_list_snapshots.remove(project_id);
        self.changed_files
            .insert(project_id.to_string(), changed_files);
        true
    }

    fn empty_changed_files() -> Arc<[ChangedFile]> {
        Arc::from(Vec::<ChangedFile>::new())
    }

    fn optimistic_stage_status(changed: &ChangedFile) -> char {
        match changed.index_status {
            ' ' | '?' => match changed.worktree_status {
                '?' => 'A',
                ' ' => 'M',
                other => other,
            },
            other => other,
        }
    }

    fn optimistic_unstage_worktree_status(changed: &ChangedFile) -> char {
        match changed.worktree_status {
            ' ' => match changed.index_status {
                '?' => '?',
                ' ' => 'M',
                other => other,
            },
            other => other,
        }
    }

    fn optimistic_stage_changed_file(changed: &mut ChangedFile) {
        changed.staged_additions += changed.unstaged_additions;
        changed.staged_deletions += changed.unstaged_deletions;
        changed.unstaged_additions = 0;
        changed.unstaged_deletions = 0;
        changed.index_status = Self::optimistic_stage_status(changed);
        changed.worktree_status = ' ';
        changed.untracked = false;
    }

    fn optimistic_unstage_changed_file(changed: &mut ChangedFile) {
        let worktree_status = Self::optimistic_unstage_worktree_status(changed);
        let became_untracked =
            changed.index_status == 'A' && !changed.untracked && changed.original_path.is_none();

        changed.unstaged_additions += changed.staged_additions;
        changed.unstaged_deletions += changed.staged_deletions;
        changed.staged_additions = 0;
        changed.staged_deletions = 0;
        changed.index_status = ' ';

        if became_untracked {
            changed.worktree_status = '?';
            changed.untracked = true;
        } else {
            changed.worktree_status = worktree_status;
            changed.untracked = changed.worktree_status == '?';
        }
    }

    fn apply_optimistic_mutation(
        changed_files: &mut Vec<ChangedFile>,
        mutation: &ChangedFilesGitMutation,
    ) -> bool {
        let mut changed_any = false;

        match mutation {
            ChangedFilesGitMutation::StageFile { changed } => {
                if let Some(file) = changed_files
                    .iter_mut()
                    .find(|file| file.path == changed.path)
                {
                    Self::optimistic_stage_changed_file(file);
                    changed_any = true;
                }
            }
            ChangedFilesGitMutation::UnstageFile { changed } => {
                if let Some(file) = changed_files
                    .iter_mut()
                    .find(|file| file.path == changed.path)
                {
                    Self::optimistic_unstage_changed_file(file);
                    changed_any = true;
                }
            }
            ChangedFilesGitMutation::StageAll => {
                for file in changed_files {
                    if file.can_stage() {
                        Self::optimistic_stage_changed_file(file);
                        changed_any = true;
                    }
                }
            }
            ChangedFilesGitMutation::UnstageAll => {
                for file in changed_files {
                    if file.can_unstage() {
                        Self::optimistic_unstage_changed_file(file);
                        changed_any = true;
                    }
                }
            }
            ChangedFilesGitMutation::RevertFiles {
                changed_files: files_to_revert,
            } => {
                let before_len = changed_files.len();
                changed_files.retain(|file| {
                    !files_to_revert.iter().any(|reverted| {
                        reverted.path == file.path && reverted.original_path == file.original_path
                    })
                });
                changed_any = changed_files.len() != before_len;
            }
        }

        changed_any
    }

    fn reapply_pending_changed_files(
        base_files: &Arc<[ChangedFile]>,
        pending: &PendingChangedFilesGitMutations,
    ) -> Arc<[ChangedFile]> {
        let mut next_files = base_files.as_ref().to_vec();
        let mut changed_any = false;

        for mutation in pending.mutations() {
            changed_any |= Self::apply_optimistic_mutation(&mut next_files, mutation);
        }

        if !changed_any {
            return base_files.clone();
        }

        next_files.retain(|file| file.has_staged_changes() || file.has_unstaged_changes());
        Arc::from(next_files)
    }

    fn spawn_changed_files_git_mutation(
        &self,
        project_id: &str,
        _project_path: std::path::PathBuf,
        mutation: ChangedFilesGitMutation,
    ) {
        self.sync_registry_project_store();
        crate::daemon_host::spawn_changed_files_mutation(
            self.changed_files_git_mutation_sender.clone(),
            self.registry_state.clone(),
            project_id.to_string(),
            mutation,
        );
    }

    fn project_path(&self, project_id: &str) -> Option<std::path::PathBuf> {
        self.project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
    }

    pub(crate) fn open_changed_file_diff(
        &mut self,
        project_id: &str,
        changed: &ChangedFile,
        source: crate::project_store::GitDiffSource,
        cx: &mut Context<Self>,
    ) {
        let Some(project_path) = self.project_path(project_id) else {
            self.show_error_toast("Could not find the selected project.", cx);
            return;
        };

        let (status, additions, deletions) = match source {
            crate::project_store::GitDiffSource::Staged => (
                changed.index_status,
                changed.staged_additions,
                changed.staged_deletions,
            ),
            crate::project_store::GitDiffSource::Unstaged => (
                if changed.untracked {
                    'A'
                } else {
                    changed.worktree_status
                },
                changed.unstaged_additions,
                changed.unstaged_deletions,
            ),
        };
        let selection = crate::project_store::GitDiffSelection {
            project_id: project_id.to_string(),
            path: changed.path.clone(),
            original_path: changed.original_path.clone(),
            source,
            status,
            additions,
            deletions,
            untracked: changed.untracked,
        };

        self.workspace_pane.update(cx, |workspace, cx| {
            workspace.active_git_diff = Some(selection.clone());
            workspace.keyboard_focus = WorkspaceKeyboardFocus::GitPanel;
            cx.notify();
        });
        self.git_diff_state = Some(GitDiffPaneState::Loading);
        self.git_diff_receiver = Some(another_one_core::git_service::spawn_changed_file_diff_load(
            selection,
            project_path,
        ));
        cx.notify();
    }

    pub(crate) fn navigate_changed_file_diff(
        &mut self,
        direction: NavigationDirection,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(active_selection) = self.workspace_pane.read(cx).active_git_diff.clone() else {
            return false;
        };
        let project_id = active_selection.project_id.clone();
        let Some(files) = self.changed_files.get(&project_id) else {
            return false;
        };

        let staged_collapsed = self.collapsed_change_sections.contains("staged");
        let uncommitted_collapsed = self.collapsed_change_sections.contains("uncommitted");
        let mut targets = Vec::new();
        for (index, changed) in files.iter().enumerate() {
            if changed.has_staged_changes() && !staged_collapsed {
                targets.push((index, crate::project_store::GitDiffSource::Staged));
            }
        }
        for (index, changed) in files.iter().enumerate() {
            if changed.has_unstaged_changes() && !uncommitted_collapsed {
                targets.push((index, crate::project_store::GitDiffSource::Unstaged));
            }
        }
        if targets.is_empty() {
            return false;
        }

        let current_index = targets
            .iter()
            .position(|(index, source)| {
                files
                    .get(*index)
                    .is_some_and(|changed| changed.path == active_selection.path)
                    && *source == active_selection.source
            })
            .unwrap_or(0);
        let next_index = wrapped_index(targets.len(), current_index, direction).unwrap_or(0);
        let (file_index, source) = targets[next_index];
        let Some(changed) = files.get(file_index).cloned() else {
            return false;
        };

        self.open_changed_file_diff(&project_id, &changed, source, cx);
        true
    }

    pub(crate) fn clear_changed_file_diff_state(&mut self, cx: &mut Context<Self>) {
        self.git_diff_receiver = None;
        self.git_diff_state = None;
        cx.notify();
    }

    pub(crate) fn active_git_action_for_project(
        &self,
        project_id: &str,
    ) -> Option<&ActiveToolbarGitAction> {
        active_toolbar_git_action_entry(&self.active_git_actions, project_id)
    }

    pub(crate) fn active_git_action_for_current_project(
        &self,
        cx: &App,
    ) -> Option<&ActiveToolbarGitAction> {
        let project_id = self.active_open_in_project_id(cx)?;
        self.active_git_action_for_project(&project_id)
    }

    pub(crate) fn changed_files_actions_busy(&self, project_id: &str) -> bool {
        has_active_toolbar_git_action(&self.active_git_actions, project_id)
    }

    pub(crate) fn changed_files_stage_all_pending(&self, project_id: &str) -> bool {
        self.pending_changed_files_git_mutations
            .get(project_id)
            .is_some_and(|pending| pending.mutations().any(ChangedFilesGitMutation::stages_all))
    }

    pub(crate) fn changed_files_unstage_all_pending(&self, project_id: &str) -> bool {
        self.pending_changed_files_git_mutations
            .get(project_id)
            .is_some_and(|pending| {
                pending
                    .mutations()
                    .any(ChangedFilesGitMutation::unstages_all)
            })
    }

    pub(crate) fn changed_files_file_pending(&self, project_id: &str, path: &str) -> bool {
        self.pending_changed_files_git_mutations
            .get(project_id)
            .is_some_and(|pending| {
                pending.mutations().any(|mutation| {
                    mutation.stages_all()
                        || mutation.unstages_all()
                        || mutation.stages_file(path)
                        || mutation.unstages_file(path)
                })
            })
    }

    pub(crate) fn changed_files_project_mutations_pending(&self, project_id: &str) -> bool {
        self.pending_changed_files_git_mutations
            .contains_key(project_id)
    }

    fn start_changed_files_git_mutation(
        &mut self,
        project_id: &str,
        mutation: ChangedFilesGitMutation,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.active_git_action_for_project(project_id).is_some() {
            return false;
        }

        let Some(project_path) = self.project_path(project_id) else {
            self.show_error_toast("Could not find the selected project.", cx);
            return false;
        };

        let current_files = self
            .changed_files
            .get(project_id)
            .cloned()
            .unwrap_or_else(Self::empty_changed_files);
        let mut start_now = None;
        let optimistic_files = {
            let pending = self
                .pending_changed_files_git_mutations
                .entry(project_id.to_string())
                .or_insert_with(|| PendingChangedFilesGitMutations {
                    confirmed_files: Some(current_files.clone()),
                    in_flight: None,
                    queued: VecDeque::new(),
                });

            if pending.confirmed_files.is_none() {
                pending.confirmed_files = Some(current_files.clone());
            }

            if pending.in_flight.is_none() {
                pending.in_flight = Some(mutation.clone());
                start_now = pending.in_flight.clone();
            } else {
                pending.queued.push_back(mutation.clone());
            }

            let base_files = pending
                .confirmed_files
                .as_ref()
                .cloned()
                .unwrap_or_else(Self::empty_changed_files);
            Self::reapply_pending_changed_files(&base_files, pending)
        };

        self.set_changed_files_snapshot(project_id, optimistic_files);

        if let Some(mutation) = start_now {
            self.spawn_changed_files_git_mutation(project_id, project_path, mutation);
        }

        cx.notify();
        true
    }

    pub(crate) fn start_toolbar_git_action(
        &mut self,
        action: crate::git_actions::ToolbarGitAction,
        cx: &mut Context<Self>,
    ) {
        let Some((project_id, project_path)) = self.active_project_context(cx) else {
            self.show_error_toast("No active project is selected.", cx);
            return;
        };

        if self.active_git_action_for_project(&project_id).is_some() {
            self.show_info_toast("A git action is already running for this project.", cx);
            return;
        }

        let branch_name_at_start = self.project_store.current_branch_name(&project_id);
        let action = match action {
            crate::git_actions::ToolbarGitAction::CreatePr { draft, .. } => {
                crate::git_actions::ToolbarGitAction::CreatePr {
                    draft,
                    base_branch: self
                        .project_store
                        .resolved_branch_settings(&project_id)
                        .and_then(|settings| settings.effective_default_target_branch),
                }
            }
            other => other,
        };

        let start_message = match &action {
            crate::git_actions::ToolbarGitAction::Commit => {
                if let Some(repo_id) = self.active_toolbar_repo_id(cx) {
                    self.dispatch_set_repo_default_commit_action(repo_id, "commit".to_string());
                }
                "Generating an AI commit message for staged changes..."
            }
            crate::git_actions::ToolbarGitAction::CommitAndPush => {
                if let Some(repo_id) = self.active_toolbar_repo_id(cx) {
                    self.dispatch_set_repo_default_commit_action(
                        repo_id,
                        "commit-and-push".to_string(),
                    );
                }
                "Generating an AI commit message before commit and push..."
            }
            crate::git_actions::ToolbarGitAction::UndoLastCommit => "",
            crate::git_actions::ToolbarGitAction::Fetch => "Fetching remote updates...",
            crate::git_actions::ToolbarGitAction::Pull => {
                "Pulling remote updates with fast-forward only..."
            }
            crate::git_actions::ToolbarGitAction::Push { force: false } => {
                "Pushing the current branch..."
            }
            crate::git_actions::ToolbarGitAction::Push { force: true } => {
                "Force-pushing the current branch with lease..."
            }
            crate::git_actions::ToolbarGitAction::CreatePr {
                draft: false,
                base_branch: Some(base_branch),
            } => {
                self.show_info_toast(
                    format!(
                        "Generating AI PR title/body and creating a pull request into {}...",
                        base_branch
                    ),
                    cx,
                );
                ""
            }
            crate::git_actions::ToolbarGitAction::CreatePr {
                draft: true,
                base_branch: Some(base_branch),
            } => {
                self.show_info_toast(
                    format!(
                        "Generating AI PR title/body and creating a draft pull request into {}...",
                        base_branch
                    ),
                    cx,
                );
                ""
            }
            crate::git_actions::ToolbarGitAction::CreatePr {
                draft: false,
                base_branch: None,
            } => "Generating AI PR title/body and creating a pull request...",
            crate::git_actions::ToolbarGitAction::CreatePr {
                draft: true,
                base_branch: None,
            } => "Generating AI PR title/body and creating a draft pull request...",
        };
        if !start_message.is_empty() {
            self.show_info_toast(start_message, cx);
        }

        self.sync_registry_project_store();

        let registry_state = self.registry_state.clone();
        let (tx, rx) = mpsc::channel();
        self.git_actions_menu_open = false;
        self.active_git_actions.insert(
            project_id.clone(),
            ActiveToolbarGitAction {
                action: action.clone(),
                branch_name_at_start,
                receiver: rx,
            },
        );
        std::thread::spawn(move || {
            let mut progress = |message: String| {
                let _ = tx.send(GitActionReply::Progress {
                    toast_kind: ToastKind::Info,
                    toast_message: message,
                });
            };
            let (refresh_git_state, toast_kind, toast_message) =
                match crate::daemon_host::run_toolbar_git_action(
                    registry_state,
                    &project_id,
                    action,
                    &mut progress,
                ) {
                    Ok(outcome) => (
                        outcome.refresh_git_state,
                        if outcome.warning {
                            ToastKind::Warning
                        } else {
                            ToastKind::Success
                        },
                        outcome.toast_message,
                    ),
                    Err(error) => (error.refresh_git_state, ToastKind::Error, error.message),
                };
            let git_state = refresh_git_state
                .then(|| crate::project_store::read_project_git_state(&project_path, true));
            let _ = tx.send(GitActionReply::Finished {
                project_id,
                refresh_git_state,
                git_state,
                toast_kind,
                toast_message,
            });
        });
        cx.notify();
    }

    fn drain_git_actions(&mut self, cx: &mut Context<Self>) -> bool {
        let drained = collect_drained_git_action_replies(&self.active_git_actions);

        if drained.is_empty() {
            return false;
        }

        for event in drained {
            match event {
                DrainedGitAction::Reply {
                    active_project_id,
                    reply,
                } => match reply {
                    GitActionReply::Progress {
                        toast_kind,
                        toast_message,
                    } => match toast_kind {
                        ToastKind::Success => self.show_success_toast(toast_message, cx),
                        ToastKind::Error => self.show_error_toast(toast_message, cx),
                        ToastKind::Warning => self.show_warning_toast(toast_message, cx),
                        ToastKind::Info => self.show_info_toast(toast_message, cx),
                    },
                    GitActionReply::Finished {
                        project_id,
                        refresh_git_state,
                        git_state,
                        toast_kind,
                        toast_message,
                    } => {
                        let active = self.active_git_actions.remove(&active_project_id);
                        let refresh_pull_request_lookup =
                            active.as_ref().is_some_and(|active| {
                                matches!(
                                    active.action,
                                    crate::git_actions::ToolbarGitAction::CreatePr { .. }
                                )
                            }) && matches!(toast_kind, ToastKind::Success);
                        if let Some(state) = git_state {
                            self.apply_project_git_state(&project_id, state);
                            let invalid_settings = self
                                .project_store
                                .clear_missing_branch_settings(&project_id);
                            let _ = self.handle_invalid_project_branch_settings(
                                &project_id,
                                invalid_settings,
                                cx,
                            );
                            self.git_workspace.mark_refreshed(true);
                        } else if refresh_git_state {
                            self.refresh_project_git_state(&project_id);
                        }
                        match toast_kind {
                            ToastKind::Success => self.show_success_toast(toast_message, cx),
                            ToastKind::Error => self.show_error_toast(toast_message, cx),
                            ToastKind::Warning => self.show_warning_toast(toast_message, cx),
                            ToastKind::Info => self.show_info_toast(toast_message, cx),
                        }
                        if refresh_pull_request_lookup {
                            if let Some((project_id, branch_name)) =
                                active.as_ref().and_then(|active| {
                                    active.branch_name_at_start.as_ref().map(|branch_name| {
                                        (project_id.clone(), branch_name.clone())
                                    })
                                })
                            {
                                self.invalidate_project_pull_request_lookup(
                                    &project_id,
                                    &branch_name,
                                );
                                self.invalidate_project_check_runs_lookup(
                                    &project_id,
                                    &branch_name,
                                );
                                if let Some(project_path) = self.project_path(&project_id) {
                                    self.request_project_pull_request_lookup_for(
                                        &project_id,
                                        &branch_name,
                                        &project_path,
                                    );
                                    let lookup_key = Self::project_check_runs_lookup_key(
                                        &project_id,
                                        &branch_name,
                                    );
                                    self.request_project_check_runs_lookup(
                                        &lookup_key,
                                        &project_path,
                                        None,
                                    );
                                }
                            }
                        }
                    }
                },
                DrainedGitAction::Disconnected { project_id } => {
                    self.active_git_actions.remove(&project_id);
                    self.show_error_toast("The background git action did not complete.", cx);
                }
            }
        }

        true
    }

    fn drain_changed_files_git_mutations(&mut self, cx: &mut Context<Self>) -> bool {
        let mut should_notify = false;

        loop {
            let reply = match self.changed_files_git_mutation_receiver.try_recv() {
                Ok(reply) => reply,
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    // Unlike the four lookup drains, the pending state
                    // here is a HashMap of optimistic UI state
                    // (queued + in-flight mutations, confirmed files),
                    // not a dedupe set. Clearing it would strand
                    // queued mutations and orphan the optimistic view.
                    // Log and carry on — capacity=64 plus a single
                    // 16 ms-tick consumer means this arm is
                    // effectively unreachable today; revisit if a
                    // daemon/mobile subscriber joins this stream.
                    log::warn!("changed_files_git_mutation drain lagged {n} messages");
                    continue;
                }
            };
            let pending = self
                .pending_changed_files_git_mutations
                .remove(&reply.project_id);
            should_notify = true;

            match reply.result {
                Ok(state) => {
                    let Some(mut pending) = pending else {
                        should_notify |= self.apply_project_git_state(&reply.project_id, state);
                        self.git_workspace.mark_refreshed(false);
                        continue;
                    };

                    let confirmed_files: Arc<[ChangedFile]> =
                        Arc::from(state.changed_files.clone());
                    pending.confirmed_files = Some(confirmed_files.clone());
                    pending.in_flight = pending.queued.pop_front();

                    if let Some(next_mutation) = pending.in_flight.clone() {
                        let optimistic_files =
                            Self::reapply_pending_changed_files(&confirmed_files, &pending);
                        should_notify |=
                            self.set_changed_files_snapshot(&reply.project_id, optimistic_files);
                        if let Some(project_path) = self.project_path(&reply.project_id) {
                            self.pending_changed_files_git_mutations
                                .insert(reply.project_id.clone(), pending);
                            self.spawn_changed_files_git_mutation(
                                &reply.project_id,
                                project_path,
                                next_mutation,
                            );
                        } else {
                            should_notify |= self.apply_project_git_state(&reply.project_id, state);
                            self.show_error_toast(
                                "Could not continue the queued git actions for the selected project.",
                                cx,
                            );
                        }
                    } else {
                        should_notify |= self.apply_project_git_state(&reply.project_id, state);
                        self.git_workspace.mark_refreshed(false);
                    }
                }
                Err(error) => {
                    if let Some(previous_files) =
                        pending.and_then(|pending| pending.confirmed_files)
                    {
                        should_notify |=
                            self.set_changed_files_snapshot(&reply.project_id, previous_files);
                    }
                    self.mark_git_refresh_stale();
                    self.show_error_toast(error, cx);
                }
            }
        }

        should_notify
    }

    fn drain_changed_file_diff(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(receiver) = self.git_diff_receiver.as_mut() else {
            return false;
        };

        match receiver.try_recv() {
            Ok(reply) => {
                self.git_diff_receiver = None;
                let active_selection = self.workspace_pane.read(cx).active_git_diff.clone();
                if active_selection.as_ref() != Some(&reply.selection) {
                    return false;
                }

                self.git_diff_state = Some(match reply.result {
                    Ok(diff) => GitDiffPaneState::Loaded(Arc::new(diff)),
                    Err(error) => {
                        self.show_error_toast(error.clone(), cx);
                        GitDiffPaneState::Failed(error)
                    }
                });
                true
            }
            Err(broadcast::error::TryRecvError::Empty) => false,
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                log::warn!("changed_file_diff drain lagged {n} messages");
                false
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                self.git_diff_receiver = None;
                false
            }
        }
    }

    fn drain_task_creation(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(receiver) = self.task_creation_receiver.as_mut() else {
            return false;
        };

        match receiver.try_recv() {
            Ok(reply) => {
                self.task_creation_receiver = None;
                let pending_launch = self.pending_task_launch.take();
                match reply.result {
                    Ok(success) => {
                        let prepared = success.project.clone();
                        let inserted = self.project_store.insert_prepared_project(prepared.clone());
                        if !inserted {
                            if pending_launch == Some(PendingTaskLaunch::NewTaskModal) {
                                if let Some(state) = self.new_task_modal.as_mut() {
                                    state.submitting = false;
                                }
                            }
                            self.show_error_toast(
                                "The worktree was created, but the app could not load it.",
                                cx,
                            );
                            return true;
                        }
                        self.commit_local_mutation();

                        let Some(project) =
                            self.project_store.project(&prepared.project.id).cloned()
                        else {
                            if pending_launch == Some(PendingTaskLaunch::NewTaskModal) {
                                if let Some(state) = self.new_task_modal.as_mut() {
                                    state.submitting = false;
                                }
                            }
                            self.show_error_toast(
                                "The worktree was created, but the app could not resolve its saved state.",
                                cx,
                            );
                            return true;
                        };

                        let task_id = uuid::Uuid::new_v4().to_string();
                        self.project_store.insert_task(Task {
                            id: task_id.clone(),
                            name: success.task_name.clone(),
                            kind: TaskKind::Worktree,
                            root_project_id: success.original_project_id.clone(),
                            target_project_id: project.id.clone(),
                            branch_name: success.branch_name.clone(),
                            section_id: SectionId::for_task(
                                &project.id,
                                &success.branch_name,
                                &task_id,
                            )
                            .store_key(),
                            worktree_project_id: Some(project.id.clone()),
                            tabs: Vec::new(),
                            active_tab_id: String::new(),
                            next_tab_id: 0,
                            cwd: None,
                        });

                        self.expanded_projects.insert(project.repo_id.clone());
                        self.project_store
                            .set_expanded_projects(&self.expanded_projects);

                        let section_id =
                            SectionId::for_task(&project.id, &success.branch_name, &task_id);
                        let project_path = project.path.clone();
                        let launch_config = success.launch_config;
                        let launch_config_for_section =
                            success.open_agent.then_some(launch_config.clone());
                        self.workspace_pane.update(cx, |workspace, cx| {
                            workspace.activate_section(
                                section_id.clone(),
                                Some(project_path.clone()),
                                launch_config_for_section,
                                cx,
                            );
                        });
                        self.prefetch_section_pull_request_and_checks(&section_id, &project_path);
                        if success.open_agent {
                            if let Some(key) = self.active_terminal_key(cx) {
                                self.attach_or_start_prewarmed_terminal(
                                    None,
                                    key,
                                    project.path.clone(),
                                    launch_config,
                                    cx,
                                );
                            }
                        }
                        if success.run_automatic_actions {
                            let automatic_actions = self
                                .project_store
                                .automatic_project_actions(&success.original_project_id);
                            for action in automatic_actions {
                                if let Err(error) = self.run_project_action_in_section(
                                    &section_id,
                                    action,
                                    Some(TerminalGridSize::default()),
                                    cx,
                                ) {
                                    self.show_error_toast(error, cx);
                                }
                            }
                        }
                        self.mark_git_refresh_stale();

                        if pending_launch == Some(PendingTaskLaunch::NewTaskModal) {
                            self.new_task_modal = None;
                        }
                        self.commit_local_mutation();
                        self.show_success_toast(
                            format!(
                                "Created worktree task {} on {}.",
                                success.task_name, success.branch_name
                            ),
                            cx,
                        );
                        // Correlated TaskOpened against the
                        // TaskOpenStarted that fired at submit time.
                        // Single-slot today (one async creation at a
                        // time), so `drain().next()` pulls whichever
                        // job is in-flight.
                        let drained = self.pending_worktree_jobs.drain().next();
                        if let Some((job_id, (originator, _project_id))) = drained {
                            // Resolve the just-added tab id, if any —
                            // worktree creation may activate a section
                            // without a tab when `open_agent` was
                            // false on the success record.
                            let tab_id = self
                                .workspace_pane
                                .read(cx)
                                .section_states
                                .get(&section_id)
                                .and_then(|s| s.tabs.last().map(|t| t.id.clone()));
                            self.emit_client_event(ClientEvent::TaskOpened {
                                originator,
                                task_id: task_id.clone(),
                                section_id: section_id.clone(),
                                tab_id,
                            });
                            let _ = job_id;
                        }
                    }
                    Err(error) => {
                        if pending_launch == Some(PendingTaskLaunch::NewTaskModal) {
                            if let Some(state) = self.new_task_modal.as_mut() {
                                state.submitting = false;
                            }
                        }
                        self.show_error_toast(error.message.clone(), cx);
                        let drained = self.pending_worktree_jobs.drain().next();
                        if let Some((job_id, (originator, _project_id))) = drained {
                            self.emit_client_event(ClientEvent::TaskOpenFailed {
                                originator,
                                job_id,
                                error: error.message,
                            });
                        }
                    }
                }
                true
            }
            Err(broadcast::error::TryRecvError::Empty) => false,
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                log::warn!("task_creation drain lagged {n} messages");
                false
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                self.task_creation_receiver = None;
                let pending_launch = self.pending_task_launch.take();
                if pending_launch == Some(PendingTaskLaunch::NewTaskModal) {
                    if let Some(state) = self.new_task_modal.as_mut() {
                        state.submitting = false;
                    }
                }
                self.show_error_toast("The task creation process did not complete.", cx);
                let drained = self.pending_worktree_jobs.drain().next();
                if let Some((job_id, (originator, _project_id))) = drained {
                    self.emit_client_event(ClientEvent::TaskOpenFailed {
                        originator,
                        job_id,
                        error: "task creation channel closed".to_string(),
                    });
                }
                true
            }
        }
    }

    fn drain_project_add(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(receiver) = self.project_add_receiver.as_mut() else {
            return false;
        };

        match receiver.try_recv() {
            Ok(reply) => {
                self.project_add_receiver = None;
                match reply.result {
                    Ok(project) => {
                        let project_name = project.project.name.clone();
                        let project_id = project.project.id.clone();
                        let added = self.project_store.insert_prepared_project(project.clone());
                        if added {
                            self.commit_local_mutation();
                            self.workspace_pane.update(cx, |workspace, cx| {
                                workspace.activate_project_page(project_id.clone(), cx);
                            });
                            self.show_success_toast(
                                format!("Added {} to the sidebar.", project_name),
                                cx,
                            );
                        } else {
                            self.show_info_toast(
                                format!("{} is already in the sidebar.", project_name),
                                cx,
                            );
                        }
                    }
                    Err(error) => {
                        self.show_error_toast(error, cx);
                    }
                }
                true
            }
            Err(broadcast::error::TryRecvError::Empty) => false,
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                log::warn!("project_add drain lagged {n} messages");
                false
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                self.project_add_receiver = None;
                self.show_error_toast("The add project process did not complete.", cx);
                true
            }
        }
    }

    fn drain_worktree_deletions(&mut self, cx: &mut Context<Self>) -> bool {
        let mut should_notify = false;

        while let Ok(reply) = self.worktree_deletion_receiver.try_recv() {
            should_notify = true;
            match reply.result {
                Ok(branch_warning) => {
                    if let Some(warning) = branch_warning {
                        self.show_warning_toast(warning, cx);
                    }
                    let worktree_display_name = self.remove_sidebar_worktree_task_from_store(
                        &reply.confirm,
                        reply.was_active_project,
                        cx,
                    );
                    self.show_success_toast(
                        format!("Deleted worktree {}.", worktree_display_name),
                        cx,
                    );
                }
                Err(error) => {
                    if crate::left_sidebar::should_remove_missing_worktree_task_from_store(
                        &error,
                        &reply.confirm.repo_path,
                        &reply.confirm.project_path,
                    ) {
                        let task_name = reply.confirm.task_name.clone();
                        self.remove_sidebar_worktree_task_from_store(
                            &reply.confirm,
                            reply.was_active_project,
                            cx,
                        );
                        self.show_warning_toast(
                            format!(
                                "The repository or worktree for {task_name} was already missing, so the task was removed from the app."
                            ),
                            cx,
                        );
                    } else {
                        self.show_error_toast(error, cx);
                    }
                }
            }
        }

        should_notify
    }

    pub(crate) fn drain_updater_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut should_notify = false;
        while let Some(event) = self.updater.try_recv() {
            match event {
                crate::updater::UpdaterEvent::StateChanged(state) => {
                    let installing = matches!(state, crate::updater::UpdateState::Installing);
                    self.updater_state = state;
                    should_notify = true;
                    if installing {
                        // The install helper is waiting for our
                        // PID to exit before it swaps the
                        // bundle/AppImage and relaunches.
                        // Schedule a quit so the user doesn't
                        // have to close the app manually.
                        cx.spawn(async move |_, cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(250))
                                .await;
                            let _ = cx.update(|cx| cx.quit());
                        })
                        .detach();
                    }
                }
                crate::updater::UpdaterEvent::Notice { kind, message } => match kind {
                    crate::updater::NoticeKind::Success => self.show_success_toast(message, cx),
                    crate::updater::NoticeKind::Warning => self.show_warning_toast(message, cx),
                    crate::updater::NoticeKind::Error => self.show_error_toast(message, cx),
                },
            }
        }
        should_notify
    }

    fn drain_commit_file_changes(&mut self, cx: &mut Context<Self>) -> bool {
        let mut should_notify = false;

        while let Ok(reply) = self.commit_file_changes_receiver.try_recv() {
            let key = Self::commit_file_changes_key(&reply.project_id, &reply.commit_id);
            let state = match reply.result {
                Ok(files) => CommitFileChangesState::Loaded(Arc::from(files)),
                Err(error) => {
                    self.show_warning_toast(error.clone(), cx);
                    CommitFileChangesState::Failed(error)
                }
            };

            if self.commit_file_changes_states.get(&key) != Some(&state) {
                self.commit_file_changes_states.insert(key, state);
                should_notify = true;
            }
        }

        should_notify
    }

    pub(crate) fn request_project_github_link_lookup(
        &mut self,
        project_id: &str,
        project_path: &std::path::Path,
    ) {
        if self.project_github_link_checked.contains(project_id)
            || self.project_github_link_requests.contains(project_id)
        {
            return;
        }

        self.project_github_link_requests
            .insert(project_id.to_string());
        another_one_core::git_service::spawn_github_link_lookup(
            self.project_github_link_sender.clone(),
            project_id.to_string(),
            project_path.to_path_buf(),
        );
    }

    fn drain_project_github_link_lookup(&mut self) -> bool {
        let mut should_notify = false;

        loop {
            let reply = match self.project_github_link_receiver.try_recv() {
                Ok(reply) => reply,
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    // Lost `n` replies. We can't know which project ids
                    // were dropped, so wipe the in-flight set; the next
                    // render tick will re-request any still-needed
                    // lookups. Without this, a dropped reply strands
                    // its project id in the set forever and
                    // `request_project_github_link_lookup` early-returns
                    // for that project until app restart.
                    log::warn!(
                        "project_github_link drain lagged {n} messages; clearing in-flight set"
                    );
                    self.project_github_link_requests.clear();
                    continue;
                }
            };
            self.project_github_link_requests.remove(&reply.project_id);
            self.project_github_link_checked
                .insert(reply.project_id.clone());

            if let Some(github_url) = reply.github_url {
                if self
                    .project_github_links
                    .get(&reply.project_id)
                    .map(String::as_str)
                    != Some(github_url.as_str())
                {
                    self.project_github_links
                        .insert(reply.project_id, github_url);
                    should_notify = true;
                }
            } else if self
                .project_github_links
                .remove(&reply.project_id)
                .is_some()
            {
                should_notify = true;
            }
        }

        should_notify
    }

    fn request_project_pull_request_lookup(
        &mut self,
        lookup_key: &str,
        branch_name: &str,
        project_path: &std::path::Path,
    ) {
        if self.project_pull_request_lookup_is_fresh(lookup_key)
            || self.project_pull_request_requests.contains(lookup_key)
        {
            return;
        }

        self.project_pull_request_requests
            .insert(lookup_key.to_string());
        another_one_core::git_service::spawn_pull_request_lookup(
            self.project_pull_request_sender.clone(),
            lookup_key.to_string(),
            project_path.to_path_buf(),
            branch_name.to_string(),
        );
    }

    pub(crate) fn project_page_pr_query_key(
        project_id: &str,
        filter_index: usize,
        query: &str,
    ) -> String {
        format!(
            "{project_id}:{filter_index}:{}",
            query.trim().to_ascii_lowercase()
        )
    }

    pub(crate) fn request_project_page_pull_requests(
        &mut self,
        project_id: &str,
        project_path: &std::path::Path,
        filter_index: usize,
        query: &str,
    ) {
        let key = Self::project_page_pr_query_key(project_id, filter_index, query);
        if self.project_page_pull_requests.contains_key(&key)
            || self.project_page_pull_requests_loading.contains(&key)
        {
            return;
        }

        self.project_page_pull_requests_loading.insert(key.clone());
        self.project_page_pull_requests_errors.remove(&key);
        another_one_core::git_service::spawn_project_page_pull_requests(
            self.project_page_pull_requests_sender.clone(),
            project_id.to_string(),
            project_path.to_path_buf(),
            filter_index,
            query.to_string(),
        );
    }

    fn drain_project_page_pull_requests(&mut self, cx: &mut Context<Self>) -> bool {
        let mut should_notify = false;
        loop {
            let reply = match self.project_page_pull_requests_receiver.try_recv() {
                Ok(reply) => reply,
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    // See note on `drain_project_github_link_lookup`:
                    // wipe the in-flight set so dropped replies don't
                    // strand their query keys as permanent ghosts.
                    log::warn!("project_page_pull_requests drain lagged {n} messages; clearing in-flight set");
                    self.project_page_pull_requests_loading.clear();
                    continue;
                }
            };
            let key = Self::project_page_pr_query_key(
                &reply.project_id,
                reply.filter_index,
                &reply.query,
            );
            self.project_page_pull_requests_loading.remove(&key);
            match reply.result {
                Ok(items) => {
                    self.project_page_pull_requests_errors.remove(&key);
                    self.project_page_pull_requests
                        .insert(key, Arc::from(items));
                    should_notify = true;
                }
                Err(error) => {
                    self.project_page_pull_requests_errors
                        .insert(key, error.clone());
                    self.show_warning_toast(error, cx);
                    should_notify = true;
                }
            }
        }
        should_notify
    }

    fn drain_project_pull_request_lookup(&mut self, cx: &mut Context<Self>) -> bool {
        let mut should_notify = false;

        loop {
            let reply = match self.project_pull_request_receiver.try_recv() {
                Ok(reply) => reply,
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    // See note on `drain_project_github_link_lookup`:
                    // wipe the in-flight set so dropped replies don't
                    // strand their lookup keys as permanent ghosts.
                    log::warn!(
                        "project_pull_request drain lagged {n} messages; clearing in-flight set"
                    );
                    self.project_pull_request_requests.clear();
                    continue;
                }
            };
            self.project_pull_request_requests.remove(&reply.lookup_key);
            self.project_pull_request_checked
                .insert(reply.lookup_key.clone());
            self.project_pull_request_checked_at
                .insert(reply.lookup_key.clone(), Instant::now());

            if let Some(pull_request) = reply.pull_request {
                if self.project_pull_requests.get(&reply.lookup_key) != Some(&pull_request) {
                    self.project_pull_requests
                        .insert(reply.lookup_key.clone(), pull_request.clone());
                    should_notify = true;
                }

                if let Some((project_id, branch_name, project_path, _)) =
                    self.active_project_check_runs_context(cx)
                {
                    let active_lookup_key =
                        Self::project_check_runs_lookup_key(&project_id, &branch_name);
                    if active_lookup_key == reply.lookup_key {
                        self.request_project_check_runs_lookup(
                            &active_lookup_key,
                            &project_path,
                            Some(pull_request.number),
                        );
                    }
                }
            } else if self
                .project_pull_requests
                .remove(&reply.lookup_key)
                .is_some()
            {
                should_notify = true;
            }
        }

        should_notify
    }

    fn request_project_check_runs_lookup(
        &mut self,
        lookup_key: &str,
        project_path: &std::path::Path,
        pull_request_number: Option<u64>,
    ) {
        if self.project_check_runs_lookup_is_fresh(lookup_key)
            || self.project_check_runs_requests.contains(lookup_key)
        {
            return;
        }

        self.project_check_runs_requests
            .insert(lookup_key.to_string());
        if !self.project_check_runs_states.contains_key(lookup_key) {
            self.project_check_runs_states
                .insert(lookup_key.to_string(), ProjectCheckRunsState::Loading);
        }
        another_one_core::git_service::spawn_check_runs_lookup(
            self.project_check_runs_sender.clone(),
            lookup_key.to_string(),
            project_path.to_path_buf(),
            pull_request_number,
        );
    }

    fn drain_project_check_runs_lookup(&mut self, cx: &mut Context<Self>) -> bool {
        let mut should_notify = false;

        loop {
            let reply = match self.project_check_runs_receiver.try_recv() {
                Ok(reply) => reply,
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    // See note on `drain_project_github_link_lookup`:
                    // wipe the in-flight set so dropped replies don't
                    // strand their lookup keys as permanent ghosts.
                    log::warn!(
                        "project_check_runs drain lagged {n} messages; clearing in-flight set"
                    );
                    self.project_check_runs_requests.clear();
                    continue;
                }
            };
            self.project_check_runs_requests.remove(&reply.lookup_key);
            self.project_check_runs_checked_at
                .insert(reply.lookup_key.clone(), Instant::now());

            let state = match reply.result {
                Ok(Some(checks)) => ProjectCheckRunsState::Loaded(Arc::from(checks)),
                Ok(None) => ProjectCheckRunsState::NoPullRequest,
                Err(error) => {
                    if self.active_right_sidebar_mode(cx) == RightSidebarMode::Checks {
                        self.show_warning_toast(error.clone(), cx);
                    }
                    ProjectCheckRunsState::Failed(error)
                }
            };

            if self.project_check_runs_states.get(&reply.lookup_key) != Some(&state) {
                self.project_check_runs_states
                    .insert(reply.lookup_key, state);
                should_notify = true;
            }
        }

        should_notify
    }

    pub(crate) fn stage_changed_file(
        &mut self,
        project_id: &str,
        changed: &ChangedFile,
        cx: &mut Context<Self>,
    ) -> bool {
        self.start_changed_files_git_mutation(
            project_id,
            ChangedFilesGitMutation::StageFile {
                changed: changed.clone(),
            },
            cx,
        )
    }

    pub(crate) fn stage_all_changes(&mut self, project_id: &str, cx: &mut Context<Self>) -> bool {
        self.start_changed_files_git_mutation(project_id, ChangedFilesGitMutation::StageAll, cx)
    }

    pub(crate) fn unstage_all_changes(&mut self, project_id: &str, cx: &mut Context<Self>) -> bool {
        self.start_changed_files_git_mutation(project_id, ChangedFilesGitMutation::UnstageAll, cx)
    }

    pub(crate) fn unstage_changed_file(
        &mut self,
        project_id: &str,
        changed: &ChangedFile,
        cx: &mut Context<Self>,
    ) -> bool {
        self.start_changed_files_git_mutation(
            project_id,
            ChangedFilesGitMutation::UnstageFile {
                changed: changed.clone(),
            },
            cx,
        )
    }

    pub(crate) fn revert_changed_file(
        &mut self,
        project_id: &str,
        changed: &ChangedFile,
        cx: &mut Context<Self>,
    ) -> bool {
        self.revert_changed_files(project_id, std::slice::from_ref(changed), cx)
    }

    pub(crate) fn revert_changed_files(
        &mut self,
        project_id: &str,
        changed_files: &[ChangedFile],
        cx: &mut Context<Self>,
    ) -> bool {
        if changed_files.is_empty() {
            return false;
        }

        self.start_changed_files_git_mutation(
            project_id,
            ChangedFilesGitMutation::RevertFiles {
                changed_files: changed_files.to_vec(),
            },
            cx,
        )
    }

    // ── Sidebar toggle animations ────────────────────────────────────

    pub fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.animating {
            return;
        }
        let from = self.sidebar_w;
        let to = if self.sidebar_is_open() {
            self.sidebar_saved = from.max(SIDEBAR_MIN_OPEN);
            SIDEBAR_COLLAPSED
        } else {
            self.sidebar_saved.max(SIDEBAR_MIN_OPEN)
        };
        if (from - to).abs() < 1. {
            self.sidebar_w = to;
            self.project_store
                .set_left_sidebar_open(to > SIDEBAR_COLLAPSED + 8.);
            self.sync_workspace_layout(cx);
            cx.notify();
            return;
        }
        self.animating = true;
        let handle = cx.entity().clone();
        window
            .spawn(cx, async move |async_cx| {
                const STEP_MS: u64 = 16;
                const DURATION_MS: f32 = 260.;
                let steps = ((DURATION_MS / STEP_MS as f32).ceil() as i32).max(1);
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let e = t * (2.0 - t);
                    let v = from + (to - from) * e;
                    let _ = handle.update(async_cx, |this, cx| {
                        this.sidebar_w = v;
                        cx.notify();
                    });
                    async_cx
                        .background_executor()
                        .timer(Duration::from_millis(STEP_MS))
                        .await;
                }
                let _ = handle.update(async_cx, |this, cx| {
                    this.sidebar_w = to;
                    this.animating = false;
                    this.project_store
                        .set_left_sidebar_open(to > SIDEBAR_COLLAPSED + 8.);
                    this.sync_workspace_layout(cx);
                    cx.notify();
                });
            })
            .detach();
        cx.notify();
    }

    pub fn toggle_right_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.animating {
            return;
        }
        let from = self.right_w;
        let to = if self.right_sidebar_is_open() {
            self.right_saved = from.max(RIGHT_SIDEBAR_MIN_OPEN);
            RIGHT_SIDEBAR_COLLAPSED
        } else {
            self.right_saved.max(RIGHT_SIDEBAR_MIN_OPEN)
        };
        if (from - to).abs() < 1. {
            self.right_w = to;
            self.sync_workspace_layout(cx);
            cx.notify();
            return;
        }
        self.animating = true;
        let handle = cx.entity().clone();
        window
            .spawn(cx, async move |async_cx| {
                const STEP_MS: u64 = 16;
                const DURATION_MS: f32 = 260.;
                let steps = ((DURATION_MS / STEP_MS as f32).ceil() as i32).max(1);
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let e = t * (2.0 - t);
                    let v = from + (to - from) * e;
                    let _ = handle.update(async_cx, |this, cx| {
                        this.right_w = v;
                        cx.notify();
                    });
                    async_cx
                        .background_executor()
                        .timer(Duration::from_millis(STEP_MS))
                        .await;
                }
                let _ = handle.update(async_cx, |this, cx| {
                    this.right_w = to;
                    this.animating = false;
                    this.sync_workspace_layout(cx);
                    cx.notify();
                });
            })
            .detach();
        cx.notify();
    }

    // ── Gutter drag handlers ─────────────────────────────────────────

    pub fn left_gutter_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.animating {
            return;
        }
        self.drag = Some((Gutter::Left, f32::from(ev.position.x)));
        self.clamp_layout(window);
        cx.notify();
    }

    pub fn right_gutter_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.animating {
            return;
        }
        self.drag = Some((Gutter::Right, f32::from(ev.position.x)));
        self.clamp_layout(window);
        cx.notify();
    }

    pub fn on_mouse_move(
        &mut self,
        ev: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.update_toast_drag(ev, cx) {
            return;
        }
        if self.update_terminal_selection_drag(ev, window, cx) {
            return;
        }
        if self.update_settings_git_action_script_selection_drag(ev, cx) {
            return;
        }

        let modifiers = window.modifiers();
        if modifiers.control || modifiers.platform {
            cx.notify();
        }

        let Some((kind, last_x)) = self.drag else {
            return;
        };
        if !ev.dragging() {
            return;
        }
        let x = f32::from(ev.position.x);
        let dx = x - last_x;
        self.drag = Some((kind, x));
        let ww = self.content_width(window);
        match kind {
            Gutter::Left => {
                self.sidebar_w = (self.sidebar_w + dx).clamp(
                    SIDEBAR_COLLAPSED,
                    ww - GUTTER - MIN_MAIN - GUTTER - MIN_RIGHT,
                );
                if self.sidebar_w > SIDEBAR_COLLAPSED + 8. {
                    self.sidebar_saved = self.sidebar_w;
                }
            }
            Gutter::Right => {
                self.right_w = (self.right_w - dx).clamp(RIGHT_SIDEBAR_COLLAPSED, ww);
                if self.right_w > RIGHT_SIDEBAR_COLLAPSED + 8. {
                    self.right_saved = self.right_w;
                }
            }
        }
        self.clamp_layout(window);
        self.sync_workspace_layout(cx);
        cx.notify();
    }

    pub fn on_mouse_up(&mut self, _ev: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let had_toast_drag = self.finish_toast_drag(cx);
        let had_terminal_selection = self.finish_terminal_selection_drag(cx);
        let had_settings_selection = self.finish_settings_git_action_script_selection_drag();
        let had_layout_drag = self.drag.take().is_some();

        if had_layout_drag {
            self.clamp_layout(window);
            self.sync_workspace_layout(cx);
            self.project_store
                .set_left_sidebar_open(self.sidebar_is_open());
        }

        if had_toast_drag || had_terminal_selection || had_settings_selection || had_layout_drag {
            cx.notify();
        }
    }

    pub fn on_modifiers_changed(
        &mut self,
        _ev: &ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
    }

    fn footer_add_project_button(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let icon_col = theme::toggle_icon_color_for_mode(window, self.project_store.ui.theme_mode);
        let hover_bg = app_theme.overlay_hover;

        div()
            .id("footer-add-project-btn")
            .flex()
            .items_center()
            .justify_center()
            .w(px(26.))
            .h(px(26.))
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .tooltip(move |_window, cx| Self::action_tooltip_view("Add a project folder", cx))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_add_project))
            .child(
                svg()
                    .path("assets/icons/icons__folder-plus.svg")
                    .size(px(16.))
                    .text_color(icon_col),
            )
    }

    fn footer_settings_button(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let icon_col = theme::toggle_icon_color_for_mode(window, self.project_store.ui.theme_mode);
        let hover_bg = app_theme.overlay_hover;

        div()
            .id("footer-settings-btn")
            .flex()
            .items_center()
            .justify_center()
            .w(px(26.))
            .h(px(26.))
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .tooltip(move |_window, cx| Self::action_tooltip_view("Open settings", cx))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_settings_button))
            .child(
                svg()
                    .path("assets/icons/icons__settings.svg")
                    .size(px(16.))
                    .text_color(icon_col),
            )
    }

    fn footer_install_update_button(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let icon_col = theme::toggle_icon_color_for_mode(window, self.project_store.ui.theme_mode);
        let accent_bg = app_theme.info.bg;
        let accent_hover_bg = app_theme.info.muted;
        let text_col = app_theme.text_primary;

        div()
            .id("footer-install-update-btn")
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .h(px(26.))
            .px(px(8.))
            .rounded_md()
            .bg(accent_bg)
            .cursor_pointer()
            .hover(move |s| s.bg(accent_hover_bg))
            .tooltip(move |_window, cx| Self::action_tooltip_view("Install downloaded update", cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.updater.send(crate::updater::UpdaterCommand::Install);
                    cx.stop_propagation();
                }),
            )
            .child(
                svg()
                    .path("assets/icons/icons__tool-download.svg")
                    .size(px(14.))
                    .text_color(icon_col),
            )
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(text_col)
                    .child(SharedString::from("Install update")),
            )
    }

    fn footer_branch_indicator(&self, window: &Window, cx: &App) -> impl IntoElement {
        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let icon_col = theme::toggle_icon_color_for_mode(window, self.project_store.ui.theme_mode);
        let text_col = app_theme.text_muted;

        if let Some(section) = self.workspace_pane.read(cx).active_section.clone() {
            let name: SharedString = section.branch_name.clone().into();
            div()
                .flex()
                .flex_row()
                .items_center()
                .relative()
                .top(px(-3.))
                .gap(px(6.))
                .child(
                    svg()
                        .path("assets/icons/icons__git-branch.svg")
                        .size(px(14.))
                        .text_color(icon_col),
                )
                .child(
                    div()
                        .text_size(rems(12. / 16.))
                        .text_color(text_col)
                        .child(name),
                )
        } else {
            div()
        }
    }

    fn footer_worktree_indicator(&self, window: &Window, cx: &App) -> impl IntoElement {
        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let icon_col = theme::toggle_icon_color_for_mode(window, self.project_store.ui.theme_mode);
        let text_col = app_theme.text_muted;

        let worktree_name = self
            .workspace_pane
            .read(cx)
            .active_section
            .as_ref()
            .and_then(|section| {
                self.project_store
                    .projects
                    .iter()
                    .find(|p| p.id == section.project_id)
                    .and_then(|p| p.worktree_name.clone())
            });

        if let Some(name) = worktree_name {
            let name: SharedString = name.into();
            div()
                .flex()
                .flex_row()
                .items_center()
                .relative()
                .top(px(-3.))
                .gap(px(6.))
                .child(
                    svg()
                        .path("assets/icons/icons__git-split.svg")
                        .size(px(14.))
                        .text_color(icon_col),
                )
                .child(
                    div()
                        .text_size(rems(12. / 16.))
                        .text_color(text_col)
                        .child(name),
                )
        } else {
            div()
        }
    }

    fn main_row(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        sw: f32,
        rw: f32,
        open: bool,
        busy: bool,
    ) -> impl IntoElement {
        let chrome = theme::chrome_bg(window);
        let gutter_bg = gpui::black().opacity(0.12);

        div()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .w(px(sw))
                    .flex_shrink_0()
                    .min_h_0()
                    .overflow_hidden()
                    .when(open, |panel| panel.child(self.sidebar_content(window, cx)))
                    .when(!open, |panel| panel.bg(chrome)),
            )
            .child(
                div()
                    .w(px(GUTTER))
                    .flex_shrink_0()
                    .bg(gutter_bg)
                    .when(!busy, |gutter| {
                        gutter.on_mouse_down(MouseButton::Left, cx.listener(Self::left_gutter_down))
                    })
                    .when(busy, |gutter| gutter.opacity(0.45)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(MIN_MAIN))
                    .min_h_0()
                    .pb(px(MAIN_PANE_BOTTOM_PAD))
                    .overflow_hidden()
                    .child(self.workspace_pane.clone()),
            )
            .child(
                div()
                    .w(px(GUTTER))
                    .flex_shrink_0()
                    .bg(gutter_bg)
                    .when(!busy, |gutter| {
                        gutter
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::right_gutter_down))
                    })
                    .when(busy, |gutter| gutter.opacity(0.45)),
            )
            .child(
                div()
                    .w(px(rw))
                    .flex_shrink_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.changed_files_panel(window, cx)),
            )
    }

    pub fn on_settings_button(
        &mut self,
        _ev: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_open = true;
        self.shortcut_capture_action = None;
        self.settings_git_commit_script_input.focused = false;
        self.settings_git_commit_script_input.selection_anchor = None;
        self.settings_git_pr_script_input.focused = false;
        self.settings_git_pr_script_input.selection_anchor = None;
        self.project_page_open_in_menu_project_id = None;
        cx.stop_propagation();
        cx.notify();
    }

    fn tick_toasts(&mut self) -> bool {
        if self.toasts.is_empty() && self.copied_toast.is_none() {
            return false;
        }

        let now = Instant::now();
        let before_len = self.toasts.len();
        self.toasts.retain(|toast| toast.dismiss_at > now);
        let has_active_toast_animation = self.toasts.iter().any(|toast| {
            now < toast.shown_at + TOAST_FADE_IN
                || toast.dismiss_at.saturating_duration_since(now) <= TOAST_FADE_OUT
        });
        let had_copy_feedback = self.copied_toast.is_some();
        if self
            .copied_toast
            .as_ref()
            .map(|(_, expires_at)| *expires_at <= now)
            .unwrap_or(false)
        {
            self.copied_toast = None;
        }
        if let Some(drag) = self.toast_drag.as_ref() {
            if !self.toasts.iter().any(|toast| toast.id == drag.toast_id) {
                self.toast_drag = None;
            }
        }
        if self
            .copied_toast
            .as_ref()
            .map(|(toast_id, _)| !self.toasts.iter().any(|toast| toast.id == *toast_id))
            .unwrap_or(false)
        {
            self.copied_toast = None;
        }

        before_len != self.toasts.len()
            || has_active_toast_animation
            || self.copied_toast.is_some()
            || had_copy_feedback
    }

    fn clear_pasted_image_preview(&mut self, cx: &mut App) -> bool {
        let Some(preview) = self.pasted_image_preview.take() else {
            return false;
        };

        preview.image.remove_asset(cx);
        true
    }

    fn tick_pasted_image_preview(&mut self, cx: &mut App) -> bool {
        let Some(preview) = self.pasted_image_preview.as_ref() else {
            return false;
        };

        let now = Instant::now();
        if preview.dismiss_at <= now {
            return self.clear_pasted_image_preview(cx);
        }

        now < preview.shown_at + TOAST_FADE_IN
            || preview.dismiss_at.saturating_duration_since(now) <= TOAST_FADE_OUT
    }

    fn refresh_timer_interval(&self) -> Duration {
        let terminal_fast_refresh = !self.terminal_manager.pending_launches.is_empty()
            || !self.prewarmed_terminal_launches.is_empty()
            || self.last_terminal_activity.elapsed() < TERMINAL_FAST_REFRESH_GRACE;

        // A blinking cursor needs steady redraws to actually animate.
        // Without bumping the cadence the terminal sits idle until the
        // next user keystroke / output and the blink flickers irregular.
        let any_blinking_cursor = self
            .terminal_surface_snapshots
            .values()
            .any(|snapshot| snapshot.cursor.as_ref().is_some_and(|c| c.blinking));

        let any_bell_active = self
            .terminal_bell_at
            .values()
            .any(|at| at.elapsed() < BELL_FLASH_DURATION);

        // While the user is dragging past the viewport edge we tick
        // the auto-scroll on each refresh — keep the cadence at the
        // animation rate so it actually feels like scrolling and not
        // a slideshow.
        let drag_autoscroll_active = self
            .terminal_selection
            .as_ref()
            .is_some_and(|s| s.dragging && s.autoscroll_dir != 0);

        if terminal_fast_refresh
            || self.resource_indicator_open
            || any_blinking_cursor
            || any_bell_active
            || drag_autoscroll_active
        {
            TOAST_ANIMATION_REFRESH_INTERVAL
        } else if self.toasts.is_empty()
            && self.copied_toast.is_none()
            && self.pasted_image_preview.is_none()
        {
            IDLE_REFRESH_INTERVAL
        } else {
            TOAST_ANIMATION_REFRESH_INTERVAL
        }
    }

    fn dismiss_toast_by_id(&mut self, toast_id: u64) -> bool {
        let before_len = self.toasts.len();
        self.toasts.retain(|toast| toast.id != toast_id);
        if self
            .toast_drag
            .as_ref()
            .map(|drag| drag.toast_id == toast_id)
            .unwrap_or(false)
        {
            self.toast_drag = None;
        }
        before_len != self.toasts.len()
    }

    fn on_toast_mouse_down(&mut self, toast_id: u64, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        let start_x = f32::from(ev.position.x);
        self.toast_drag = Some(ToastDrag {
            toast_id,
            start_x,
            current_x: start_x,
        });
        cx.stop_propagation();
        cx.notify();
    }

    fn update_toast_drag(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.toast_drag.as_mut() else {
            return false;
        };
        if !ev.dragging() {
            return false;
        }

        drag.current_x = f32::from(ev.position.x);
        cx.notify();
        true
    }

    fn finish_toast_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.toast_drag.take() else {
            return false;
        };

        if (drag.current_x - drag.start_x).max(0.) >= TOAST_SWIPE_DISMISS_THRESHOLD {
            self.dismiss_toast_by_id(drag.toast_id);
        }

        cx.notify();
        true
    }

    fn toast_drag_offset(&self, toast_id: u64) -> f32 {
        self.toast_drag
            .as_ref()
            .filter(|drag| drag.toast_id == toast_id)
            .map(|drag| (drag.current_x - drag.start_x).max(0.))
            .unwrap_or(0.)
    }

    fn show_toast_copy_feedback(&mut self, toast_id: u64) {
        self.copied_toast = Some((toast_id, Instant::now() + TOAST_COPY_FEEDBACK));
    }

    fn toast_copy_feedback_visible(&self, toast_id: u64) -> bool {
        self.copied_toast
            .as_ref()
            .map(|(copied_id, expires_at)| *copied_id == toast_id && *expires_at > Instant::now())
            .unwrap_or(false)
    }

    fn toast_visuals(
        kind: ToastKind,
        app_theme: theme::AppTheme,
    ) -> (
        &'static str,
        gpui::Hsla,
        gpui::Hsla,
        gpui::Hsla,
        &'static str,
    ) {
        match kind {
            ToastKind::Success => (
                "assets/icons/icons__badge-check.svg",
                app_theme.success.icon,
                app_theme.success.bg,
                app_theme.success.muted,
                "Success",
            ),
            ToastKind::Error => (
                "assets/icons/icons__alert-triangle.svg",
                app_theme.error.icon,
                app_theme.error.bg,
                app_theme.error.muted,
                "Error",
            ),
            ToastKind::Warning => (
                "assets/icons/icons__badge-alert.svg",
                app_theme.warning.icon,
                app_theme.warning.bg,
                app_theme.warning.muted,
                "Warning",
            ),
            ToastKind::Info => (
                "assets/icons/icons__file_icons__info.svg",
                app_theme.info.icon,
                app_theme.info.bg,
                app_theme.info.muted,
                "Info",
            ),
        }
    }

    fn toast_animation_state(toast: &AppToast, now: Instant) -> (f32, f32) {
        Self::transient_animation_state(toast.shown_at, toast.dismiss_at, now)
    }

    fn pasted_image_preview_animation_state(
        preview: &PastedImagePreview,
        now: Instant,
    ) -> (f32, f32) {
        Self::transient_animation_state(preview.shown_at, preview.dismiss_at, now)
    }

    fn transient_animation_state(
        shown_at: Instant,
        dismiss_at: Instant,
        now: Instant,
    ) -> (f32, f32) {
        let fade_in_progress = if TOAST_FADE_IN.is_zero() {
            1.
        } else {
            (now.saturating_duration_since(shown_at).as_secs_f32() / TOAST_FADE_IN.as_secs_f32())
                .clamp(0., 1.)
        };
        let fade_in = fade_in_progress * fade_in_progress * (3. - 2. * fade_in_progress);

        let fade_out_progress = if now >= dismiss_at {
            0.
        } else if TOAST_FADE_OUT.is_zero() {
            1.
        } else {
            (dismiss_at.saturating_duration_since(now).as_secs_f32() / TOAST_FADE_OUT.as_secs_f32())
                .clamp(0., 1.)
        };
        let fade_out = fade_out_progress * fade_out_progress * (3. - 2. * fade_out_progress);

        let opacity = fade_in.min(fade_out);
        let slide_offset = (1. - fade_in) * 14.;
        (opacity, slide_offset)
    }

    fn pasted_image_preview_card(
        &self,
        preview: &PastedImagePreview,
        opacity: f32,
    ) -> impl IntoElement {
        let label_color = hsla(208. / 360., 0.60, 0.72, 1.);
        let text_color = hsla(0., 0., 0.92, 1.);
        let format_color = hsla(0., 0., 0.70, 1.);
        let terminal_bg = theme::terminal_background_for_theme(theme::ResolvedTheme::Dark);

        div()
            .w(px(320.))
            .rounded(px(12.))
            .border_1()
            .border_color(hsla(208. / 360., 0.36, 0.32, 0.55))
            .bg(rgb(0x202329))
            .shadow_md()
            .overflow_hidden()
            .occlude()
            .opacity(opacity)
            .child(
                div().h(px(240.)).w_full().bg(terminal_bg).child(
                    img(preview.image.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(10.))
                    .child(
                        div()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(label_color)
                                    .child("Clipboard image"),
                            )
                            .child(
                                div()
                                    .pt(px(2.))
                                    .text_size(rems(12. / 16.))
                                    .text_color(text_color)
                                    .child("Preview"),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(format_color)
                            .child(Self::image_format_label(preview.image.format())),
                    ),
            )
    }

    fn image_format_label(format: gpui::ImageFormat) -> &'static str {
        match format {
            gpui::ImageFormat::Png => "PNG",
            gpui::ImageFormat::Jpeg => "JPEG",
            gpui::ImageFormat::Webp => "WEBP",
            gpui::ImageFormat::Gif => "GIF",
            gpui::ImageFormat::Svg => "SVG",
            gpui::ImageFormat::Bmp => "BMP",
            gpui::ImageFormat::Tiff => "TIFF",
            gpui::ImageFormat::Ico => "ICO",
        }
    }

    fn toast_card(
        &self,
        index: usize,
        toast: &AppToast,
        opacity: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let toast_id = toast.id;
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let (icon_path, icon_color, icon_bg, border_color, tone_label) =
            Self::toast_visuals(toast.kind, app_theme);
        let text_col = app_theme.text_primary;
        let card_bg = app_theme.card_bg;
        let copy_hover = app_theme.overlay_hover;
        let copied = self.toast_copy_feedback_visible(toast_id);
        let copy_icon = if copied {
            app_theme.success.icon
        } else {
            app_theme.text_muted
        };
        let message = toast.message.clone();
        let copy_message = toast.copy_message.clone();
        let copy_tooltip = if toast.kind == ToastKind::Error {
            "Copy error details"
        } else {
            "Copy notification message"
        };

        div()
            .w(px(360.))
            .px(px(12.))
            .py(px(10.))
            .rounded(px(10.))
            .bg(card_bg)
            .border_1()
            .border_color(border_color)
            .shadow_md()
            .overflow_hidden()
            .occlude()
            .opacity(opacity)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _window, cx| {
                    this.on_toast_mouse_down(toast_id, ev, cx);
                }),
            )
            .child(
                div()
                    .flex()
                    .w_full()
                    .items_start()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .w(px(28.))
                            .h(px(28.))
                            .rounded(px(999.))
                            .bg(icon_bg)
                            .child(svg().path(icon_path).size(px(16.)).text_color(icon_color)),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex_1()
                            .pt(px(1.))
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(icon_color)
                                    .child(tone_label),
                            )
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .pt(px(2.))
                                    .text_size(rems(12. / 16.))
                                    .line_height(rems(18. / 16.))
                                    .text_color(text_col)
                                    .child(message.clone()),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("toast-copy-{index}")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .w(px(28.))
                            .h(px(28.))
                            .rounded(px(7.))
                            .cursor_pointer()
                            .hover(move |style| style.bg(copy_hover))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    if copied { "Copied" } else { copy_tooltip },
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _event: &MouseDownEvent, _window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_message.to_string(),
                                    ));
                                    this.show_toast_copy_feedback(toast_id);
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                svg()
                                    .path(if copied {
                                        "assets/icons/icons__check.svg"
                                    } else {
                                        "assets/icons/icons__copy.svg"
                                    })
                                    .size(px(15.))
                                    .text_color(copy_icon),
                            ),
                    ),
            )
    }

    fn toast_layer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.toasts.is_empty() && self.pasted_image_preview.is_none() {
            return div().id("toast-layer");
        }

        let now = Instant::now();
        let mut layer = div()
            .id("toast-layer")
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .justify_end()
            .items_end()
            .gap(px(8.))
            .pr(px(14.))
            .pb(px(14.));

        if let Some(preview) = self.pasted_image_preview.as_ref() {
            let (opacity, slide_offset) = Self::pasted_image_preview_animation_state(preview, now);
            layer = layer.child(
                div()
                    .relative()
                    .top(px(slide_offset))
                    .w(px(320.))
                    .flex()
                    .justify_end()
                    .child(self.pasted_image_preview_card(preview, opacity)),
            );
        }

        for (index, toast) in self.toasts.iter().enumerate() {
            let (opacity, slide_offset) = Self::toast_animation_state(toast, now);
            let drag_offset = self.toast_drag_offset(toast.id);
            let drag_opacity = opacity * (1. - (drag_offset / 240.).clamp(0., 0.45));
            layer = layer.child(
                div()
                    .relative()
                    .top(px(slide_offset))
                    .w(px(360. + drag_offset))
                    .flex()
                    .justify_end()
                    .child(self.toast_card(index, toast, drag_opacity, cx)),
            );
        }

        layer
    }
}

fn select_active_section(
    active_section: &mut Option<SectionId>,
    active_project_page: &mut Option<String>,
    section_id: SectionId,
) -> bool {
    let changed = active_section.as_ref() != Some(&section_id) || active_project_page.is_some();
    *active_section = Some(section_id);
    *active_project_page = None;
    changed
}

fn persisted_active_section_key(active_section: Option<&SectionId>) -> Option<String> {
    active_section.map(SectionId::store_key)
}

/// Absolute path to the `another-one-mcp-shim` binary for the
/// daemon-MCP catalog entry. Resolved by looking next to the
/// running app binary — that's where the release packaging
/// drops it. Returns `None` when `current_exe()` fails so the
/// caller can skip registering a broken catalog entry; we
/// intentionally do **not** fall back to a bare name (letting
/// `$PATH` resolve it would let a hostile shim earlier on `PATH`
/// be written into users' harness configs during `sync_all`).
fn shim_binary_path() -> Option<std::path::PathBuf> {
    let exe_name = if cfg!(windows) {
        "another-one-mcp-shim.exe"
    } else {
        "another-one-mcp-shim"
    };
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    Some(parent.join(exe_name))
}

fn choose_initial_section(
    projects: &[crate::project_store::Project],
    section_states: &HashMap<SectionId, SectionState>,
    last_active_section_key: Option<&str>,
) -> Option<SectionId> {
    if let Some(section_id) = last_active_section_key
        .and_then(SectionId::from_store_key)
        .filter(|section_id| {
            section_id.task_id.is_some() && section_states.contains_key(section_id)
        })
    {
        return Some(section_id);
    }

    let project_order = projects
        .iter()
        .enumerate()
        .map(|(index, project)| (project.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut restored_sections = section_states.iter().collect::<Vec<_>>();
    restored_sections.sort_by(|(left_id, _), (right_id, _)| {
        project_order
            .get(left_id.project_id.as_str())
            .copied()
            .unwrap_or(usize::MAX)
            .cmp(
                &project_order
                    .get(right_id.project_id.as_str())
                    .copied()
                    .unwrap_or(usize::MAX),
            )
            .then_with(|| left_id.project_id.cmp(&right_id.project_id))
            .then_with(|| left_id.branch_name.cmp(&right_id.branch_name))
            .then_with(|| left_id.task_id.cmp(&right_id.task_id))
    });

    if let Some(section_id) = restored_sections
        .into_iter()
        .find_map(|(section_id, state)| {
            section_id.task_id.as_ref()?;
            state
                .tabs
                .get(state.active_tab)
                .filter(|tab| tab.launch_config.provider.is_some())
                .map(|_| section_id.clone())
        })
    {
        return Some(section_id);
    }

    projects.first().and_then(|project| {
        project
            .checkout
            .current_branch
            .as_ref()
            .map(|branch_name| SectionId::new(&project.id, branch_name))
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationDirection {
    Next,
    Previous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SidebarTaskNavigationTarget {
    root_project_id: String,
    project_id: String,
    task_id: String,
    branch_name: String,
    project_path: std::path::PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GlobalTabNavigationTarget {
    root_project_id: String,
    section_id: SectionId,
    tab_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NewTaskShortcutTarget {
    project_id: String,
    source_branch: Option<String>,
}

fn wrapped_index(
    len: usize,
    current_index: usize,
    direction: NavigationDirection,
) -> Option<usize> {
    if len == 0 || current_index >= len {
        return None;
    }

    Some(match direction {
        NavigationDirection::Next => (current_index + 1) % len,
        NavigationDirection::Previous => (current_index + len - 1) % len,
    })
}

fn next_global_tab_navigation_target<'a>(
    targets: &'a [GlobalTabNavigationTarget],
    projects: &[crate::project_store::Project],
    active_section: Option<&SectionId>,
    active_project_page: Option<&str>,
    active_tab: Option<usize>,
    direction: NavigationDirection,
) -> Option<&'a GlobalTabNavigationTarget> {
    if targets.len() < 2 {
        return None;
    }

    if let Some(project_id) = active_project_page {
        let root_project_id = sidebar_root_project_id_for_project(projects, project_id)
            .unwrap_or_else(|| project_id.to_string());
        return match direction {
            NavigationDirection::Next => targets
                .iter()
                .find(|target| target.root_project_id == root_project_id),
            NavigationDirection::Previous => targets
                .iter()
                .rev()
                .find(|target| target.root_project_id == root_project_id),
        };
    }

    let active_section = active_section?;
    let active_tab = active_tab?;
    let current_index = targets.iter().position(|target| {
        target.section_id == *active_section && target.tab_index == active_tab
    })?;
    let next_index = wrapped_index(targets.len(), current_index, direction)?;

    targets.get(next_index)
}

fn global_tab_navigation_targets(
    projects: &[crate::project_store::Project],
    tasks_by_root_project: &HashMap<String, Vec<Task>>,
    pinned_task_ids: &HashSet<String>,
    section_states: &HashMap<SectionId, SectionState>,
) -> Vec<GlobalTabNavigationTarget> {
    let project_order = projects
        .iter()
        .enumerate()
        .map(|(index, project)| (project.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut ordered_sections = Vec::new();
    let mut seen_sections = HashSet::new();

    for task_target in
        sidebar_task_navigation_targets(projects, tasks_by_root_project, pinned_task_ids)
    {
        let section_id = SectionId::for_task(
            &task_target.project_id,
            &task_target.branch_name,
            &task_target.task_id,
        );
        if section_states.contains_key(&section_id) && seen_sections.insert(section_id.clone()) {
            ordered_sections.push((task_target.root_project_id, section_id));
        }
    }

    let mut remaining_sections = section_states
        .keys()
        .filter(|section_id| !seen_sections.contains(*section_id))
        .cloned()
        .collect::<Vec<_>>();
    remaining_sections.sort_by(|left, right| {
        project_order
            .get(left.project_id.as_str())
            .copied()
            .unwrap_or(usize::MAX)
            .cmp(
                &project_order
                    .get(right.project_id.as_str())
                    .copied()
                    .unwrap_or(usize::MAX),
            )
            .then_with(|| left.project_id.cmp(&right.project_id))
            .then_with(|| left.task_id.cmp(&right.task_id))
            .then_with(|| left.branch_name.cmp(&right.branch_name))
    });

    ordered_sections.extend(remaining_sections.into_iter().map(|section_id| {
        let root_project_id = sidebar_root_project_id_for_project(projects, &section_id.project_id)
            .unwrap_or_else(|| section_id.project_id.clone());
        (root_project_id, section_id)
    }));

    ordered_sections
        .into_iter()
        .flat_map(|(root_project_id, section_id)| {
            let tab_count = section_states
                .get(&section_id)
                .map(|state| state.tabs.len())
                .unwrap_or(0);

            (0..tab_count).map(move |tab_index| GlobalTabNavigationTarget {
                root_project_id: root_project_id.clone(),
                section_id: section_id.clone(),
                tab_index,
            })
        })
        .collect()
}

fn root_project_navigation_targets(projects: &[crate::project_store::Project]) -> Vec<String> {
    let mut group_order = Vec::new();
    let mut grouped_indices: HashMap<String, Vec<usize>> = HashMap::new();

    for (index, project) in projects.iter().enumerate() {
        let group_key = sidebar_group_key(project);
        grouped_indices
            .entry(group_key.clone())
            .and_modify(|indices| indices.push(index))
            .or_insert_with(|| {
                group_order.push(group_key);
                vec![index]
            });
    }

    group_order
        .into_iter()
        .filter_map(|group_key| {
            let indices = grouped_indices.get(&group_key)?;
            let root_index = indices
                .iter()
                .copied()
                .find(|index| projects[*index].worktree_name.is_none())
                .unwrap_or(indices[0]);
            Some(projects[root_index].id.clone())
        })
        .collect()
}

fn next_project_navigation_target<'a>(
    targets: &'a [String],
    projects: &[crate::project_store::Project],
    active_section: Option<&SectionId>,
    active_project_page: Option<&str>,
) -> Option<&'a str> {
    if targets.is_empty() {
        return None;
    }

    let current_project_id = active_project_page
        .map(str::to_string)
        .or_else(|| active_section.map(|section| section.project_id.clone()));

    let Some(current_project_id) = current_project_id else {
        return targets.first().map(String::as_str);
    };

    let current_root_project_id =
        sidebar_root_project_id_for_project(projects, &current_project_id)
            .unwrap_or(current_project_id);
    let current_index = targets
        .iter()
        .position(|project_id| *project_id == current_root_project_id)?;
    let next_index = wrapped_index(targets.len(), current_index, NavigationDirection::Next)?;

    targets.get(next_index).map(String::as_str)
}

fn sidebar_group_key(project: &crate::project_store::Project) -> String {
    project
        .repo_common_dir
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| format!("project:{}", project.id))
}

fn sidebar_root_project_id_for_project(
    projects: &[crate::project_store::Project],
    project_id: &str,
) -> Option<String> {
    let project = projects.iter().find(|project| project.id == project_id)?;
    let group_key = sidebar_group_key(project);

    projects
        .iter()
        .find(|candidate| {
            sidebar_group_key(candidate) == group_key && candidate.worktree_name.is_none()
        })
        .map(|project| project.id.clone())
        .or_else(|| Some(project.id.clone()))
}

fn sidebar_task_navigation_targets(
    projects: &[crate::project_store::Project],
    tasks_by_root_project: &HashMap<String, Vec<Task>>,
    pinned_task_ids: &HashSet<String>,
) -> Vec<SidebarTaskNavigationTarget> {
    let mut group_order = Vec::new();
    let mut grouped_indices: HashMap<String, Vec<usize>> = HashMap::new();

    for (index, project) in projects.iter().enumerate() {
        let group_key = sidebar_group_key(project);
        grouped_indices
            .entry(group_key.clone())
            .and_modify(|indices| indices.push(index))
            .or_insert_with(|| {
                group_order.push(group_key);
                vec![index]
            });
    }

    let mut targets = Vec::new();
    for group_key in group_order {
        let Some(indices) = grouped_indices.get(&group_key) else {
            continue;
        };

        let root_index = indices
            .iter()
            .copied()
            .find(|index| projects[*index].worktree_name.is_none())
            .unwrap_or(indices[0]);
        let root_project = &projects[root_index];

        let mut group_targets = tasks_by_root_project
            .get(&root_project.id)
            .into_iter()
            .flat_map(|tasks| tasks.iter())
            .filter_map(|task| {
                crate::task_launcher::task_workspace_target(projects, root_project, task).map(
                    |target| SidebarTaskNavigationTarget {
                        root_project_id: target.root_project_id,
                        project_id: target.project_id,
                        task_id: target.task_id,
                        branch_name: target.branch_name,
                        project_path: target.project_path,
                    },
                )
            })
            .collect::<Vec<_>>();

        group_targets.sort_by_key(|target| !pinned_task_ids.contains(&target.task_id));
        targets.extend(group_targets);
    }

    targets
}

fn next_task_navigation_target<'a>(
    targets: &'a [SidebarTaskNavigationTarget],
    projects: &[crate::project_store::Project],
    active_section: Option<&SectionId>,
    active_project_page: Option<&str>,
    direction: NavigationDirection,
) -> Option<&'a SidebarTaskNavigationTarget> {
    if targets.is_empty() {
        return None;
    }

    if let Some(project_id) = active_project_page {
        let root_project_id = sidebar_root_project_id_for_project(projects, project_id)
            .unwrap_or_else(|| project_id.to_string());
        return match direction {
            NavigationDirection::Next => targets
                .iter()
                .find(|target| target.root_project_id == root_project_id),
            NavigationDirection::Previous => targets
                .iter()
                .rev()
                .find(|target| target.root_project_id == root_project_id),
        };
    }

    let active_task_id = active_section.and_then(|section| section.task_id.as_deref())?;
    let current_index = targets
        .iter()
        .position(|target| target.task_id == active_task_id)?;
    let next_index = wrapped_index(targets.len(), current_index, direction)?;

    targets.get(next_index)
}

fn resolve_new_task_shortcut_target<F>(
    active_section: Option<&SectionId>,
    active_project_page: Option<&str>,
    task_root_project_id: F,
) -> Option<NewTaskShortcutTarget>
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(section) = active_section {
        let project_id = section
            .task_id
            .as_deref()
            .and_then(task_root_project_id)
            .unwrap_or_else(|| section.project_id.clone());

        return Some(NewTaskShortcutTarget {
            project_id,
            source_branch: Some(section.branch_name.clone()),
        });
    }

    active_project_page.map(|project_id| NewTaskShortcutTarget {
        project_id: project_id.to_string(),
        source_branch: None,
    })
}

fn apply_terminal_session_backfill(
    section_states: &mut HashMap<SectionId, SectionState>,
    key: &TerminalRuntimeKey,
    session: TerminalSessionRef,
) -> bool {
    let Some(tab) = section_states
        .get_mut(&key.section_id)
        .and_then(|state| state.tabs.iter_mut().find(|tab| tab.id == key.tab_id))
    else {
        return false;
    };

    tab.launch_config = tab.launch_config.clone().with_session(Some(session));
    true
}

fn remove_terminal_runtime_state<T>(
    live_terminal_runtimes: &mut HashMap<TerminalRuntimeKey, T>,
    terminal_surface_snapshots: &mut HashMap<TerminalRuntimeKey, TerminalSurfaceSnapshot>,
    pending_terminal_launches: &mut HashSet<TerminalRuntimeKey>,
    terminal_recent_output: &mut HashMap<TerminalRuntimeKey, String>,
    terminal_runtime_errors: &mut HashMap<TerminalRuntimeKey, String>,
    key: &TerminalRuntimeKey,
) -> Option<T> {
    pending_terminal_launches.remove(key);
    terminal_surface_snapshots.remove(key);
    terminal_recent_output.remove(key);
    terminal_runtime_errors.remove(key);
    live_terminal_runtimes.remove(key)
}

#[cfg(test)]
mod tests {
    use super::{
        active_toolbar_git_action_entry, apply_terminal_session_backfill,
        apply_terminal_title_update, choose_initial_section, collect_drained_git_action_replies,
        encode_terminal_mouse_event, fixed_title_for_project_action, global_tab_navigation_targets,
        has_active_toolbar_git_action, new_tab_seed_agent_id, next_global_tab_navigation_target,
        next_project_navigation_target, next_task_navigation_target,
        open_in_target_path_for_project, persisted_active_section_key,
        remove_terminal_runtime_state, resolve_new_task_shortcut_target,
        root_project_navigation_targets, select_active_section, sidebar_task_navigation_targets,
        terminal_line_selection_range, terminal_link_at_position, terminal_link_ranges,
        terminal_open_link_modifier_held, terminal_scroll_lines, terminal_selected_text,
        terminal_selection_range, terminal_word_selection_range, ActiveToolbarGitAction,
        AnotherOneApp, AppToast, DrainedGitAction, GitActionReply, NavigationDirection,
        NewTaskShortcutTarget, SectionId, SectionState, TabCloseScope, TerminalCellPosition,
        TerminalLinkRange, TerminalMouseAction, TerminalMouseButton, TerminalMouseModifiers,
        TerminalSelectionRange, TerminalTab, ToastKind,
    };
    use crate::agents::{
        agent_output_indicates_missing_session, AgentProviderKind, TerminalLaunchConfig,
        TerminalSessionKind, TerminalSessionRef,
    };
    use crate::git_actions::ToolbarGitAction;
    use crate::project_store::{
        GitDiffSelection, GitDiffSource, PersistedSectionState, PersistedTerminalTab, Project,
        ProjectAction, ProjectActionIcon, ProjectActionKind, ProjectActionScope,
        ProjectCheckoutState, ProjectKind, Task, TaskKind,
    };
    use crate::terminal_runtime::{
        TerminalCellSnapshot, TerminalLineSnapshot, TerminalMouseEncoding, TerminalMouseLevel,
        TerminalMouseProtocol, TerminalRuntimeKey, TerminalRuntimeUpdate, TerminalSurfaceSnapshot,
    };
    use crate::theme::{dark_theme, light_theme};
    use daemon_proto::TerminalRestoreStatus;
    use gpui::{point, px, ClipboardItem, Image, ImageFormat, Modifiers, ScrollDelta};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn shell_tab(id: usize, title: &str) -> PersistedTerminalTab {
        PersistedTerminalTab {
            id: id.to_string(),
            title: title.to_string(),
            pinned: false,
            fixed_title: None,
            provider: None,
            launch_config: Some(TerminalLaunchConfig::default()),
            restore_status: TerminalRestoreStatus::NotStarted,
            failure_message: None,
            failure_details: None,
        }
    }

    fn agent_tab(id: &str, title: &str, provider: AgentProviderKind) -> PersistedTerminalTab {
        PersistedTerminalTab {
            id: id.to_string(),
            title: title.to_string(),
            pinned: false,
            fixed_title: None,
            provider: Some(provider),
            launch_config: Some(TerminalLaunchConfig::for_provider(provider)),
            restore_status: TerminalRestoreStatus::NotStarted,
            failure_message: None,
            failure_details: None,
        }
    }

    fn sample_project(id: &str, branch_name: &str) -> Project {
        Project {
            id: id.to_string(),
            repo_id: format!("repo-{id}"),
            name: format!("Project {id}"),
            path: PathBuf::from(format!("/tmp/{id}")),
            kind: ProjectKind::Root,
            checkout: ProjectCheckoutState {
                current_branch: Some(branch_name.to_string()),
                lines_added: 0,
                lines_removed: 0,
            },
            branch_settings: crate::project_store::ProjectBranchSettings::default(),
            actions: Vec::new(),
            worktree_name: None,
            repo_common_dir: None,
        }
    }

    fn sample_project_in_repo(
        id: &str,
        repo_id: &str,
        branch_name: &str,
        worktree_name: Option<&str>,
    ) -> Project {
        let mut project = sample_project(id, branch_name);
        project.repo_id = repo_id.to_string();
        project.worktree_name = worktree_name.map(str::to_string);
        project.repo_common_dir = Some(PathBuf::from(format!("/tmp/{repo_id}/.git")));
        project
    }

    fn sample_task(
        id: &str,
        name: &str,
        kind: TaskKind,
        root_project_id: &str,
        target_project_id: &str,
        branch_name: &str,
        worktree_project_id: Option<&str>,
    ) -> Task {
        Task {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            root_project_id: root_project_id.to_string(),
            target_project_id: target_project_id.to_string(),
            branch_name: branch_name.to_string(),
            section_id: format!("{target_project_id}::{branch_name}::{id}"),
            worktree_project_id: worktree_project_id.map(str::to_string),
            tabs: Vec::new(),
            active_tab_id: String::new(),
            next_tab_id: 1,
            cwd: None,
        }
    }

    #[test]
    fn open_in_target_path_uses_active_changed_file_for_same_project() {
        let project_path = PathBuf::from("/repo");
        let selection = GitDiffSelection {
            project_id: "project-a".to_string(),
            path: "packages/common/package.json".to_string(),
            original_path: None,
            source: GitDiffSource::Unstaged,
            status: 'M',
            additions: 1,
            deletions: 0,
            untracked: false,
        };

        assert_eq!(
            open_in_target_path_for_project("project-a", &project_path, Some(&selection)),
            PathBuf::from("/repo/packages/common/package.json")
        );
    }

    #[test]
    fn open_in_target_path_falls_back_to_project_directory_without_matching_file() {
        let project_path = PathBuf::from("/repo");
        let other_project_selection = GitDiffSelection {
            project_id: "project-b".to_string(),
            path: "packages/common/package.json".to_string(),
            original_path: None,
            source: GitDiffSource::Unstaged,
            status: 'M',
            additions: 1,
            deletions: 0,
            untracked: false,
        };

        assert_eq!(
            open_in_target_path_for_project("project-a", &project_path, None),
            project_path
        );
        assert_eq!(
            open_in_target_path_for_project(
                "project-a",
                &PathBuf::from("/repo"),
                Some(&other_project_selection),
            ),
            PathBuf::from("/repo")
        );
    }

    fn shell_action(name: &str, command: &str) -> ProjectAction {
        ProjectAction {
            id: "action-1".to_string(),
            name: name.to_string(),
            icon: ProjectActionIcon::default(),
            run_on_worktree_create: false,
            scope: ProjectActionScope::default(),
            kind: ProjectActionKind::Shell {
                command: command.to_string(),
            },
        }
    }

    fn terminal_cell(column: usize, ch: char) -> TerminalCellSnapshot {
        TerminalCellSnapshot {
            column,
            width: 1,
            text: ch.to_string(),
            copy_text: ch.to_string(),
            hyperlink: None,
        }
    }

    fn active_toolbar_action(
        action: ToolbarGitAction,
    ) -> (mpsc::Sender<GitActionReply>, ActiveToolbarGitAction) {
        let (tx, receiver) = mpsc::channel();
        (
            tx,
            ActiveToolbarGitAction {
                action,
                branch_name_at_start: Some("feature/test".to_string()),
                receiver,
            },
        )
    }

    #[test]
    fn active_toolbar_git_action_lookup_is_project_scoped() {
        let (_tx, action) = active_toolbar_action(ToolbarGitAction::Fetch);
        let mut active_git_actions = HashMap::new();
        active_git_actions.insert("project-a".to_string(), action);

        assert!(has_active_toolbar_git_action(
            &active_git_actions,
            "project-a"
        ));
        assert!(!has_active_toolbar_git_action(
            &active_git_actions,
            "project-b"
        ));
        assert!(matches!(
            active_toolbar_git_action_entry(&active_git_actions, "project-a")
                .map(|active| &active.action),
            Some(ToolbarGitAction::Fetch)
        ));
        assert!(active_toolbar_git_action_entry(&active_git_actions, "project-b").is_none());
    }

    #[test]
    fn collect_drained_git_action_replies_polls_all_projects() {
        let (project_a_tx, project_a_action) = active_toolbar_action(ToolbarGitAction::Commit);
        let (project_b_tx, project_b_action) = active_toolbar_action(ToolbarGitAction::Fetch);
        let (project_c_tx, project_c_action) =
            active_toolbar_action(ToolbarGitAction::UndoLastCommit);
        let mut active_git_actions = HashMap::new();
        active_git_actions.insert("project-a".to_string(), project_a_action);
        active_git_actions.insert("project-b".to_string(), project_b_action);
        active_git_actions.insert("project-c".to_string(), project_c_action);

        project_a_tx
            .send(GitActionReply::Progress {
                toast_kind: ToastKind::Info,
                toast_message: "project-a progress".to_string(),
            })
            .unwrap();
        project_b_tx
            .send(GitActionReply::Finished {
                project_id: "project-b".to_string(),
                refresh_git_state: false,
                git_state: None,
                toast_kind: ToastKind::Success,
                toast_message: "project-b done".to_string(),
            })
            .unwrap();
        drop(project_c_tx);

        let drained = collect_drained_git_action_replies(&active_git_actions);

        assert!(drained.iter().any(|event| matches!(
            event,
            DrainedGitAction::Reply {
                active_project_id,
                reply: GitActionReply::Progress { toast_message, .. },
            } if active_project_id == "project-a" && toast_message == "project-a progress"
        )));
        assert!(drained.iter().any(|event| matches!(
            event,
            DrainedGitAction::Reply {
                active_project_id,
                reply: GitActionReply::Finished { project_id, .. },
            } if active_project_id == "project-b" && project_id == "project-b"
        )));
        assert!(drained.iter().any(|event| matches!(
            event,
            DrainedGitAction::Disconnected { project_id } if project_id == "project-c"
        )));
        assert!(!drained.iter().any(|event| matches!(
            event,
            DrainedGitAction::Disconnected { project_id } if project_id == "project-a"
        )));
    }

    #[test]
    fn toast_visuals_follow_light_theme_colors() {
        let light = light_theme();
        let (_icon_path, icon_color, icon_bg, border_color, _tone_label) =
            AnotherOneApp::toast_visuals(ToastKind::Info, light);

        assert_eq!(icon_color, light.info.icon);
        assert_eq!(icon_bg, light.info.bg);
        assert_eq!(border_color, light.info.muted);
    }

    #[test]
    fn toast_visuals_follow_dark_theme_colors() {
        let dark = dark_theme();
        let (_icon_path, icon_color, icon_bg, border_color, _tone_label) =
            AnotherOneApp::toast_visuals(ToastKind::Warning, dark);

        assert_eq!(icon_color, dark.warning.icon);
        assert_eq!(icon_bg, dark.warning.bg);
        assert_eq!(border_color, dark.warning.muted);
    }

    #[test]
    fn toast_copy_message_defaults_to_visible_message() {
        let now = Instant::now();
        let toast = AppToast::new(
            1,
            ToastKind::Info,
            "Visible message",
            now,
            now + Duration::from_secs(1),
        );

        assert_eq!(toast.message.as_ref(), "Visible message");
        assert_eq!(toast.copy_message.as_ref(), "Visible message");
    }

    #[test]
    fn toast_copy_message_can_hold_error_details() {
        let now = Instant::now();
        let toast = AppToast::with_copy_message(
            1,
            ToastKind::Error,
            "Could not start daemon.",
            "Could not start daemon: build daemon tokio runtime\n\nCaused by:\n    boom",
            now,
            now + Duration::from_secs(1),
        );

        assert_eq!(toast.message.as_ref(), "Could not start daemon.");
        assert!(toast.copy_message.as_ref().contains("Caused by:"));
        assert_ne!(toast.message, toast.copy_message);
    }

    #[test]
    fn anyhow_error_details_include_context_chain() {
        use anyhow::Context as _;

        let error = Err::<(), _>(anyhow::anyhow!("root cause"))
            .context("outer context")
            .unwrap_err();
        let details = AnotherOneApp::anyhow_error_details(&error);

        assert!(details.contains("outer context"));
        assert!(details.contains("root cause"));
    }

    #[test]
    fn terminal_failure_details_include_recent_output_when_available() {
        let details = AnotherOneApp::terminal_failure_details(
            "Exited with code 2",
            Some("thread 'main' panicked\nstack backtrace:\n   0: example"),
        );

        assert!(details.contains("Exited with code 2"));
        assert!(details.contains("Recent terminal output:"));
        assert!(details.contains("stack backtrace:"));
    }

    #[test]
    fn terminal_failure_details_fall_back_to_status_without_recent_output() {
        let details = AnotherOneApp::terminal_failure_details("Exited with code 2", Some("  \n"));

        assert_eq!(details, "Exited with code 2");
    }

    #[test]
    fn clipboard_image_returns_first_image_entry() {
        let image = Image::from_bytes(ImageFormat::Png, vec![1, 2, 3, 4]);
        let item = ClipboardItem::new_image(&image);

        assert_eq!(AnotherOneApp::clipboard_image(&item), Some(image));
    }

    #[test]
    fn clipboard_image_ignores_text_only_clipboards() {
        let item = ClipboardItem::new_string("hello".to_string());

        assert_eq!(AnotherOneApp::clipboard_image(&item), None);
    }

    #[test]
    fn section_state_restores_active_tab_with_stable_tab_ids() {
        let state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: "99".to_string(),
                next_tab_id: 1,
                cwd: None,
                tabs: vec![
                    shell_tab(0, "Terminal"),
                    PersistedTerminalTab {
                        id: "4".to_string(),
                        title: "Codex".to_string(),
                        pinned: false,
                        fixed_title: None,
                        provider: Some(AgentProviderKind::Codex),
                        launch_config: Some(TerminalLaunchConfig::for_provider(
                            AgentProviderKind::Codex,
                        )),
                        restore_status: TerminalRestoreStatus::NotStarted,
                        failure_message: None,
                        failure_details: None,
                    },
                ],
            },
            Some(PathBuf::from("/tmp/project")),
        );

        assert_eq!(state.active_tab, 1);
        assert_eq!(state.next_tab_id, 1);
        assert_eq!(state.cwd, Some(PathBuf::from("/tmp/project")));
    }

    #[test]
    fn section_state_from_persisted_preserves_empty_tabs() {
        let state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: String::new(),
                next_tab_id: 0,
                cwd: Some(PathBuf::from("/tmp/project")),
                tabs: Vec::new(),
            },
            None,
        );

        assert!(state.tabs.is_empty());
        assert_eq!(state.active_tab, 0);
        assert_eq!(state.next_tab_id, 1);
        assert_eq!(state.cwd, Some(PathBuf::from("/tmp/project")));
    }

    #[test]
    fn section_state_can_close_last_tab() {
        let mut state = SectionState::with_initial_tab(
            Some(PathBuf::from("/tmp/project")),
            TerminalLaunchConfig::default(),
        );
        let only_tab_id = state.tabs[0].id.clone();

        let removed = state.close_tab(0);

        assert_eq!(removed, Some(only_tab_id));
        assert!(state.tabs.is_empty());
        assert_eq!(state.active_tab, 0);
    }

    #[test]
    fn section_state_finds_tabs_to_close_relative_to_anchor() {
        let state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: "b".to_string(),
                next_tab_id: 4,
                cwd: None,
                tabs: vec![
                    shell_tab(0, "A"),
                    agent_tab("b", "B", AgentProviderKind::Codex),
                    agent_tab("c", "C", AgentProviderKind::ClaudeCode),
                    agent_tab("d", "D", AgentProviderKind::Pi),
                ],
            },
            None,
        );

        assert_eq!(
            state.tab_ids_for_close_scope("c", TabCloseScope::Other),
            vec!["0", "b", "d"]
        );
        assert_eq!(
            state.tab_ids_for_close_scope("c", TabCloseScope::Left),
            vec!["0", "b"]
        );
        assert_eq!(
            state.tab_ids_for_close_scope("c", TabCloseScope::Right),
            vec!["d"]
        );
        assert!(state
            .tab_ids_for_close_scope("missing", TabCloseScope::Other)
            .is_empty());
    }

    #[test]
    fn section_state_closes_multiple_tabs_by_id_and_keeps_anchor_when_active_is_removed() {
        let mut state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: "d".to_string(),
                next_tab_id: 4,
                cwd: None,
                tabs: vec![
                    shell_tab(0, "A"),
                    agent_tab("b", "B", AgentProviderKind::Codex),
                    agent_tab("c", "C", AgentProviderKind::ClaudeCode),
                    agent_tab("d", "D", AgentProviderKind::Pi),
                ],
            },
            None,
        );
        let to_close = state.tab_ids_for_close_scope("b", TabCloseScope::Right);

        let removed = state.close_tabs_by_ids(to_close);

        assert_eq!(removed, vec!["c", "d"]);
        assert_eq!(
            state
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["0", "b"]
        );
        assert_eq!(state.active_tab_id(), "b");
    }

    #[test]
    fn new_tab_seed_agent_uses_default_when_section_has_no_tabs() {
        let state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: String::new(),
                next_tab_id: 1,
                cwd: Some(PathBuf::from("/tmp/project")),
                tabs: Vec::new(),
            },
            None,
        );

        assert_eq!(
            new_tab_seed_agent_id(Some(&state), Some("codex")).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn new_tab_seed_agent_uses_default_when_active_tab_is_terminal() {
        let state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: "0".to_string(),
                next_tab_id: 1,
                cwd: Some(PathBuf::from("/tmp/project")),
                tabs: vec![shell_tab(0, "Terminal")],
            },
            None,
        );

        assert_eq!(
            new_tab_seed_agent_id(Some(&state), Some("codex")).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn new_tab_seed_agent_uses_default_when_active_tab_is_another_agent() {
        let state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: "claude".to_string(),
                next_tab_id: 1,
                cwd: Some(PathBuf::from("/tmp/project")),
                tabs: vec![agent_tab(
                    "claude",
                    "Claude Code",
                    AgentProviderKind::ClaudeCode,
                )],
            },
            None,
        );

        assert_eq!(
            new_tab_seed_agent_id(Some(&state), Some("codex")).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn section_state_pinning_moves_tab_before_unpinned_tabs() {
        let mut state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: "b".to_string(),
                next_tab_id: 4,
                cwd: None,
                tabs: vec![
                    shell_tab(0, "A"),
                    PersistedTerminalTab {
                        id: "b".to_string(),
                        title: "B".to_string(),
                        pinned: false,
                        fixed_title: None,
                        provider: None,
                        launch_config: Some(TerminalLaunchConfig::default()),
                        restore_status: TerminalRestoreStatus::NotStarted,
                        failure_message: None,
                        failure_details: None,
                    },
                    PersistedTerminalTab {
                        id: "c".to_string(),
                        title: "C".to_string(),
                        pinned: false,
                        fixed_title: None,
                        provider: None,
                        launch_config: Some(TerminalLaunchConfig::default()),
                        restore_status: TerminalRestoreStatus::NotStarted,
                        failure_message: None,
                        failure_details: None,
                    },
                ],
            },
            None,
        );

        assert!(state.set_tab_pinned(1, true));

        assert_eq!(
            state
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "0", "c"]
        );
        assert_eq!(state.active_tab_id(), "b");
        assert!(state.tab_is_pinned(0));
    }

    #[test]
    fn section_state_unpinning_returns_tab_to_unpinned_group_and_preserves_active_tab() {
        let mut state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: "active".to_string(),
                next_tab_id: 4,
                cwd: None,
                tabs: vec![
                    PersistedTerminalTab {
                        id: "pinned".to_string(),
                        title: "Pinned".to_string(),
                        pinned: true,
                        fixed_title: None,
                        provider: None,
                        launch_config: Some(TerminalLaunchConfig::default()),
                        restore_status: TerminalRestoreStatus::NotStarted,
                        failure_message: None,
                        failure_details: None,
                    },
                    PersistedTerminalTab {
                        id: "active".to_string(),
                        title: "Active".to_string(),
                        pinned: true,
                        fixed_title: None,
                        provider: None,
                        launch_config: Some(TerminalLaunchConfig::default()),
                        restore_status: TerminalRestoreStatus::NotStarted,
                        failure_message: None,
                        failure_details: None,
                    },
                    PersistedTerminalTab {
                        id: "plain".to_string(),
                        title: "Plain".to_string(),
                        pinned: false,
                        fixed_title: None,
                        provider: None,
                        launch_config: Some(TerminalLaunchConfig::default()),
                        restore_status: TerminalRestoreStatus::NotStarted,
                        failure_message: None,
                        failure_details: None,
                    },
                ],
            },
            None,
        );

        assert!(state.set_tab_pinned(1, false));

        assert_eq!(
            state
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["pinned", "active", "plain"]
        );
        assert_eq!(state.active_tab_id(), "active");
        assert!(!state.tab_is_pinned(state.active_tab));
    }

    #[test]
    fn section_state_add_tab_with_launch_config_continues_after_restored_next_tab_id() {
        let mut state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: "0".to_string(),
                next_tab_id: 7,
                cwd: Some(PathBuf::from("/tmp/project")),
                tabs: vec![PersistedTerminalTab {
                    id: "0".to_string(),
                    title: "Pi".to_string(),
                    pinned: false,
                    fixed_title: None,
                    provider: Some(AgentProviderKind::Pi),
                    launch_config: Some(TerminalLaunchConfig::for_provider(AgentProviderKind::Pi)),
                    restore_status: TerminalRestoreStatus::NotStarted,
                    failure_message: None,
                    failure_details: None,
                }],
            },
            None,
        );

        let id = state.add_tab_with_launch_config(
            TerminalLaunchConfig::for_provider(AgentProviderKind::Pi),
            None,
        );

        assert!(!id.is_empty());
        assert_eq!(state.next_tab_id, 8);
        assert_eq!(
            state.tabs[state.active_tab].launch_config,
            TerminalLaunchConfig::for_provider(AgentProviderKind::Pi)
        );
    }

    #[test]
    fn section_state_add_tab_with_launch_config_uses_selected_agent() {
        let mut state = SectionState::with_cwd(Some(PathBuf::from("/tmp/project")));

        let id = state.add_tab_with_launch_config(
            TerminalLaunchConfig::for_provider(AgentProviderKind::ClaudeCode),
            None,
        );

        assert!(!id.is_empty());
        assert_eq!(state.active_tab, 1);
        assert_eq!(state.tabs[1].title, "Claude Code");
        assert_eq!(
            state.tabs[1].launch_config,
            TerminalLaunchConfig::for_provider(AgentProviderKind::ClaudeCode)
        );
    }

    #[test]
    fn shell_project_action_uses_action_name_as_fixed_tab_title() {
        let action = shell_action("  Run tests  ", "cargo test");
        let fixed_title = fixed_title_for_project_action(&action);
        let mut state = SectionState::with_cwd(None);

        let id = state.add_tab_with_launch_config(TerminalLaunchConfig::default(), fixed_title);
        let tab = state
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .expect("action tab should be added");

        assert_eq!(tab.title, "Run tests");
        assert_eq!(tab.fixed_title.as_deref(), Some("Run tests"));
    }

    #[test]
    fn terminal_title_updates_do_not_replace_fixed_tab_title() {
        let mut tab = TerminalTab::with_id(
            "tab-1".to_string(),
            TerminalLaunchConfig::default(),
            Some("Run tests".to_string()),
        );

        apply_terminal_title_update(
            &mut tab,
            &TerminalRuntimeUpdate {
                title: Some("cargo test".to_string()),
                reset_title: false,
                bell: false,
            },
        );
        assert_eq!(tab.title, "Run tests");

        apply_terminal_title_update(
            &mut tab,
            &TerminalRuntimeUpdate {
                title: None,
                reset_title: true,
                bell: false,
            },
        );
        assert_eq!(tab.title, "Run tests");
    }

    #[test]
    fn terminal_title_updates_still_apply_to_normal_tabs() {
        let mut tab =
            TerminalTab::with_id("tab-1".to_string(), TerminalLaunchConfig::default(), None);

        apply_terminal_title_update(
            &mut tab,
            &TerminalRuntimeUpdate {
                title: Some("cargo test".to_string()),
                reset_title: false,
                bell: false,
            },
        );
        assert_eq!(tab.title, "cargo test");

        apply_terminal_title_update(
            &mut tab,
            &TerminalRuntimeUpdate {
                title: None,
                reset_title: true,
                bell: false,
            },
        );
        assert_eq!(tab.title, "Terminal");
    }

    #[test]
    fn global_tab_navigation_targets_follow_task_order_and_tab_order() {
        let projects = vec![
            sample_project_in_repo("root-a", "repo-a", "main", None),
            sample_project_in_repo("root-a-wt", "repo-a", "feature/a2", Some("wt-a2")),
            sample_project_in_repo("root-b", "repo-b", "main", None),
        ];
        let tasks = HashMap::from([
            (
                "root-a".to_string(),
                vec![
                    sample_task(
                        "task-a1",
                        "Task A1",
                        TaskKind::Direct,
                        "root-a",
                        "root-a",
                        "feature/a1",
                        None,
                    ),
                    sample_task(
                        "task-a2",
                        "Task A2",
                        TaskKind::Worktree,
                        "root-a",
                        "root-a-wt",
                        "feature/a2",
                        Some("root-a-wt"),
                    ),
                ],
            ),
            (
                "root-b".to_string(),
                vec![sample_task(
                    "task-b1",
                    "Task B1",
                    TaskKind::Direct,
                    "root-b",
                    "root-b",
                    "feature/b1",
                    None,
                )],
            ),
        ]);
        let section_states = HashMap::from([
            (
                SectionId::for_task("root-a", "main", "task-a1"),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "a1-tab-1".to_string(),
                        next_tab_id: 3,
                        cwd: Some(PathBuf::from("/tmp/root-a")),
                        tabs: vec![
                            PersistedTerminalTab {
                                id: "a1-tab-1".to_string(),
                                title: "Codex".to_string(),
                                pinned: false,
                                fixed_title: None,
                                provider: Some(AgentProviderKind::Codex),
                                launch_config: Some(TerminalLaunchConfig::for_provider(
                                    AgentProviderKind::Codex,
                                )),
                                restore_status: TerminalRestoreStatus::NotStarted,
                                failure_message: None,
                                failure_details: None,
                            },
                            PersistedTerminalTab {
                                id: "a1-tab-2".to_string(),
                                title: "Claude Code".to_string(),
                                pinned: false,
                                fixed_title: None,
                                provider: Some(AgentProviderKind::ClaudeCode),
                                launch_config: Some(TerminalLaunchConfig::for_provider(
                                    AgentProviderKind::ClaudeCode,
                                )),
                                restore_status: TerminalRestoreStatus::NotStarted,
                                failure_message: None,
                                failure_details: None,
                            },
                        ],
                    },
                    None,
                ),
            ),
            (
                SectionId::for_task("root-a-wt", "feature/a2", "task-a2"),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "a2-tab-1".to_string(),
                        next_tab_id: 2,
                        cwd: Some(PathBuf::from("/tmp/root-a-wt")),
                        tabs: vec![PersistedTerminalTab {
                            id: "a2-tab-1".to_string(),
                            title: "Pi".to_string(),
                            pinned: false,
                            fixed_title: None,
                            provider: Some(AgentProviderKind::Pi),
                            launch_config: Some(TerminalLaunchConfig::for_provider(
                                AgentProviderKind::Pi,
                            )),
                            restore_status: TerminalRestoreStatus::NotStarted,
                            failure_message: None,
                            failure_details: None,
                        }],
                    },
                    None,
                ),
            ),
            (
                SectionId::for_task("root-b", "feature/b1", "task-b1"),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "b1-tab-1".to_string(),
                        next_tab_id: 2,
                        cwd: Some(PathBuf::from("/tmp/root-b")),
                        tabs: vec![PersistedTerminalTab {
                            id: "b1-tab-1".to_string(),
                            title: "Terminal".to_string(),
                            pinned: false,
                            fixed_title: None,
                            provider: None,
                            launch_config: Some(TerminalLaunchConfig::default()),
                            restore_status: TerminalRestoreStatus::NotStarted,
                            failure_message: None,
                            failure_details: None,
                        }],
                    },
                    None,
                ),
            ),
        ]);

        let targets =
            global_tab_navigation_targets(&projects, &tasks, &HashSet::new(), &section_states);
        let ordered_targets = targets
            .into_iter()
            .map(|target| {
                (
                    target.section_id.task_id.unwrap_or_default(),
                    target.section_id.project_id,
                    target.tab_index,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            ordered_targets,
            vec![
                ("task-a1".to_string(), "root-a".to_string(), 0),
                ("task-a1".to_string(), "root-a".to_string(), 1),
                ("task-a2".to_string(), "root-a-wt".to_string(), 0),
                ("task-b1".to_string(), "root-b".to_string(), 0),
            ]
        );
    }

    #[test]
    fn next_global_tab_navigation_target_wraps_across_sections() {
        let projects = vec![
            sample_project_in_repo("root-a", "repo-a", "main", None),
            sample_project_in_repo("root-b", "repo-b", "main", None),
        ];
        let tasks = HashMap::from([
            (
                "root-a".to_string(),
                vec![sample_task(
                    "task-a1",
                    "Task A1",
                    TaskKind::Direct,
                    "root-a",
                    "root-a",
                    "feature/a1",
                    None,
                )],
            ),
            (
                "root-b".to_string(),
                vec![sample_task(
                    "task-b1",
                    "Task B1",
                    TaskKind::Direct,
                    "root-b",
                    "root-b",
                    "feature/b1",
                    None,
                )],
            ),
        ]);
        let section_states = HashMap::from([
            (
                SectionId::for_task("root-a", "feature/a1", "task-a1"),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "a1-tab-2".to_string(),
                        next_tab_id: 3,
                        cwd: Some(PathBuf::from("/tmp/root-a")),
                        tabs: vec![
                            PersistedTerminalTab {
                                id: "a1-tab-1".to_string(),
                                title: "Codex".to_string(),
                                pinned: false,
                                fixed_title: None,
                                provider: Some(AgentProviderKind::Codex),
                                launch_config: Some(TerminalLaunchConfig::for_provider(
                                    AgentProviderKind::Codex,
                                )),
                                restore_status: TerminalRestoreStatus::NotStarted,
                                failure_message: None,
                                failure_details: None,
                            },
                            PersistedTerminalTab {
                                id: "a1-tab-2".to_string(),
                                title: "Claude Code".to_string(),
                                pinned: false,
                                fixed_title: None,
                                provider: Some(AgentProviderKind::ClaudeCode),
                                launch_config: Some(TerminalLaunchConfig::for_provider(
                                    AgentProviderKind::ClaudeCode,
                                )),
                                restore_status: TerminalRestoreStatus::NotStarted,
                                failure_message: None,
                                failure_details: None,
                            },
                        ],
                    },
                    None,
                ),
            ),
            (
                SectionId::for_task("root-b", "feature/b1", "task-b1"),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "b1-tab-1".to_string(),
                        next_tab_id: 2,
                        cwd: Some(PathBuf::from("/tmp/root-b")),
                        tabs: vec![PersistedTerminalTab {
                            id: "b1-tab-1".to_string(),
                            title: "Pi".to_string(),
                            pinned: false,
                            fixed_title: None,
                            provider: Some(AgentProviderKind::Pi),
                            launch_config: Some(TerminalLaunchConfig::for_provider(
                                AgentProviderKind::Pi,
                            )),
                            restore_status: TerminalRestoreStatus::NotStarted,
                            failure_message: None,
                            failure_details: None,
                        }],
                    },
                    None,
                ),
            ),
        ]);
        let targets =
            global_tab_navigation_targets(&projects, &tasks, &HashSet::new(), &section_states);

        let next = next_global_tab_navigation_target(
            &targets,
            &projects,
            Some(&SectionId::for_task("root-a", "feature/a1", "task-a1")),
            None,
            Some(1),
            NavigationDirection::Next,
        )
        .map(|target| (target.section_id.task_id.clone(), target.tab_index));
        let previous = next_global_tab_navigation_target(
            &targets,
            &projects,
            Some(&SectionId::for_task("root-a", "feature/a1", "task-a1")),
            None,
            Some(0),
            NavigationDirection::Previous,
        )
        .map(|target| (target.section_id.task_id.clone(), target.tab_index));

        assert_eq!(next, Some((Some("task-b1".to_string()), 0)));
        assert_eq!(previous, Some((Some("task-b1".to_string()), 0)));
    }

    #[test]
    fn next_global_tab_navigation_target_from_project_overview_uses_project_group_tabs() {
        let projects = vec![
            sample_project_in_repo("root-a", "repo-a", "main", None),
            sample_project_in_repo("root-a-wt", "repo-a", "feature/a2", Some("wt-a2")),
            sample_project_in_repo("root-b", "repo-b", "main", None),
        ];
        let tasks = HashMap::from([
            (
                "root-a".to_string(),
                vec![sample_task(
                    "task-a2",
                    "Task A2",
                    TaskKind::Worktree,
                    "root-a",
                    "root-a-wt",
                    "feature/a2",
                    Some("root-a-wt"),
                )],
            ),
            (
                "root-b".to_string(),
                vec![sample_task(
                    "task-b1",
                    "Task B1",
                    TaskKind::Direct,
                    "root-b",
                    "root-b",
                    "feature/b1",
                    None,
                )],
            ),
        ]);
        let section_states = HashMap::from([
            (
                SectionId::for_task("root-a-wt", "feature/a2", "task-a2"),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "a2-tab-1".to_string(),
                        next_tab_id: 2,
                        cwd: Some(PathBuf::from("/tmp/root-a-wt")),
                        tabs: vec![PersistedTerminalTab {
                            id: "a2-tab-1".to_string(),
                            title: "Codex".to_string(),
                            pinned: false,
                            fixed_title: None,
                            provider: Some(AgentProviderKind::Codex),
                            launch_config: Some(TerminalLaunchConfig::for_provider(
                                AgentProviderKind::Codex,
                            )),
                            restore_status: TerminalRestoreStatus::NotStarted,
                            failure_message: None,
                            failure_details: None,
                        }],
                    },
                    None,
                ),
            ),
            (
                SectionId::for_task("root-b", "feature/b1", "task-b1"),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "b1-tab-1".to_string(),
                        next_tab_id: 2,
                        cwd: Some(PathBuf::from("/tmp/root-b")),
                        tabs: vec![PersistedTerminalTab {
                            id: "b1-tab-1".to_string(),
                            title: "Pi".to_string(),
                            pinned: false,
                            fixed_title: None,
                            provider: Some(AgentProviderKind::Pi),
                            launch_config: Some(TerminalLaunchConfig::for_provider(
                                AgentProviderKind::Pi,
                            )),
                            restore_status: TerminalRestoreStatus::NotStarted,
                            failure_message: None,
                            failure_details: None,
                        }],
                    },
                    None,
                ),
            ),
        ]);
        let targets =
            global_tab_navigation_targets(&projects, &tasks, &HashSet::new(), &section_states);

        let next = next_global_tab_navigation_target(
            &targets,
            &projects,
            None,
            Some("root-a"),
            None,
            NavigationDirection::Next,
        )
        .map(|target| target.section_id.task_id.clone());
        let previous = next_global_tab_navigation_target(
            &targets,
            &projects,
            None,
            Some("root-a"),
            None,
            NavigationDirection::Previous,
        )
        .map(|target| target.section_id.task_id.clone());

        assert_eq!(next, Some(Some("task-a2".to_string())));
        assert_eq!(previous, Some(Some("task-a2".to_string())));
    }

    #[test]
    fn sidebar_task_navigation_targets_follow_sidebar_group_order() {
        let projects = vec![
            sample_project_in_repo("root-a", "repo-a", "main", None),
            sample_project_in_repo("root-b", "repo-b", "main", None),
        ];
        let tasks = HashMap::from([
            (
                "root-a".to_string(),
                vec![
                    sample_task(
                        "task-a1",
                        "Task A1",
                        TaskKind::Direct,
                        "root-a",
                        "root-a",
                        "feature/a1",
                        None,
                    ),
                    sample_task(
                        "task-a2",
                        "Task A2",
                        TaskKind::Direct,
                        "root-a",
                        "root-a",
                        "feature/a2",
                        None,
                    ),
                ],
            ),
            (
                "root-b".to_string(),
                vec![sample_task(
                    "task-b1",
                    "Task B1",
                    TaskKind::Direct,
                    "root-b",
                    "root-b",
                    "feature/b1",
                    None,
                )],
            ),
        ]);

        let targets = sidebar_task_navigation_targets(&projects, &tasks, &HashSet::new());
        let ordered_task_ids = targets
            .into_iter()
            .map(|target| target.task_id)
            .collect::<Vec<_>>();

        assert_eq!(ordered_task_ids, vec!["task-a1", "task-a2", "task-b1"]);
    }

    #[test]
    fn sidebar_task_navigation_targets_keep_pinned_tasks_first_within_group() {
        let projects = vec![sample_project_in_repo("root-a", "repo-a", "main", None)];
        let tasks = HashMap::from([(
            "root-a".to_string(),
            vec![
                sample_task(
                    "task-a1",
                    "Task A1",
                    TaskKind::Direct,
                    "root-a",
                    "root-a",
                    "feature/a1",
                    None,
                ),
                sample_task(
                    "task-a2",
                    "Task A2",
                    TaskKind::Direct,
                    "root-a",
                    "root-a",
                    "feature/a2",
                    None,
                ),
                sample_task(
                    "task-a3",
                    "Task A3",
                    TaskKind::Direct,
                    "root-a",
                    "root-a",
                    "feature/a3",
                    None,
                ),
            ],
        )]);
        let pinned = HashSet::from(["task-a2".to_string()]);

        let targets = sidebar_task_navigation_targets(&projects, &tasks, &pinned);
        let ordered_task_ids = targets
            .into_iter()
            .map(|target| target.task_id)
            .collect::<Vec<_>>();

        assert_eq!(ordered_task_ids, vec!["task-a2", "task-a1", "task-a3"]);
    }

    #[test]
    fn next_task_navigation_target_wraps_forward_and_backward() {
        let projects = vec![sample_project_in_repo("root-a", "repo-a", "main", None)];
        let tasks = HashMap::from([(
            "root-a".to_string(),
            vec![
                sample_task(
                    "task-a1",
                    "Task A1",
                    TaskKind::Direct,
                    "root-a",
                    "root-a",
                    "feature/a1",
                    None,
                ),
                sample_task(
                    "task-a2",
                    "Task A2",
                    TaskKind::Direct,
                    "root-a",
                    "root-a",
                    "feature/a2",
                    None,
                ),
                sample_task(
                    "task-a3",
                    "Task A3",
                    TaskKind::Direct,
                    "root-a",
                    "root-a",
                    "feature/a3",
                    None,
                ),
            ],
        )]);
        let targets = sidebar_task_navigation_targets(&projects, &tasks, &HashSet::new());

        let next = next_task_navigation_target(
            &targets,
            &projects,
            Some(&SectionId::for_task("root-a", "feature/a3", "task-a3")),
            None,
            NavigationDirection::Next,
        )
        .map(|target| target.task_id.as_str());
        let previous = next_task_navigation_target(
            &targets,
            &projects,
            Some(&SectionId::for_task("root-a", "feature/a1", "task-a1")),
            None,
            NavigationDirection::Previous,
        )
        .map(|target| target.task_id.as_str());

        assert_eq!(next, Some("task-a1"));
        assert_eq!(previous, Some("task-a3"));
    }

    #[test]
    fn next_task_navigation_target_from_project_overview_uses_first_or_last_task() {
        let projects = vec![
            sample_project_in_repo("root-a", "repo-a", "main", None),
            sample_project_in_repo("root-a-wt", "repo-a", "feature/a2", Some("wt-a2")),
        ];
        let tasks = HashMap::from([(
            "root-a".to_string(),
            vec![
                sample_task(
                    "task-a1",
                    "Task A1",
                    TaskKind::Direct,
                    "root-a",
                    "root-a",
                    "feature/a1",
                    None,
                ),
                sample_task(
                    "task-a2",
                    "Task A2",
                    TaskKind::Worktree,
                    "root-a",
                    "root-a-wt",
                    "feature/a2",
                    Some("root-a-wt"),
                ),
            ],
        )]);
        let targets = sidebar_task_navigation_targets(&projects, &tasks, &HashSet::new());

        let next = next_task_navigation_target(
            &targets,
            &projects,
            None,
            Some("root-a"),
            NavigationDirection::Next,
        )
        .map(|target| target.task_id.as_str());
        let previous = next_task_navigation_target(
            &targets,
            &projects,
            None,
            Some("root-a"),
            NavigationDirection::Previous,
        )
        .map(|target| target.task_id.as_str());

        assert_eq!(next, Some("task-a1"));
        assert_eq!(previous, Some("task-a2"));
    }

    #[test]
    fn sidebar_task_navigation_target_prefers_current_worktree_branch() {
        let root_project = sample_project_in_repo("root-a", "repo-a", "main", None);
        let mut worktree_project =
            sample_project_in_repo("root-a-wt", "repo-a", "feature/new", Some("wt-a2"));
        worktree_project.checkout.current_branch = Some("feature/current".to_string());
        let task = sample_task(
            "task-a2",
            "Task A2",
            TaskKind::Worktree,
            "root-a",
            "root-a-wt",
            "feature/stale",
            Some("root-a-wt"),
        );

        let target = crate::task_launcher::task_workspace_target(
            &[root_project.clone(), worktree_project.clone()],
            &root_project,
            &task,
        )
        .expect("worktree target should resolve");

        assert_eq!(target.project_id, "root-a-wt");
        assert_eq!(target.branch_name, "feature/current");
        assert_eq!(target.project_path, worktree_project.path);
    }

    #[test]
    fn root_project_navigation_targets_follow_sidebar_group_order() {
        let projects = vec![
            sample_project_in_repo("root-a", "repo-a", "main", None),
            sample_project_in_repo("root-a-wt", "repo-a", "feature/a2", Some("wt-a2")),
            sample_project_in_repo("root-b", "repo-b", "main", None),
        ];

        let targets = root_project_navigation_targets(&projects);

        assert_eq!(targets, vec!["root-a", "root-b"]);
    }

    #[test]
    fn next_project_navigation_target_wraps_from_task_to_next_root_project() {
        let projects = vec![
            sample_project_in_repo("root-a", "repo-a", "main", None),
            sample_project_in_repo("root-a-wt", "repo-a", "feature/a2", Some("wt-a2")),
            sample_project_in_repo("root-b", "repo-b", "main", None),
        ];
        let targets = root_project_navigation_targets(&projects);

        let next = next_project_navigation_target(
            &targets,
            &projects,
            Some(&SectionId::for_task("root-a-wt", "feature/a2", "task-a2")),
            None,
        );

        assert_eq!(next, Some("root-b"));
    }

    #[test]
    fn next_project_navigation_target_wraps_between_project_pages() {
        let projects = vec![
            sample_project_in_repo("root-a", "repo-a", "main", None),
            sample_project_in_repo("root-b", "repo-b", "main", None),
        ];
        let targets = root_project_navigation_targets(&projects);

        let next = next_project_navigation_target(&targets, &projects, None, Some("root-b"));

        assert_eq!(next, Some("root-a"));
    }

    #[test]
    fn resolve_new_task_shortcut_target_uses_root_project_for_active_task() {
        let section = SectionId::for_task("root-a-wt", "feature/a2", "task-a2");

        let target = resolve_new_task_shortcut_target(Some(&section), None, |task_id| {
            (task_id == "task-a2").then(|| "root-a".to_string())
        });

        assert_eq!(
            target,
            Some(NewTaskShortcutTarget {
                project_id: "root-a".to_string(),
                source_branch: Some("feature/a2".to_string()),
            })
        );
    }

    #[test]
    fn resolve_new_task_shortcut_target_falls_back_to_project_page() {
        let target = resolve_new_task_shortcut_target(None, Some("root-a"), |_| None);

        assert_eq!(
            target,
            Some(NewTaskShortcutTarget {
                project_id: "root-a".to_string(),
                source_branch: None,
            })
        );
    }

    #[test]
    fn choose_initial_section_prefers_saved_task_section_key() {
        let project = sample_project("project-1", "main");
        let task_section = SectionId::for_task(&project.id, "main", "task-1");
        let section_states = HashMap::from([
            (
                SectionId::new(&project.id, "main"),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "0".to_string(),
                        next_tab_id: 1,
                        cwd: Some(project.path.clone()),
                        tabs: vec![shell_tab(0, "Terminal")],
                    },
                    None,
                ),
            ),
            (
                task_section.clone(),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "task-tab-2".to_string(),
                        next_tab_id: 3,
                        cwd: Some(project.path.clone()),
                        tabs: vec![
                            agent_tab("task-tab-1", "Codex", AgentProviderKind::Codex),
                            agent_tab("task-tab-2", "Claude Code", AgentProviderKind::ClaudeCode),
                        ],
                    },
                    None,
                ),
            ),
        ]);

        let chosen =
            choose_initial_section(&[project], &section_states, Some(&task_section.store_key()));

        assert_eq!(chosen, Some(task_section.clone()));
        assert_eq!(section_states[&task_section].active_tab, 1);
    }

    #[test]
    fn choose_initial_section_ignores_saved_non_task_section_key() {
        let project = sample_project("project-1", "main");
        let main_section = SectionId::new(&project.id, "main");
        let task_section = SectionId::for_task(&project.id, "main", "task-1");
        let section_states = HashMap::from([
            (
                main_section.clone(),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "0".to_string(),
                        next_tab_id: 1,
                        cwd: Some(project.path.clone()),
                        tabs: vec![shell_tab(0, "Terminal")],
                    },
                    None,
                ),
            ),
            (
                task_section.clone(),
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "task-tab-1".to_string(),
                        next_tab_id: 1,
                        cwd: Some(project.path.clone()),
                        tabs: vec![agent_tab(
                            "task-tab-1",
                            "Claude Code",
                            AgentProviderKind::ClaudeCode,
                        )],
                    },
                    None,
                ),
            ),
        ]);

        let chosen =
            choose_initial_section(&[project], &section_states, Some(&main_section.store_key()));

        assert_eq!(chosen, Some(task_section));
    }

    #[test]
    fn choose_initial_section_falls_back_when_saved_task_section_is_missing() {
        let project = sample_project("project-1", "main");
        let task_section = SectionId::for_task(&project.id, "main", "task-1");
        let missing_section = SectionId::for_task(&project.id, "main", "task-missing");
        let section_states = HashMap::from([(
            task_section.clone(),
            SectionState::from_persisted(
                PersistedSectionState {
                    active_tab_id: "task-tab-1".to_string(),
                    next_tab_id: 2,
                    cwd: Some(project.path.clone()),
                    tabs: vec![agent_tab("task-tab-1", "Codex", AgentProviderKind::Codex)],
                },
                None,
            ),
        )]);

        let chosen = choose_initial_section(
            &[project],
            &section_states,
            Some(&missing_section.store_key()),
        );

        assert_eq!(chosen, Some(task_section));
    }

    #[test]
    fn choose_initial_section_falls_back_when_saved_section_key_is_invalid() {
        let project = sample_project("project-1", "main");
        let task_section = SectionId::for_task(&project.id, "main", "task-1");
        let section_states = HashMap::from([(
            task_section,
            SectionState::from_persisted(
                PersistedSectionState {
                    active_tab_id: "0".to_string(),
                    next_tab_id: 1,
                    cwd: Some(project.path.clone()),
                    tabs: vec![shell_tab(0, "Terminal")],
                },
                None,
            ),
        )]);

        let chosen = choose_initial_section(&[project.clone()], &section_states, Some("invalid"));

        assert_eq!(chosen, Some(SectionId::new(&project.id, "main")));
    }

    #[test]
    fn select_active_section_updates_persisted_section_key() {
        let section = SectionId::for_task("project-1", "main", "task-1");
        let mut active_section = None;
        let mut active_project_page = Some("project-1".to_string());

        let changed = select_active_section(
            &mut active_section,
            &mut active_project_page,
            section.clone(),
        );

        assert!(changed);
        assert_eq!(active_section, Some(section.clone()));
        assert_eq!(active_project_page, None);
        assert_eq!(
            persisted_active_section_key(active_section.as_ref()),
            Some(section.store_key())
        );
    }

    #[test]
    fn restored_tabs_stay_lazy_until_runtime_is_requested() {
        let section = SectionId::for_task("project-1", "main", "task-1");
        let state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: "tab-1".to_string(),
                next_tab_id: 2,
                cwd: Some(PathBuf::from("/tmp/project-1")),
                tabs: vec![PersistedTerminalTab {
                    id: "tab-1".to_string(),
                    title: "Claude Code".to_string(),
                    pinned: false,
                    fixed_title: None,
                    provider: Some(AgentProviderKind::ClaudeCode),
                    launch_config: Some(TerminalLaunchConfig::for_provider(
                        AgentProviderKind::ClaudeCode,
                    )),
                    restore_status: TerminalRestoreStatus::NotStarted,
                    failure_message: None,
                    failure_details: None,
                }],
            },
            None,
        );
        let section_states = HashMap::from([(section.clone(), state)]);
        let live_terminal_runtimes = HashMap::<TerminalRuntimeKey, usize>::new();

        assert!(live_terminal_runtimes.is_empty());
        assert_eq!(
            section_states[&section].tabs[0].restore_status,
            TerminalRestoreStatus::NotStarted
        );
    }

    #[test]
    fn async_session_backfill_updates_restored_tab_metadata() {
        let section = SectionId::for_task("project-1", "main", "task-1");
        let key = TerminalRuntimeKey {
            section_id: section.clone(),
            tab_id: "tab-1".to_string(),
        };
        let mut section_states = HashMap::from([(
            section,
            SectionState::from_persisted(
                PersistedSectionState {
                    active_tab_id: "tab-1".to_string(),
                    next_tab_id: 2,
                    cwd: Some(PathBuf::from("/tmp/project-1")),
                    tabs: vec![PersistedTerminalTab {
                        id: "tab-1".to_string(),
                        title: "Codex".to_string(),
                        pinned: false,
                        fixed_title: None,
                        provider: Some(AgentProviderKind::Codex),
                        launch_config: Some(TerminalLaunchConfig::for_provider(
                            AgentProviderKind::Codex,
                        )),
                        restore_status: TerminalRestoreStatus::Launching,
                        failure_message: None,
                        failure_details: None,
                    }],
                },
                None,
            ),
        )]);

        let applied = apply_terminal_session_backfill(
            &mut section_states,
            &key,
            TerminalSessionRef {
                kind: TerminalSessionKind::CodexSession,
                id: "session-42".to_string(),
            },
        );

        assert!(applied);
        assert_eq!(
            section_states[&key.section_id].tabs[0]
                .launch_config
                .session
                .as_ref()
                .map(|session| session.id.as_str()),
            Some("session-42")
        );
    }

    #[test]
    fn closing_tab_cleanup_removes_live_runtime_state() {
        let key = TerminalRuntimeKey {
            section_id: SectionId::for_task("project-1", "main", "task-1"),
            tab_id: "tab-1".to_string(),
        };
        let mut live_terminal_runtimes = HashMap::from([(key.clone(), 7_usize)]);
        let mut terminal_surface_snapshots = HashMap::from([(
            key.clone(),
            TerminalSurfaceSnapshot {
                text: "hello".to_string(),
                columns: 5,
                lines: Vec::new(),
                positioned_runs: Vec::new(),
                cursor: None,
            },
        )]);
        let mut pending_terminal_launches = std::collections::HashSet::from([key.clone()]);
        let mut terminal_recent_output = HashMap::from([(key.clone(), "output".to_string())]);
        let mut terminal_runtime_errors = HashMap::from([(key.clone(), "failed".to_string())]);

        let removed = remove_terminal_runtime_state(
            &mut live_terminal_runtimes,
            &mut terminal_surface_snapshots,
            &mut pending_terminal_launches,
            &mut terminal_recent_output,
            &mut terminal_runtime_errors,
            &key,
        );

        assert_eq!(removed, Some(7));
        assert!(!live_terminal_runtimes.contains_key(&key));
        assert!(!terminal_surface_snapshots.contains_key(&key));
        assert!(!pending_terminal_launches.contains(&key));
        assert!(!terminal_recent_output.contains_key(&key));
        assert!(!terminal_runtime_errors.contains_key(&key));
    }

    #[test]
    fn detects_missing_claude_restore_conversation_output() {
        assert!(agent_output_indicates_missing_session(
            AgentProviderKind::ClaudeCode,
            "Error: No conversation found for session abc123"
        ));
        assert!(!agent_output_indicates_missing_session(
            AgentProviderKind::ClaudeCode,
            "Error: network request failed"
        ));
    }

    #[test]
    fn terminal_selection_range_normalizes_reverse_drag() {
        let selection = terminal_selection_range(
            TerminalCellPosition { line: 3, column: 8 },
            TerminalCellPosition { line: 1, column: 2 },
        );

        assert_eq!(
            selection,
            Some(TerminalSelectionRange {
                start_line: 1,
                start_column: 2,
                end_line: 3,
                end_column: 8,
            })
        );
    }

    #[test]
    fn terminal_selected_text_spans_multiple_lines() {
        let snapshot = TerminalSurfaceSnapshot {
            text: String::new(),
            columns: 6,
            lines: vec![
                TerminalLineSnapshot {
                    text: "hello ".to_string(),
                    cells: vec![
                        TerminalCellSnapshot {
                            column: 0,
                            width: 1,
                            text: "h".to_string(),
                            copy_text: "h".to_string(),
                            hyperlink: None,
                        },
                        TerminalCellSnapshot {
                            column: 1,
                            width: 1,
                            text: "e".to_string(),
                            copy_text: "e".to_string(),
                            hyperlink: None,
                        },
                        TerminalCellSnapshot {
                            column: 2,
                            width: 1,
                            text: "l".to_string(),
                            copy_text: "l".to_string(),
                            hyperlink: None,
                        },
                        TerminalCellSnapshot {
                            column: 3,
                            width: 1,
                            text: "l".to_string(),
                            copy_text: "l".to_string(),
                            hyperlink: None,
                        },
                        TerminalCellSnapshot {
                            column: 4,
                            width: 1,
                            text: "o".to_string(),
                            copy_text: "o".to_string(),
                            hyperlink: None,
                        },
                        TerminalCellSnapshot {
                            column: 5,
                            width: 1,
                            text: " ".to_string(),
                            copy_text: " ".to_string(),
                            hyperlink: None,
                        },
                    ],
                    runs: Vec::new(),
                    background_spans: Vec::new(),
                },
                TerminalLineSnapshot {
                    text: "world ".to_string(),
                    cells: vec![
                        TerminalCellSnapshot {
                            column: 0,
                            width: 1,
                            text: "w".to_string(),
                            copy_text: "w".to_string(),
                            hyperlink: None,
                        },
                        TerminalCellSnapshot {
                            column: 1,
                            width: 1,
                            text: "o".to_string(),
                            copy_text: "o".to_string(),
                            hyperlink: None,
                        },
                        TerminalCellSnapshot {
                            column: 2,
                            width: 1,
                            text: "r".to_string(),
                            copy_text: "r".to_string(),
                            hyperlink: None,
                        },
                        TerminalCellSnapshot {
                            column: 3,
                            width: 1,
                            text: "l".to_string(),
                            copy_text: "l".to_string(),
                            hyperlink: None,
                        },
                        TerminalCellSnapshot {
                            column: 4,
                            width: 1,
                            text: "d".to_string(),
                            copy_text: "d".to_string(),
                            hyperlink: None,
                        },
                        TerminalCellSnapshot {
                            column: 5,
                            width: 1,
                            text: " ".to_string(),
                            copy_text: " ".to_string(),
                            hyperlink: None,
                        },
                    ],
                    runs: Vec::new(),
                    background_spans: Vec::new(),
                },
            ],
            positioned_runs: Vec::new(),
            cursor: None,
        };

        let copied = terminal_selected_text(
            &snapshot,
            TerminalSelectionRange {
                start_line: 0,
                start_column: 2,
                end_line: 1,
                end_column: 3,
            },
        );

        assert_eq!(copied.as_deref(), Some("llo\nworl"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn terminal_open_link_modifier_uses_command_on_macos() {
        assert!(terminal_open_link_modifier_held(Modifiers {
            platform: true,
            ..Modifiers::default()
        }));
        assert!(!terminal_open_link_modifier_held(Modifiers {
            control: true,
            ..Modifiers::default()
        }));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn terminal_open_link_modifier_uses_control_off_macos() {
        assert!(terminal_open_link_modifier_held(Modifiers {
            control: true,
            ..Modifiers::default()
        }));
        assert!(!terminal_open_link_modifier_held(Modifiers {
            platform: true,
            ..Modifiers::default()
        }));
    }

    #[test]
    fn terminal_link_at_position_uses_explicit_hyperlink() {
        let snapshot = TerminalSurfaceSnapshot {
            text: String::new(),
            columns: 5,
            lines: vec![TerminalLineSnapshot {
                text: "click".to_string(),
                cells: "click"
                    .chars()
                    .enumerate()
                    .map(|(column, ch)| {
                        let mut cell = terminal_cell(column, ch);
                        cell.hyperlink = Some("https://example.com/from-osc8".to_string());
                        cell
                    })
                    .collect(),
                runs: Vec::new(),
                background_spans: Vec::new(),
            }],
            positioned_runs: Vec::new(),
            cursor: None,
        };

        let link =
            terminal_link_at_position(&snapshot, TerminalCellPosition { line: 0, column: 2 });

        assert_eq!(link.as_deref(), Some("https://example.com/from-osc8"));
    }

    #[test]
    fn terminal_link_at_position_detects_plain_text_url() {
        let text = "visit https://example.com/path.";
        let snapshot = TerminalSurfaceSnapshot {
            text: String::new(),
            columns: text.len(),
            lines: vec![TerminalLineSnapshot {
                text: text.to_string(),
                cells: text
                    .chars()
                    .enumerate()
                    .map(|(column, ch)| terminal_cell(column, ch))
                    .collect(),
                runs: Vec::new(),
                background_spans: Vec::new(),
            }],
            positioned_runs: Vec::new(),
            cursor: None,
        };

        let link = terminal_link_at_position(
            &snapshot,
            TerminalCellPosition {
                line: 0,
                column: 12,
            },
        );

        assert_eq!(link.as_deref(), Some("https://example.com/path"));
    }

    #[test]
    fn terminal_link_ranges_include_explicit_hyperlinks() {
        let snapshot = TerminalSurfaceSnapshot {
            text: String::new(),
            columns: 5,
            lines: vec![TerminalLineSnapshot {
                text: "click".to_string(),
                cells: "click"
                    .chars()
                    .enumerate()
                    .map(|(column, ch)| {
                        let mut cell = terminal_cell(column, ch);
                        if (1..=3).contains(&column) {
                            cell.hyperlink = Some("https://example.com/from-osc8".to_string());
                        }
                        cell
                    })
                    .collect(),
                runs: Vec::new(),
                background_spans: Vec::new(),
            }],
            positioned_runs: Vec::new(),
            cursor: None,
        };

        assert_eq!(
            terminal_link_ranges(&snapshot),
            vec![TerminalLinkRange {
                line: 0,
                start_column: 1,
                end_column: 4,
            }]
        );
    }

    #[test]
    fn terminal_link_ranges_include_plain_text_urls() {
        let text = "visit https://example.com/path.";
        let snapshot = TerminalSurfaceSnapshot {
            text: String::new(),
            columns: text.len(),
            lines: vec![TerminalLineSnapshot {
                text: text.to_string(),
                cells: text
                    .chars()
                    .enumerate()
                    .map(|(column, ch)| terminal_cell(column, ch))
                    .collect(),
                runs: Vec::new(),
                background_spans: Vec::new(),
            }],
            positioned_runs: Vec::new(),
            cursor: None,
        };

        assert_eq!(
            terminal_link_ranges(&snapshot),
            vec![TerminalLinkRange {
                line: 0,
                start_column: 6,
                end_column: 30,
            }]
        );
    }

    #[test]
    fn terminal_link_ranges_include_ssh_and_git_urls() {
        let text = "clone git://github.com/foo/bar.git from ssh://user@host:22/path";
        let snapshot = TerminalSurfaceSnapshot {
            text: String::new(),
            columns: text.len(),
            lines: vec![TerminalLineSnapshot {
                text: text.to_string(),
                cells: text
                    .chars()
                    .enumerate()
                    .map(|(column, ch)| terminal_cell(column, ch))
                    .collect(),
                runs: Vec::new(),
                background_spans: Vec::new(),
            }],
            positioned_runs: Vec::new(),
            cursor: None,
        };

        let git_start = text.find("git://").unwrap();
        let git_end = git_start + "git://github.com/foo/bar.git".len();
        let ssh_start = text.find("ssh://").unwrap();
        let ssh_end = text.len();

        assert_eq!(
            terminal_link_ranges(&snapshot),
            vec![
                TerminalLinkRange {
                    line: 0,
                    start_column: git_start,
                    end_column: git_end,
                },
                TerminalLinkRange {
                    line: 0,
                    start_column: ssh_start,
                    end_column: ssh_end,
                },
            ]
        );
    }

    #[test]
    fn terminal_link_ranges_include_file_and_mailto() {
        let text = "see file:///etc/hosts or mailto:foo@example.com.";
        let snapshot = TerminalSurfaceSnapshot {
            text: String::new(),
            columns: text.len(),
            lines: vec![TerminalLineSnapshot {
                text: text.to_string(),
                cells: text
                    .chars()
                    .enumerate()
                    .map(|(column, ch)| terminal_cell(column, ch))
                    .collect(),
                runs: Vec::new(),
                background_spans: Vec::new(),
            }],
            positioned_runs: Vec::new(),
            cursor: None,
        };

        let file_start = text.find("file://").unwrap();
        let file_end = file_start + "file:///etc/hosts".len();
        let mailto_start = text.find("mailto:").unwrap();
        let mailto_end = mailto_start + "mailto:foo@example.com".len();

        assert_eq!(
            terminal_link_ranges(&snapshot),
            vec![
                TerminalLinkRange {
                    line: 0,
                    start_column: file_start,
                    end_column: file_end,
                },
                TerminalLinkRange {
                    line: 0,
                    start_column: mailto_start,
                    end_column: mailto_end,
                },
            ]
        );
    }

    #[test]
    fn encode_terminal_mouse_event_sgr_press_and_release() {
        let protocol = TerminalMouseProtocol {
            level: TerminalMouseLevel::ButtonDrag,
            encoding: TerminalMouseEncoding::Sgr,
        };
        let press = encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::Left,
            TerminalMouseAction::Press,
            10, // col
            5,  // row
            TerminalMouseModifiers::default(),
        )
        .expect("press encoded");
        assert_eq!(press, b"\x1b[<0;11;6M".to_vec());

        let release = encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::Left,
            TerminalMouseAction::Release,
            10,
            5,
            TerminalMouseModifiers::default(),
        )
        .expect("release encoded");
        assert_eq!(release, b"\x1b[<0;11;6m".to_vec());
    }

    #[test]
    fn encode_terminal_mouse_event_sgr_drag_carries_motion_bit() {
        let protocol = TerminalMouseProtocol {
            level: TerminalMouseLevel::ButtonDrag,
            encoding: TerminalMouseEncoding::Sgr,
        };
        let drag = encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::Left,
            TerminalMouseAction::Drag,
            0,
            0,
            TerminalMouseModifiers::default(),
        )
        .expect("drag encoded");
        // Left button (0) + motion bit (32) = 32.
        assert_eq!(drag, b"\x1b[<32;1;1M".to_vec());
    }

    #[test]
    fn encode_terminal_mouse_event_sgr_modifiers() {
        let protocol = TerminalMouseProtocol {
            level: TerminalMouseLevel::ButtonDrag,
            encoding: TerminalMouseEncoding::Sgr,
        };
        let event = encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::Right,
            TerminalMouseAction::Press,
            0,
            0,
            TerminalMouseModifiers {
                shift: false,
                alt: true,
                control: true,
            },
        )
        .expect("encoded");
        // Right (2) + alt (8) + control (16) = 26.
        assert_eq!(event, b"\x1b[<26;1;1M".to_vec());
    }

    #[test]
    fn encode_terminal_mouse_event_wheel() {
        let protocol = TerminalMouseProtocol {
            level: TerminalMouseLevel::ClickOnly,
            encoding: TerminalMouseEncoding::Sgr,
        };
        let up = encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::WheelUp,
            TerminalMouseAction::Press,
            3,
            7,
            TerminalMouseModifiers::default(),
        )
        .expect("wheel up");
        assert_eq!(up, b"\x1b[<64;4;8M".to_vec());

        let down = encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::WheelDown,
            TerminalMouseAction::Press,
            3,
            7,
            TerminalMouseModifiers::default(),
        )
        .expect("wheel down");
        assert_eq!(down, b"\x1b[<65;4;8M".to_vec());
    }

    #[test]
    fn encode_terminal_mouse_event_horizontal_wheel() {
        let protocol = TerminalMouseProtocol {
            level: TerminalMouseLevel::ClickOnly,
            encoding: TerminalMouseEncoding::Sgr,
        };
        let left = encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::WheelLeft,
            TerminalMouseAction::Press,
            0,
            0,
            TerminalMouseModifiers::default(),
        )
        .expect("wheel left");
        assert_eq!(left, b"\x1b[<66;1;1M".to_vec());

        let right = encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::WheelRight,
            TerminalMouseAction::Press,
            0,
            0,
            TerminalMouseModifiers::default(),
        )
        .expect("wheel right");
        assert_eq!(right, b"\x1b[<67;1;1M".to_vec());
    }

    #[test]
    fn encode_terminal_mouse_event_default_legacy_clamp() {
        let protocol = TerminalMouseProtocol {
            level: TerminalMouseLevel::ClickOnly,
            encoding: TerminalMouseEncoding::Default,
        };
        let event = encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::Left,
            TerminalMouseAction::Press,
            5,
            10,
            TerminalMouseModifiers::default(),
        )
        .expect("encoded");
        // CSI M, button 0+32=32, col=5+1+32=38, row=10+1+32=43.
        assert_eq!(event, vec![0x1b, b'[', b'M', 32, 38, 43]);
    }

    #[test]
    fn encode_terminal_mouse_event_default_release_uses_button_3() {
        let protocol = TerminalMouseProtocol {
            level: TerminalMouseLevel::ButtonDrag,
            encoding: TerminalMouseEncoding::Default,
        };
        let event = encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::Left,
            TerminalMouseAction::Release,
            0,
            0,
            TerminalMouseModifiers::default(),
        )
        .expect("encoded");
        // CSI M with button=3+32=35, col=33, row=33.
        assert_eq!(event, vec![0x1b, b'[', b'M', 35, 33, 33]);
    }

    #[test]
    fn encode_terminal_mouse_event_motion_requires_any_motion_level() {
        let click_only = TerminalMouseProtocol {
            level: TerminalMouseLevel::ClickOnly,
            encoding: TerminalMouseEncoding::Sgr,
        };
        assert!(encode_terminal_mouse_event(
            click_only,
            TerminalMouseButton::None,
            TerminalMouseAction::Motion,
            0,
            0,
            TerminalMouseModifiers::default(),
        )
        .is_none());
        assert!(encode_terminal_mouse_event(
            click_only,
            TerminalMouseButton::Left,
            TerminalMouseAction::Drag,
            0,
            0,
            TerminalMouseModifiers::default(),
        )
        .is_none());

        let any_motion = TerminalMouseProtocol {
            level: TerminalMouseLevel::AnyMotion,
            encoding: TerminalMouseEncoding::Sgr,
        };
        let motion = encode_terminal_mouse_event(
            any_motion,
            TerminalMouseButton::None,
            TerminalMouseAction::Motion,
            1,
            1,
            TerminalMouseModifiers::default(),
        )
        .expect("any-motion accepts motion");
        // None button (3) + motion bit (32) = 35.
        assert_eq!(motion, b"\x1b[<35;2;2M".to_vec());
    }

    #[test]
    fn encode_terminal_mouse_event_click_only_drops_release() {
        let protocol = TerminalMouseProtocol {
            level: TerminalMouseLevel::ClickOnly,
            encoding: TerminalMouseEncoding::Sgr,
        };
        assert!(encode_terminal_mouse_event(
            protocol,
            TerminalMouseButton::Left,
            TerminalMouseAction::Release,
            0,
            0,
            TerminalMouseModifiers::default(),
        )
        .is_none());
    }

    #[test]
    fn terminal_word_selection_range_selects_clicked_word() {
        let snapshot = TerminalSurfaceSnapshot {
            text: String::new(),
            columns: 11,
            lines: vec![TerminalLineSnapshot {
                text: "foo.bar baz".to_string(),
                cells: "foo.bar baz"
                    .chars()
                    .enumerate()
                    .map(|(column, ch)| TerminalCellSnapshot {
                        column,
                        width: 1,
                        text: ch.to_string(),
                        copy_text: ch.to_string(),
                        hyperlink: None,
                    })
                    .collect(),
                runs: Vec::new(),
                background_spans: Vec::new(),
            }],
            positioned_runs: Vec::new(),
            cursor: None,
        };

        let selection =
            terminal_word_selection_range(&snapshot, TerminalCellPosition { line: 0, column: 5 });

        assert_eq!(
            selection,
            Some(TerminalSelectionRange {
                start_line: 0,
                start_column: 4,
                end_line: 0,
                end_column: 6,
            })
        );
    }

    #[test]
    fn terminal_line_selection_range_selects_full_visual_line() {
        let snapshot = TerminalSurfaceSnapshot {
            text: String::new(),
            columns: 8,
            lines: vec![TerminalLineSnapshot {
                text: "content ".to_string(),
                cells: "content "
                    .chars()
                    .enumerate()
                    .map(|(column, ch)| TerminalCellSnapshot {
                        column,
                        width: 1,
                        text: ch.to_string(),
                        copy_text: ch.to_string(),
                        hyperlink: None,
                    })
                    .collect(),
                runs: Vec::new(),
                background_spans: Vec::new(),
            }],
            positioned_runs: Vec::new(),
            cursor: None,
        };

        let selection =
            terminal_line_selection_range(&snapshot, TerminalCellPosition { line: 0, column: 3 });

        assert_eq!(
            selection,
            Some(TerminalSelectionRange {
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 7,
            })
        );
    }

    #[test]
    fn terminal_scroll_lines_accumulates_fractional_wheel_input() {
        let (first_lines, first_remainder) =
            terminal_scroll_lines(ScrollDelta::Pixels(point(px(0.), px(7.))), px(14.), 0.0);
        let (second_lines, second_remainder) = terminal_scroll_lines(
            ScrollDelta::Pixels(point(px(0.), px(7.))),
            px(14.),
            first_remainder,
        );

        assert_eq!(first_lines, 0);
        assert_eq!(second_lines, 1);
        assert_eq!(second_remainder, 0.0);
    }
}

// ── Render ───────────────────────────────────────────────────────────

// ── Narrow / phone layout helpers ───────────────────────────────────
//
// Keep these off the `Render` trait impl (Render only takes `render`); a
// separate inherent impl lets us reuse them from the wide path too if
// needed in the future. The narrow render reuses the same builder
// functions (`sidebar_content`, `workspace_pane`, `changed_files_panel`)
// as `main_row` — that's how "same source, no GUI drift" stays true.
impl AnotherOneApp {
    /// Phone-style top bar shown above the active narrow pane. Three slots:
    /// a back chevron when there's nav history (otherwise a hamburger that
    /// opens settings), a centered title, and a contextual right action.
    /// Top action row for the narrow Projects view: a Pair-mobile
    /// chip and a Settings chip prepended above the project tree.
    /// These are the affordances that lived in the global phone
    /// header before; with no shared chrome they live inline at the
    /// top of the view that owns them.
    fn narrow_home_actions(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let chrome = theme::chrome_bg(window);
        let pair = div()
            .id("mobile-home-pair")
            .flex()
            .items_center()
            .justify_center()
            .h(px(28.))
            .px(px(12.))
            .rounded(px(6.))
            .bg(hsla(215. / 360., 0.55, 0.45, 1.0))
            .cursor_pointer()
            .text_size(rems(12. / 16.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(gpui::white())
            .child(SharedString::from("Scan"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.start_qr_scan(cx);
                    cx.stop_propagation();
                }),
            );
        let settings = div()
            .id("mobile-home-settings")
            .flex()
            .items_center()
            .justify_center()
            .h(px(28.))
            .px(px(12.))
            .rounded(px(6.))
            .cursor_pointer()
            .text_size(rems(13. / 16.))
            .text_color(gpui::white().opacity(0.92))
            .child(SharedString::from("⚙ Settings"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.settings_open = true;
                    cx.stop_propagation();
                    cx.notify();
                }),
            );
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .h(px(PHONE_HEADER_H))
            .px(px(10.))
            .flex_shrink_0()
            .bg(chrome)
            .child(pair)
            .child(settings)
            .into_any_element()
    }

    /// Narrow workspace strip: `‹  task name  Δ` rendered as the
    /// first child of the workspace body. `‹` pops back to Projects;
    /// `Δ` pushes Changed Files. No global header — this is part of
    /// the workspace view's own content.
    fn narrow_workspace_strip(
        &self,
        project_id: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chrome = theme::chrome_bg(window);
        let title: String = self
            .project_store
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| project_id.to_string());
        // Reuse the desktop sidebar-toggle SVGs so the gutter icons
        // are visually identical across desktop chrome and the
        // narrow workspace strip.
        let back = div()
            .id("mobile-workspace-back")
            .flex()
            .items_center()
            .justify_center()
            .w(px(44.))
            .h(px(PHONE_HEADER_H))
            .cursor_pointer()
            .child(Self::sidebar_toggle_svg(
                window,
                self.project_store.ui.theme_mode,
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                    this.mobile_back(cx);
                    cx.stop_propagation();
                }),
            );
        let diffs = div()
            .id("mobile-workspace-diffs")
            .flex()
            .items_center()
            .justify_center()
            .w(px(44.))
            .h(px(PHONE_HEADER_H))
            .cursor_pointer()
            .child(Self::right_sidebar_toggle_svg(
                window,
                self.project_store.ui.theme_mode,
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                    this.mobile_push(MobileView::ChangedFiles, cx);
                    cx.stop_propagation();
                }),
            );
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(PHONE_HEADER_H))
            .flex_shrink_0()
            .bg(chrome)
            .child(back)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(rems(15. / 16.))
                    .text_color(gpui::white().opacity(0.92))
                    .child(SharedString::from(title)),
            )
            .child(diffs)
            .into_any_element()
    }

    /// Narrow Changed Files strip: `‹  Changed files`. `‹` pops back
    /// to the workspace.
    fn narrow_changed_files_strip(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let chrome = theme::chrome_bg(window);
        let back = div()
            .id("mobile-changed-files-back")
            .flex()
            .items_center()
            .justify_center()
            .w(px(44.))
            .h(px(PHONE_HEADER_H))
            .cursor_pointer()
            .child(Self::sidebar_toggle_svg(
                window,
                self.project_store.ui.theme_mode,
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                    this.mobile_back(cx);
                    cx.stop_propagation();
                }),
            );
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(PHONE_HEADER_H))
            .flex_shrink_0()
            .bg(chrome)
            .child(back)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(rems(15. / 16.))
                    .text_color(gpui::white().opacity(0.92))
                    .child(SharedString::from("Changed files")),
            )
            // Right-side spacer keeps the title visually centered.
            .child(div().w(px(44.)).h(px(PHONE_HEADER_H)))
            .into_any_element()
    }

    /// Build the narrow render tree: phone header on top, a single body
    /// pane chosen by `mobile_view`, then the same overlays/modals/toasts
    /// the wide path appends. Same `AppInputHost` wrapper so global key
    /// dispatch / focus / mouse handlers behave identically.
    fn render_narrow(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        view: Entity<Self>,
    ) -> AppInputHost {
        let supports_custom_chrome =
            crate::platform::CurrentPlatform::supports_custom_chrome(window);
        // Each `MobileView` owns its own minimal top strip — no
        // shared phone header. Projects view exposes the
        // settings + pair-mobile entry points inline; the workspace
        // view prepends a `‹ task-name Δ` strip whose `‹` returns to
        // Projects and `Δ` jumps to Changed Files; Changed Files
        // prepends a `‹ Changed files` strip.
        let body: AnyElement = match self.mobile_view.clone() {
            MobileView::Home => div()
                .flex()
                .flex_col()
                .size_full()
                .overflow_hidden()
                .child(self.narrow_home_actions(window, cx))
                .child(self.sidebar_content(window, cx))
                .into_any_element(),
            MobileView::Project(project_id) => div()
                .flex()
                .flex_col()
                .size_full()
                .child(self.narrow_workspace_strip(&project_id, window, cx))
                .child(self.workspace_pane.clone())
                .into_any_element(),
            MobileView::ChangedFiles => div()
                .flex()
                .flex_col()
                .size_full()
                .child(self.narrow_changed_files_strip(window, cx))
                .child(self.changed_files_panel(window, cx))
                .into_any_element(),
        };
        let body = div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(body)
            .into_any_element();

        AppInputHost::new(
            div()
                .flex()
                .flex_col()
                .relative()
                .size_full()
                .track_focus(&self.focus_handle)
                .when(supports_custom_chrome, |d| d.bg(theme::chrome_bg(window)))
                .on_mouse_move(cx.listener(Self::on_mouse_move))
                .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                .on_key_down(cx.listener(Self::handle_global_key_down))
                .on_action(cx.listener(Self::zoom_in))
                .on_action(cx.listener(Self::zoom_out))
                .on_action(cx.listener(Self::zoom_reset))
                .on_action(cx.listener(Self::next_tab))
                .on_action(cx.listener(Self::previous_tab))
                .on_action(cx.listener(Self::next_task))
                .on_action(cx.listener(Self::previous_task))
                .on_action(cx.listener(Self::next_project))
                .on_action(cx.listener(Self::new_tab))
                .on_action(cx.listener(Self::new_task))
                .on_action(cx.listener(Self::handle_terminal_find))
                .on_action(cx.listener(Self::handle_terminal_search_close))
                .on_action(cx.listener(Self::handle_terminal_search_next))
                .on_action(cx.listener(Self::handle_terminal_search_prev))
                .child(body)
                .child(self.new_task_modal_overlay(cx))
                .child(self.create_branch_modal_overlay(cx))
                .child(self.add_agent_modal_overlay(cx))
                .child(self.custom_action_modal_overlay(cx))
                .child(self.project_remove_confirm_modal(cx))
                .child(self.sidebar_task_delete_confirm_modal(cx))
                .child(self.pinned_tab_close_confirm_modal(window, cx))
                .child(self.pair_mobile_overlay(cx))
                .child(self.toast_layer(cx)),
            self.focus_handle.clone(),
            view,
        )
    }
}

impl AnotherOneApp {
    /// Trigger the QR scanner. On Android this dispatches to the Kotlin
    /// `QrScanLauncher` via JNI; on other platforms it just toasts that
    /// the feature is mobile-only. Successful scans land in
    /// `mobile::QR_SCAN_QUEUE` and are picked up by the render tick
    /// (`drain_qr_scan_queue`) which dispatches a follow-up action.
    pub fn start_qr_scan(&mut self, cx: &mut Context<Self>) {
        match mobile::launch_qr_scanner() {
            Ok(()) => {
                log::info!("QR scanner launched");
            }
            Err(err) => {
                self.show_error_toast(format!("Couldn't open scanner: {err}"), cx);
            }
        }
    }

    /// Drain any URLs the JNI scan callback queued. Each URL kicks off
    /// an iroh client dial on a dedicated tokio runtime; status events
    /// from the dial flow back via [`drain_iroh_dial_status`].
    pub fn drain_qr_scan_queue(&mut self, cx: &mut Context<Self>) -> bool {
        let urls = mobile::drain_qr_scan_results();
        if urls.is_empty() {
            return false;
        }
        for url in urls {
            self.show_info_toast(format!("Dialing daemon at {url}"), cx);
            crate::iroh_client::dial(url);
        }
        true
    }

    /// Drain queued daemon worker replies — currently only
    /// `WorkerReply::ProjectList`, which the daemon-client session
    /// treats as the authoritative project tree. Replaces the local
    /// `project_store` snapshot wholesale; the existing sidebar
    /// renderer picks the data up via the same code path desktop
    /// uses, so there is no separate "remote" component.
    pub fn drain_remote_worker_replies(&mut self, cx: &mut Context<Self>) -> bool {
        let replies = crate::iroh_client::drain_worker_replies();
        if replies.is_empty() {
            return false;
        }
        let mut changed = false;
        for reply in replies {
            match reply {
                daemon_proto::WorkerReply::ProjectList {
                    projects: summaries,
                    repos: repo_summaries,
                    ui,
                } => {
                    let task_total: usize = summaries.iter().map(|p| p.tasks.len()).sum();
                    log::info!(
                        "daemon ProjectList: {} project(s), {} repo(s), {} task(s) total, ui pinned={} expanded={} last_active={:?}",
                        summaries.len(),
                        repo_summaries.len(),
                        task_total,
                        ui.pinned_task_ids.len(),
                        ui.expanded_repo_ids.len(),
                        ui.last_active_section_id
                    );
                    // Single absorb path — both clients call this same
                    // method on the same wire types. No more parallel
                    // convert_remote_snapshot / set_remote_snapshot
                    // pair; no more lossy fabrication of defaults.
                    self.project_store
                        .absorb_projection(summaries, repo_summaries, ui);
                    log::info!(
                        "post-absorb: store has {} projects and tasks for {} root projects",
                        self.project_store.projects.len(),
                        self.project_store.tasks.len()
                    );
                    // The sidebar's expand chevron is rendered with an
                    // SVG that doesn't load on Android (no asset
                    // bundling yet), so a freshly-seeded store would
                    // hide tasks behind an invisible toggle. Pre-mark
                    // every project that has at least one task as
                    // expanded so tasks show inline beneath each
                    // project — same code path desktop uses when the
                    // user has manually expanded a row.
                    for project in &self.project_store.projects {
                        if self
                            .project_store
                            .tasks
                            .get(&project.id)
                            .is_some_and(|tasks| !tasks.is_empty())
                        {
                            self.expanded_projects.insert(project.repo_id.clone());
                        }
                    }
                    changed = true;
                }
                // The daemon may push WorkerReply variants the
                // desktop GUI doesn't consume (it sources most of
                // its state in-process). Ignore them — MCP clients
                // and mobile pick up the same variants directly.
                _ => {}
            }
        }
        if changed {
            cx.notify();
        }
        changed
    }

    /// Drain queued iroh-client dial status events into toasts so the
    /// connection progress is visible to the user without standing up
    /// a separate status panel yet.
    pub fn drain_iroh_dial_status(&mut self, cx: &mut Context<Self>) -> bool {
        let events = crate::iroh_client::drain_dial_status();
        if events.is_empty() {
            return false;
        }
        for ev in events {
            match ev {
                crate::iroh_client::DialStatus::Started { endpoint_id } => {
                    self.show_info_toast(format!("Connecting to {}…", short_id(&endpoint_id)), cx);
                }
                crate::iroh_client::DialStatus::Bound => {
                    self.show_info_toast("Endpoint bound", cx);
                }
                crate::iroh_client::DialStatus::Connected => {
                    self.show_info_toast("Daemon connected", cx);
                }
                crate::iroh_client::DialStatus::HelloSent => {
                    self.show_info_toast("Pairing hello sent", cx);
                }
                crate::iroh_client::DialStatus::Error(err) => {
                    self.show_error_toast(format!("Pairing failed: {err}"), cx);
                }
            }
        }
        true
    }
}


fn short_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…{}", &id[..6], &id[id.len() - 4..])
    } else {
        id.to_string()
    }
}

fn word_start_before(text: &str, cursor: usize) -> usize {
    let prefix = &text[..cursor];
    let trimmed = prefix.trim_end_matches(char::is_whitespace);
    let after_ws = trimmed.len();
    match trimmed.rfind(char::is_whitespace) {
        Some(idx) => {
            let mut i = idx;
            while !trimmed.is_char_boundary(i) {
                i += 1;
            }
            let after = trimmed[i..].chars().next().map_or(i, |c| i + c.len_utf8());
            after.min(after_ws)
        }
        None => 0,
    }
}

impl Render for AnotherOneApp {
    #[hotpath::measure]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().clone();
        // Start terminal refresh timer once.
        if !self.refresh_timer_started {
            self.refresh_timer_started = true;
            let handle = cx.entity().clone();
            let initial_interval = self.refresh_timer_interval();
            window
                .spawn(cx, async move |async_cx| {
                    let mut interval = initial_interval;
                    loop {
                        async_cx.background_executor().timer(interval).await;
                        let next_interval = handle.update(async_cx, |this, cx| {
                            let mut should_notify = false;
                            should_notify |= this.drain_git_actions(cx);
                            should_notify |= this.drain_changed_files_git_mutations(cx);
                            should_notify |= this.drain_changed_file_diff(cx);
                            should_notify |= this.drain_git_refresh(cx);
                            should_notify |= this.drain_new_task_branch_refresh(cx);
                            should_notify |= this.drain_task_creation(cx);
                            should_notify |= this.drain_branch_creation(cx);
                            should_notify |= this.drain_project_add(cx);
                            should_notify |= this.drain_worktree_deletions(cx);
                            should_notify |= this.drain_commit_file_changes(cx);
                            should_notify |= this.drain_project_github_link_lookup();
                            should_notify |= this.drain_project_pull_request_lookup(cx);
                            should_notify |= this.drain_project_page_pull_requests(cx);
                            should_notify |= this.drain_project_check_runs_lookup(cx);
                            should_notify |= this.drain_pending_session_handoff();
                            should_notify |= this.drain_session_events(cx);
                            let terminal_launched = this.drain_terminal_launch_replies(cx);
                            let warm_launched = this.drain_warm_terminal_launch_replies(cx);
                            should_notify |= terminal_launched;
                            should_notify |= warm_launched;
                            // Any terminal-launch state change may have
                            // added/removed tabs or flipped their
                            // running status; re-snapshot the store
                            // for the daemon's `ListProjects` path.
                            if terminal_launched || warm_launched {
                                this.sync_registry_project_store();
                            }
                            should_notify |= this.drain_daemon_handle(cx);
                            should_notify |= this.drain_pending_spawn_terminals(cx);
                            should_notify |= this.drain_pending_close_tabs(cx);
                            should_notify |= this.drain_pending_select_focus(cx);
                            should_notify |= this.drain_pending_ui_actions(cx);
                            // Observe GUI-driven focus changes after
                            // the spawn/close/select drains so any
                            // event WE just produced isn't double-
                            // emitted as if the user clicked it.
                            this.observe_gui_focus(cx);
                            should_notify |= this.drain_gui_events(cx);
                            should_notify |= this.drain_pending_tab_launches(cx);
                            should_notify |= this.drain_pending_tab_resizes(cx);
                            should_notify |= this.drain_qr_scan_queue(cx);
                            should_notify |= this.drain_iroh_dial_status(cx);
                            should_notify |= this.drain_remote_worker_replies(cx);
                            should_notify |= this.drain_updater_events(cx);
                            should_notify |= this.drain_terminal_drag_autoscroll(cx);
                            should_notify |= this.tick_toasts();
                            should_notify |= this.tick_resource_usage();
                            should_notify |= this.tick_pasted_image_preview(cx);
                            this.maybe_schedule_active_git_refresh(cx);
                            if should_notify {
                                cx.notify();
                            }
                            this.refresh_timer_interval()
                        });
                        interval = next_interval;
                    }
                })
                .detach();
        }
        // Scale the entire UI based on the zoom level (font_size relative to default 13.0).
        let scale = self.font_size / 13.0;
        window.set_rem_size(px(16.0 * scale));

        let theme_mode = self.project_store.ui.theme_mode;
        let chrome_bg = theme::chrome_bg_for_mode(window, theme_mode);
        // Publish the resolved theme so non-GPUI renderers (terminal cell
        // resolver) can pick theme-aware default fg/bg colors. If System
        // resolves differently from the last frame, rebuild snapshots because
        // their default fg/bg colors are baked into cached TextRuns.
        let resolved_theme = theme::resolve_theme(window, theme_mode);
        if theme::current_terminal_theme() != resolved_theme {
            theme::set_terminal_theme(resolved_theme);
            self.terminal_surface_snapshots.clear();
            for (key, runtime) in &mut self.live_terminal_runtimes {
                runtime.invalidate_snapshot();
                self.terminal_surface_snapshots
                    .insert(key.clone(), runtime.snapshot());
            }
        } else {
            theme::set_terminal_theme(resolved_theme);
        }

        // ── Settings page (replaces normal layout) ─────────────
        if self.settings_open {
            let settings = self.render_settings_page(window, cx);
            let supports_custom_chrome =
                crate::platform::CurrentPlatform::supports_custom_chrome(window);
            return AppInputHost::new(
                div()
                    .flex()
                    .flex_col()
                    .relative()
                    .size_full()
                    .track_focus(&self.focus_handle)
                    .when(supports_custom_chrome, |d| d.bg(chrome_bg))
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_key_down(cx.listener(Self::handle_global_key_down))
                    .on_action(cx.listener(Self::zoom_in))
                    .on_action(cx.listener(Self::zoom_out))
                    .on_action(cx.listener(Self::zoom_reset))
                    .on_action(cx.listener(Self::next_tab))
                    .on_action(cx.listener(Self::previous_tab))
                    .on_action(cx.listener(Self::next_task))
                    .on_action(cx.listener(Self::previous_task))
                    .on_action(cx.listener(Self::next_project))
                    .on_action(cx.listener(Self::new_tab))
                    .on_action(cx.listener(Self::new_task))
                    .child(settings)
                    .child(self.toast_layer(cx)),
                self.focus_handle.clone(),
                view.clone(),
            );
        }

        // ── Per-tick state reconciliation, regardless of breakpoint ──
        // Both wide and narrow renders need these. Keeping them above
        // the narrow/wide fork is the single-source-of-truth invariant
        // the whole responsive layout rests on: behaviour stays
        // identical when the same window resizes across the
        // breakpoint, and there's no mobile/desktop drift.
        // `clamp_layout` already early-returns under narrow, and
        // `ensure_active_terminal_runtime` / `sync_workspace_layout`
        // are layout-agnostic by design.
        self.clamp_layout(window);
        self.sync_workspace_layout(cx);
        self.ensure_active_terminal_runtime(window, cx);
        self.request_active_project_github_link_lookup(cx);

        // ── Narrow (phone) layout ───────────────────────────────
        // Single-pane master/detail stack. Same builder functions as
        // the wide path — only the parent container and visibility
        // logic differ.
        if self.is_narrow(window) {
            return self.render_narrow(window, cx, view);
        }

        // ── Normal main layout ──────────────────────────────────
        let sw = self.sidebar_w;
        let rw = self.right_w;
        let open = self.sidebar_is_open();
        let busy = self.animating;

        let main = self.main_row(window, cx, sw, rw, open, busy);

        let footer = div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(FOOTER_H))
            .flex_shrink_0()
            .bg(chrome_bg)
            // Left section: fixed width matching sidebar
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(10.))
                    .flex_shrink_0()
                    .w(px(sw))
                    .child(self.footer_settings_button(window, cx))
                    .child(self.footer_add_project_button(window, cx))
                    .when(
                        matches!(
                            self.updater_state,
                            crate::updater::UpdateState::ReadyToInstall { .. }
                        ),
                        |d| d.child(self.footer_install_update_button(window, cx)),
                    ),
            )
            // Right section: branch + worktree
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_1()
                    .gap(px(8.))
                    .pl(px(GUTTER + 4.))
                    .pr(px(10.))
                    .child(self.footer_branch_indicator(window, cx))
                    .child(self.footer_worktree_indicator(window, cx)),
            );

        let supports_custom_chrome =
            crate::platform::CurrentPlatform::supports_custom_chrome(window);
        let footer = if supports_custom_chrome {
            footer
        } else {
            footer.child(self.resource_indicator_button(window, cx))
        };

        AppInputHost::new(
            div()
                .flex()
                .flex_col()
                .relative()
                .size_full()
                .track_focus(&self.focus_handle)
                .when(supports_custom_chrome, |d| d.bg(chrome_bg))
                .on_mouse_move(cx.listener(Self::on_mouse_move))
                .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                .on_key_down(cx.listener(Self::handle_global_key_down))
                .on_action(cx.listener(Self::zoom_in))
                .on_action(cx.listener(Self::zoom_out))
                .on_action(cx.listener(Self::zoom_reset))
                .on_action(cx.listener(Self::next_tab))
                .on_action(cx.listener(Self::previous_tab))
                .on_action(cx.listener(Self::next_task))
                .on_action(cx.listener(Self::previous_task))
                .on_action(cx.listener(Self::next_project))
                .on_action(cx.listener(Self::new_tab))
                .on_action(cx.listener(Self::new_task))
                .on_action(cx.listener(Self::handle_terminal_find))
                .on_action(cx.listener(Self::handle_terminal_search_close))
                .on_action(cx.listener(Self::handle_terminal_search_next))
                .on_action(cx.listener(Self::handle_terminal_search_prev))
                .when(supports_custom_chrome, |d| {
                    d.child(self.custom_title_strip(window, cx, busy))
                })
                .child(main)
                .child(footer)
                .when(supports_custom_chrome, |d| {
                    d.child(self.titlebar_custom_actions_overlay(cx))
                        .child(self.titlebar_open_in_overlay(cx))
                        .child(self.titlebar_git_actions_overlay(cx))
                })
                .child(self.resource_indicator_overlay(window, cx))
                .child(self.project_menu_overlay(sw, cx))
                .child(self.sidebar_task_menu_overlay(window, cx))
                .child(self.terminal_tab_menu_overlay(window, cx))
                .child(self.terminal_context_menu_overlay(window, cx))
                .child(self.terminal_search_bar_overlay(window, cx))
                .child(self.new_task_modal_overlay(cx))
                .child(self.create_branch_modal_overlay(cx))
                .child(self.add_agent_modal_overlay(cx))
                .child(self.custom_action_modal_overlay(cx))
                .child(self.project_remove_confirm_modal(cx))
                .child(self.sidebar_task_delete_confirm_modal(cx))
                .child(self.pinned_tab_close_confirm_modal(window, cx))
                .child(self.pair_mobile_overlay(cx))
                .child(self.toast_layer(cx)),
            self.focus_handle.clone(),
            view,
        )
    }
}
