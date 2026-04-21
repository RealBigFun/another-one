//! Application state, core event handlers, animation, and `Render` impl.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    actions, div, hsla, prelude::*, px, rems, rgb, svg, AnyElement, AnyView, App, Bounds,
    ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, Focusable, GlobalElementId, InspectorElementId, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, SharedString, Timer,
    UTF16Selection, WeakEntity, Window,
};

actions!(zoom, [ZoomIn, ZoomOut, ZoomReset]);

use crate::agents::{
    terminal_launch_config_for_selected_agent, terminal_launch_config_for_selected_agents,
    AgentProviderKind, TerminalLaunchConfig, TerminalRestoreStatus, TerminalSessionRef, AGENTS,
};
use crate::layout::*;
use crate::project_store::{
    ChangedFile, PersistedSectionState, PersistedTerminalTab, ProjectGitState, ProjectStore, Task,
    TaskKind,
};
use crate::terminal_launch::{
    spawn_terminal_launch, spawn_warm_terminal_launch, TerminalLaunchReply, WarmTerminalLaunchReply,
};
use crate::terminal_runtime::{
    LiveTerminalRuntime, TerminalGridSize, TerminalRuntimeKey, TerminalSurfaceSnapshot,
};
use crate::theme;

const ACTIVE_GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(4);
const ACTIVE_GIT_METADATA_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const IDLE_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const TOAST_ANIMATION_REFRESH_INTERVAL: Duration = Duration::from_millis(16);
const TOAST_LIFETIME: Duration = Duration::from_secs(4);
const TOAST_ERROR_EXTRA_LIFETIME: Duration = Duration::from_secs(3);
const TOAST_FADE_IN: Duration = Duration::from_millis(220);
const TOAST_FADE_OUT: Duration = Duration::from_millis(220);
const TOAST_STACK_LIMIT: usize = 4;
const TOAST_SWIPE_DISMISS_THRESHOLD: f32 = 120.;
const TOAST_COPY_FEEDBACK: Duration = Duration::from_millis(1200);
pub(crate) const SIDEBAR_TASK_DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(400);
const PROJECT_EXPAND_ANIMATION_DURATION: Duration = Duration::from_millis(160);
const PROJECT_EXPAND_ANIMATION_STEP: Duration = Duration::from_millis(16);
const TERMINAL_RECENT_OUTPUT_LIMIT: usize = 16 * 1024;

fn output_mentions_missing_claude_conversation(text: &str) -> bool {
    text.to_ascii_lowercase().contains("no conversation found")
}

fn trim_to_recent_output_limit(buffer: &mut String) {
    if buffer.len() <= TERMINAL_RECENT_OUTPUT_LIMIT {
        return;
    }

    let min_start = buffer.len() - TERMINAL_RECENT_OUTPUT_LIMIT;
    let start = buffer
        .char_indices()
        .map(|(idx, _)| idx)
        .find(|&idx| idx >= min_start)
        .unwrap_or(buffer.len());

    buffer.drain(..start);
}

/// Identifies a section: a specific branch within a specific project.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SectionId {
    pub project_id: String,
    pub branch_name: String,
    pub task_id: Option<String>,
}

impl SectionId {
    pub fn new(project_id: &str, branch_name: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
            branch_name: branch_name.to_string(),
            task_id: None,
        }
    }

    pub fn for_task(project_id: &str, branch_name: &str, task_id: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
            branch_name: branch_name.to_string(),
            task_id: Some(task_id.to_string()),
        }
    }

    pub fn store_key(&self) -> String {
        format!(
            "{}::{}::{}",
            self.project_id,
            self.branch_name,
            self.task_id.as_deref().unwrap_or("")
        )
    }

    pub fn from_store_key(key: &str) -> Option<Self> {
        let mut parts = key.splitn(3, "::");
        let project_id = parts.next()?.to_string();
        let branch_name = parts.next()?.to_string();
        let task_id = parts.next()?.to_string();

        Some(Self {
            project_id,
            branch_name,
            task_id: (!task_id.is_empty()).then_some(task_id),
        })
    }
}

/// A single terminal tab within a section.
pub struct TerminalTab {
    pub id: String,
    pub title: String,
    pub launch_config: TerminalLaunchConfig,
    pub restore_status: TerminalRestoreStatus,
}

struct PrewarmedTerminalLaunch {
    cwd: std::path::PathBuf,
    launch_config: TerminalLaunchConfig,
    attached_tab: Option<TerminalRuntimeKey>,
    runtime: Option<LiveTerminalRuntime>,
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

    pub fn add_tab_with_launch_config(&mut self, launch_config: TerminalLaunchConfig) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.next_tab_id += 1;
        let new_tab = TerminalTab::with_id(id.clone(), launch_config);
        self.tabs.push(new_tab);
        self.active_tab = self.tabs.len() - 1;
        id
    }

    pub fn close_tab(&mut self, index: usize) -> Option<String> {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return None; // keep at least one tab
        }

        let removed = self.tabs.remove(index);
        if index < self.active_tab {
            self.active_tab = self.active_tab.saturating_sub(1);
        }
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        Some(removed.id)
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
        let mut tabs = persisted
            .tabs
            .into_iter()
            .map(TerminalTab::from_persisted)
            .collect::<Vec<_>>();

        if tabs.is_empty() {
            tabs.push(TerminalTab::new(TerminalLaunchConfig::default()));
        }

        let active_tab = tabs
            .iter()
            .position(|tab| tab.id == persisted.active_tab_id)
            .unwrap_or_else(|| tabs.len().saturating_sub(1));

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
        Self::with_id(uuid::Uuid::new_v4().to_string(), launch_config)
    }

    fn with_id(id: String, launch_config: TerminalLaunchConfig) -> Self {
        let title = launch_config.default_title();
        Self {
            id,
            title,
            launch_config,
            restore_status: TerminalRestoreStatus::NotStarted,
        }
    }

    fn to_persisted(&self) -> PersistedTerminalTab {
        PersistedTerminalTab {
            id: self.id.clone(),
            title: self.title.clone(),
            provider: self.launch_config.provider,
            launch_config: Some(self.launch_config.clone()),
            restore_status: self.restore_status,
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
            launch_config,
            restore_status: persisted.restore_status,
        }
    }
}

struct GitRefreshReply {
    project_id: String,
    include_metadata: bool,
    state: ProjectGitState,
}

struct GitActionReply {
    project_id: String,
    refresh_git_state: bool,
    toast_kind: ToastKind,
    toast_message: String,
}

struct TerminalRuntimeRequest {
    key: TerminalRuntimeKey,
    cwd: std::path::PathBuf,
    launch_config: TerminalLaunchConfig,
    size: TerminalGridSize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChangedFilesGitMutation {
    StageFile { changed: ChangedFile },
    UnstageFile { changed: ChangedFile },
    StageAll,
    UnstageAll,
}

impl ChangedFilesGitMutation {
    fn stages_file(&self, path: &str) -> bool {
        matches!(self, Self::StageFile { changed } if changed.path == path)
    }

    fn unstages_file(&self, path: &str) -> bool {
        matches!(self, Self::UnstageFile { changed } if changed.path == path)
    }

    fn stages_all(&self) -> bool {
        matches!(self, Self::StageAll)
    }

    fn unstages_all(&self) -> bool {
        matches!(self, Self::UnstageAll)
    }
}

struct ChangedFilesGitMutationReply {
    project_id: String,
    result: Result<ProjectGitState, String>,
}

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

struct TaskCreationReply {
    result: Result<TaskCreationSuccess, TaskCreationFailure>,
}

struct ProjectAddReply {
    result: Result<crate::project_store::PreparedProject, String>,
}

struct ProjectGitHubLinkReply {
    project_id: String,
    github_url: Option<String>,
}

struct TaskCreationSuccess {
    original_project_id: String,
    project: crate::project_store::PreparedProject,
    branch_name: String,
    task_name: String,
    launch_config: TerminalLaunchConfig,
}

struct TaskCreationFailure {
    message: String,
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
    shown_at: Instant,
    dismiss_at: Instant,
}

#[derive(Debug, Clone)]
struct ToastDrag {
    toast_id: u64,
    start_x: f32,
    current_x: f32,
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
    /// Search text for the tasks list on the project page.
    pub(crate) project_page_task_search: String,
    /// Whether the Open PRs section is collapsed on the project page.
    pub(crate) project_page_prs_collapsed: bool,
    /// Active PR filter tab index (0=All Open, 1=Needs My Review, 2=My PRs, 3=Draft).
    pub(crate) project_page_pr_filter: usize,
    /// Per-section placeholder tab state.
    pub(crate) section_states: HashMap<SectionId, SectionState>,
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
            project_page_task_search: String::new(),
            project_page_prs_collapsed: false,
            project_page_pr_filter: 0,
            section_states,
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
        let changed =
            self.active_section.as_ref() != Some(&section_id) || self.active_project_page.is_some();
        self.active_section = Some(section_id);
        self.active_project_page = None;
        self.persist_active_section(cx);
        if changed {
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
            changed = true;
        }

        if self
            .active_project_page
            .as_ref()
            .is_some_and(|project_id| project_ids.contains(project_id))
        {
            self.active_project_page = None;
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
            self.active_section = Some(section_id);
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
            self.persist_section_state(section_id, cx);
            cx.notify();
        }
        activated
    }

    pub(crate) fn add_tab_with_launch_config(
        &mut self,
        section_id: &SectionId,
        launch_config: TerminalLaunchConfig,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let added_tab_id = self
            .section_states
            .get_mut(section_id)
            .map(|state| state.add_tab_with_launch_config(launch_config.clone()));
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

    pub(crate) fn mark_git_refresh_stale(&self, cx: &mut Context<Self>) {
        let _ = self.app.update(cx, |app, _| app.mark_git_refresh_stale());
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
        let section_key = self.active_section.as_ref().map(SectionId::store_key);
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
        let selected_agent_id = self
            .section_states
            .get(&section_id)
            .and_then(|state| state.tabs.get(state.active_tab))
            .and_then(|tab| tab.launch_config.provider)
            .and_then(|provider| {
                AGENTS
                    .iter()
                    .find(|agent| agent.provider == Some(provider))
                    .map(|agent| agent.id)
            })
            .map(str::to_string);
        let app = self.app.clone();
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, app_cx| {
                app.open_add_agent_modal(section_id.clone(), selected_agent_id.clone(), app_cx);
                app_cx.notify();
            });
        });
    }

    pub(crate) fn request_sidebar_task_delete(
        &self,
        request: SidebarTaskDeleteRequest,
        cx: &mut Context<Self>,
    ) {
        let app = self.app.clone();
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, app_cx| {
                app.request_sidebar_task_delete(request, app_cx);
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
    pub(crate) project_store: ProjectStore,
    pub(crate) project_github_links: HashMap<String, String>,
    pub(crate) expanded_projects: HashSet<String>,
    pub(crate) project_expand_animations: HashMap<String, SidebarProjectExpandAnimation>,
    pub(crate) next_project_expand_animation_id: u64,
    pub(crate) project_menu_project: Option<String>,
    pub(crate) sidebar_task_menu: Option<SidebarTaskMenuState>,
    /// Collapsed change-file sections in the right sidebar (e.g. "staged", "uncommitted").
    pub(crate) collapsed_change_sections: HashSet<String>,
    /// Whether the Create PR dropdown menu is open.
    pub(crate) create_pr_menu_open: bool,
    /// Whether the Push dropdown menu is open.
    pub(crate) push_menu_open: bool,
    /// Active transient notifications displayed above the app chrome.
    toasts: Vec<AppToast>,
    next_toast_id: u64,
    toast_drag: Option<ToastDrag>,
    copied_toast: Option<(u64, Instant)>,
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
    /// Whether the refresh timer has been started.
    pub(crate) refresh_timer_started: bool,
    /// The toolbar git action currently running in the background, if any.
    pub(crate) active_git_action: Option<crate::git_actions::ToolbarGitAction>,
    /// Receiver for the in-flight toolbar git action result.
    git_action_receiver: Option<mpsc::Receiver<GitActionReply>>,
    /// Pending right-sidebar git mutations keyed by project id.
    pending_changed_files_git_mutations: HashMap<String, PendingChangedFilesGitMutations>,
    /// Sender for background right-sidebar git mutation replies.
    changed_files_git_mutation_sender: mpsc::Sender<ChangedFilesGitMutationReply>,
    /// Receiver for background right-sidebar git mutation replies.
    changed_files_git_mutation_receiver: mpsc::Receiver<ChangedFilesGitMutationReply>,
    /// Whether an automatic git refresh is already running.
    pub(crate) git_refresh_in_flight: bool,
    /// Receiver for the in-flight automatic git refresh result.
    git_refresh_receiver: Option<mpsc::Receiver<GitRefreshReply>>,
    /// Receiver for the in-flight new task worktree creation result.
    task_creation_receiver: Option<mpsc::Receiver<TaskCreationReply>>,
    /// Receiver for the in-flight add-project background preparation result.
    project_add_receiver: Option<mpsc::Receiver<ProjectAddReply>>,
    /// Sender used by background project GitHub-link lookups.
    project_github_link_sender: mpsc::Sender<ProjectGitHubLinkReply>,
    /// Receiver for background project GitHub-link lookups.
    project_github_link_receiver: mpsc::Receiver<ProjectGitHubLinkReply>,
    /// Sender used by background terminal launch/resume work.
    terminal_launch_sender: mpsc::Sender<TerminalLaunchReply>,
    /// Receiver for background terminal launch/resume work.
    terminal_launch_receiver: mpsc::Receiver<TerminalLaunchReply>,
    /// Sender used by hidden add-agent terminal prewarming work.
    warm_terminal_launch_sender: mpsc::Sender<WarmTerminalLaunchReply>,
    /// Receiver for hidden add-agent terminal prewarming work.
    warm_terminal_launch_receiver: mpsc::Receiver<WarmTerminalLaunchReply>,
    /// Live PTY-backed terminal runtimes keyed by section and tab id.
    live_terminal_runtimes: HashMap<TerminalRuntimeKey, LiveTerminalRuntime>,
    /// Cached render snapshots for live terminal tabs.
    terminal_surface_snapshots: HashMap<TerminalRuntimeKey, TerminalSurfaceSnapshot>,
    /// Launches currently in flight.
    pending_terminal_launches: HashSet<TerminalRuntimeKey>,
    /// Recent terminal output used for restore-failure detection.
    terminal_recent_output: HashMap<TerminalRuntimeKey, String>,
    /// Last launch/exit error for a terminal tab.
    terminal_runtime_errors: HashMap<TerminalRuntimeKey, String>,
    /// Prewarmed launches keyed by launch id until they are canceled or exit.
    prewarmed_terminal_launches: HashMap<u64, PrewarmedTerminalLaunch>,
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
    /// New Task modal state. Some when open, None when closed.
    pub(crate) new_task_modal: Option<crate::new_task_modal::NewTaskModalState>,
    /// Add Agent modal state. Some when open, None when closed.
    pub(crate) add_agent_modal: Option<crate::add_agent_modal::AddAgentModalState>,
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
    /// UI font size (adjusted by Cmd+/Cmd- zoom).
    pub(crate) font_size: f32,
    /// Last time changed-file state was refreshed from git.
    pub(crate) last_git_status_refresh: Instant,
    /// Last time branch/worktree metadata was refreshed from git.
    pub(crate) last_git_metadata_refresh: Instant,
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
    SidebarTaskRename,
    Terminal,
    Blocked,
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
            TextInputTarget::SidebarTaskRename => self
                .sidebar_task_rename
                .as_ref()
                .map(|state| text_for_utf16_range(&state.task_name, range, adjusted_range)),
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
            TextInputTarget::SidebarTaskRename => self.sidebar_task_rename.as_ref().map(|state| {
                utf16_selection_for_text(
                    &state.task_name,
                    state.task_name_cursor,
                    state.task_name_selection_anchor,
                )
            }),
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
                    );
                    cx.notify();
                }
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
            TextInputTarget::NewTaskModal | TextInputTarget::SidebarTaskRename => {
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
        _range_utf16: std::ops::Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        None
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
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
) {
    let replacement = sanitize_single_line_input(new_text);
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

        if self.new_task_modal.is_some() {
            return TextInputTarget::Blocked;
        }

        if self.add_agent_modal.is_some() {
            return TextInputTarget::Blocked;
        }

        if self.sidebar_task_rename.is_some() {
            return TextInputTarget::SidebarTaskRename;
        }

        if self.settings_open {
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

    pub(crate) fn show_warning_toast(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.show_toast(ToastKind::Warning, message, cx);
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
        let now = Instant::now();
        let toast_id = self.next_toast_id;
        let lifetime = match kind {
            ToastKind::Error => TOAST_LIFETIME + TOAST_ERROR_EXTRA_LIFETIME,
            ToastKind::Success | ToastKind::Warning | ToastKind::Info => TOAST_LIFETIME,
        };
        self.next_toast_id += 1;
        self.toasts.push(AppToast {
            id: toast_id,
            kind,
            message: message.into(),
            shown_at: now,
            dismiss_at: now + lifetime,
        });

        if self.toasts.len() > TOAST_STACK_LIMIT {
            let excess = self.toasts.len() - TOAST_STACK_LIMIT;
            self.toasts.drain(0..excess);
        }

        cx.notify();
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
                    Timer::after(PROJECT_EXPAND_ANIMATION_STEP).await;
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
    pub fn new(cx: &mut Context<Self>) -> Self {
        let store = ProjectStore::load();
        let left_sidebar_open = store.ui.left_sidebar_open;
        let (project_github_link_sender, project_github_link_receiver) = mpsc::channel();
        let (changed_files_git_mutation_sender, changed_files_git_mutation_receiver) =
            mpsc::channel();
        let (terminal_launch_sender, terminal_launch_receiver) = mpsc::channel();
        let (warm_terminal_launch_sender, warm_terminal_launch_receiver) = mpsc::channel();
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
            project_store: store,
            project_github_links: HashMap::new(),
            expanded_projects: expanded,
            project_expand_animations: HashMap::new(),
            next_project_expand_animation_id: 1,
            project_menu_project: None,
            sidebar_task_menu: None,
            collapsed_change_sections: HashSet::new(),
            create_pr_menu_open: false,
            push_menu_open: false,
            toasts: Vec::new(),
            next_toast_id: 1,
            toast_drag: None,
            copied_toast: None,
            discard_confirm: None,
            project_remove_confirm: None,
            sidebar_task_delete_confirm: None,
            new_task_modal: None,
            sidebar_task_rename: None,
            workspace_pane,
            changed_files: HashMap::new(),
            changed_files_list_snapshots: HashMap::new(),
            focus_handle,
            refresh_timer_started: false,
            active_git_action: None,
            git_action_receiver: None,
            pending_changed_files_git_mutations: HashMap::new(),
            changed_files_git_mutation_sender,
            changed_files_git_mutation_receiver,
            git_refresh_in_flight: false,
            git_refresh_receiver: None,
            task_creation_receiver: None,
            project_add_receiver: None,
            project_github_link_sender,
            project_github_link_receiver,
            terminal_launch_sender,
            terminal_launch_receiver,
            warm_terminal_launch_sender,
            warm_terminal_launch_receiver,
            live_terminal_runtimes: HashMap::new(),
            terminal_surface_snapshots: HashMap::new(),
            pending_terminal_launches: HashSet::new(),
            terminal_recent_output: HashMap::new(),
            terminal_runtime_errors: HashMap::new(),
            prewarmed_terminal_launches: HashMap::new(),
            canceled_prewarmed_launch_ids: HashSet::new(),
            active_add_agent_warm_launch_id: None,
            active_new_task_warm_launch_id: None,
            next_prewarmed_launch_id: 1,
            project_github_link_requests: HashSet::new(),
            project_github_link_checked: HashSet::new(),
            settings_open: false,
            settings_section: crate::settings_page::SettingsSection::Agents,
            marked_text: None,
            add_agent_modal: None,
            sidebar_task_last_click: None,
            font_size: initial_font_size,
            last_git_status_refresh: Instant::now() - ACTIVE_GIT_STATUS_REFRESH_INTERVAL,
            last_git_metadata_refresh: Instant::now() - ACTIVE_GIT_METADATA_REFRESH_INTERVAL,
        };

        app
    }

    fn set_last_active_section_key(&mut self, section_key: Option<String>) {
        self.project_store.set_last_active_section_key(section_key);
    }

    fn persist_section_state(&mut self, section_id: &SectionId, persisted: PersistedSectionState) {
        if let Some(task_id) = section_id.task_id.as_deref() {
            self.project_store.update_task_tabs(task_id, &persisted);
        } else {
            self.project_store
                .set_terminal_section(section_id.store_key(), persisted);
        }
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
            size: TerminalGridSize::default(),
        })
    }

    fn append_terminal_recent_output(&mut self, key: &TerminalRuntimeKey, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let buffer = self.terminal_recent_output.entry(key.clone()).or_default();
        buffer.push_str(&text);
        trim_to_recent_output_limit(buffer);
    }

    fn clear_terminal_recent_output(&mut self, key: &TerminalRuntimeKey) {
        self.terminal_recent_output.remove(key);
    }

    fn maybe_retry_claude_restore(
        &mut self,
        key: &TerminalRuntimeKey,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.pending_terminal_launches.contains(key) {
            return false;
        }

        let Some(request) = self.terminal_request_for_key(key, cx) else {
            return false;
        };
        let is_claude_restore = request.launch_config.provider == Some(AgentProviderKind::ClaudeCode)
            && request.launch_config.session.is_some();
        if !is_claude_restore {
            return false;
        }

        let recent_output = self
            .terminal_recent_output
            .get(key)
            .map(String::as_str)
            .unwrap_or_default();
        if !output_mentions_missing_claude_conversation(recent_output) {
            return false;
        }

        let launch_config = request.launch_config.with_session(None);
        self.pending_terminal_launches.insert(key.clone());
        self.terminal_runtime_errors.remove(key);
        self.clear_terminal_recent_output(key);
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
            size: self.terminal_panel_size(window),
        })
    }

    fn terminal_panel_size(&self, window: &Window) -> TerminalGridSize {
        let viewport = window.viewport_size();
        let titlebar_height = if cfg!(target_os = "macos") {
            TITLEBAR_CHROME_H
        } else {
            0.0
        };
        let width = (f32::from(viewport.width) - self.sidebar_w - self.right_w - GUTTER * 2.0)
            .max(MIN_MAIN);
        let height = (f32::from(viewport.height) - FOOTER_H - titlebar_height - 36.0).max(120.0);
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
            launch_config,
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
        &self,
        _cx: &App,
    ) -> Option<(std::path::PathBuf, TerminalLaunchConfig)> {
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

        if let Some(key) = launch.attached_tab {
            self.pending_terminal_launches.remove(&key);
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
        let Some(launch) = self.prewarmed_terminal_launches.get_mut(&launch_id) else {
            return false;
        };

        launch.attached_tab = Some(key.clone());

        if let Some(mut runtime) = launch.runtime.take() {
            self.pending_terminal_launches.remove(&key);
            self.terminal_runtime_errors.remove(&key);
            self.terminal_surface_snapshots
                .insert(key.clone(), runtime.snapshot());
            self.live_terminal_runtimes.insert(key.clone(), runtime);
            let launch_config = launch.launch_config.clone();
            self.update_terminal_tab(&key, cx, |tab| {
                tab.launch_config = launch_config.clone();
                tab.restore_status = TerminalRestoreStatus::Ready;
            });
        } else {
            self.pending_terminal_launches.insert(key.clone());
            let launch_config = launch.launch_config.clone();
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

        if let Some(runtime) = self.live_terminal_runtimes.get_mut(&request.key) {
            match runtime.resize(request.size) {
                Ok(true) => {
                    self.terminal_surface_snapshots
                        .insert(request.key.clone(), runtime.snapshot());
                    cx.notify();
                }
                Ok(false) => {}
                Err(error) => {
                    self.terminal_runtime_errors
                        .insert(request.key.clone(), error.to_string());
                    self.show_error_toast(error.to_string(), cx);
                }
            }
            return;
        }

        if self.pending_terminal_launches.contains(&request.key) {
            return;
        }

        self.pending_terminal_launches.insert(request.key.clone());
        self.update_terminal_tab(&request.key, cx, |tab| {
            tab.restore_status = TerminalRestoreStatus::Launching;
        });
        spawn_terminal_launch(
            self.terminal_launch_sender.clone(),
            request.key,
            Some(request.cwd),
            request.launch_config,
            request.size,
        );
    }

    fn drain_terminal_launch_replies(&mut self, cx: &mut Context<Self>) -> bool {
        let mut updated = false;

        loop {
            match self.terminal_launch_receiver.try_recv() {
                Ok(TerminalLaunchReply::Launched {
                    key,
                    runtime,
                    launch_config,
                }) => {
                    self.pending_terminal_launches.remove(&key);
                    self.clear_terminal_recent_output(&key);
                    self.terminal_runtime_errors.remove(&key);

                    let mut runtime = LiveTerminalRuntime::from_prepared(runtime);
                    self.terminal_surface_snapshots
                        .insert(key.clone(), runtime.snapshot());
                    self.live_terminal_runtimes.insert(key.clone(), runtime);
                    self.update_terminal_tab(&key, cx, |tab| {
                        tab.launch_config = launch_config.clone();
                        tab.restore_status = TerminalRestoreStatus::Ready;
                    });
                    updated = true;
                }
                Ok(TerminalLaunchReply::Output { key, bytes }) => {
                    self.append_terminal_recent_output(&key, &bytes);
                    if let Some(runtime) = self.live_terminal_runtimes.get_mut(&key) {
                        let terminal_update = runtime.apply_output(&bytes);
                        self.terminal_surface_snapshots
                            .insert(key.clone(), runtime.snapshot());
                        if terminal_update.reset_title {
                            self.update_terminal_tab(&key, cx, |tab| {
                                tab.title = tab.launch_config.default_title();
                            });
                        } else if let Some(title) = terminal_update.title {
                            self.update_terminal_tab(&key, cx, |tab| {
                                tab.title = title.clone();
                            });
                        }
                        updated = true;
                    } else if self.maybe_retry_claude_restore(&key, cx) {
                        updated = true;
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
                        self.live_terminal_runtimes.remove(&key);
                        self.terminal_surface_snapshots.remove(&key);
                        updated = true;
                        continue;
                    }
                    self.pending_terminal_launches.remove(&key);
                    self.terminal_surface_snapshots.remove(&key);
                    self.terminal_runtime_errors.insert(key.clone(), status);
                    self.clear_terminal_recent_output(&key);
                    self.live_terminal_runtimes.remove(&key);
                    self.update_terminal_tab(&key, cx, |tab| {
                        tab.restore_status = TerminalRestoreStatus::Failed;
                    });
                    updated = true;
                }
                Ok(TerminalLaunchReply::Failed { key, message }) => {
                    self.pending_terminal_launches.remove(&key);
                    self.live_terminal_runtimes.remove(&key);
                    self.terminal_surface_snapshots.remove(&key);
                    self.terminal_runtime_errors
                        .insert(key.clone(), message.clone());
                    self.clear_terminal_recent_output(&key);
                    self.update_terminal_tab(&key, cx, |tab| {
                        tab.restore_status = TerminalRestoreStatus::Failed;
                    });
                    self.show_error_toast(message, cx);
                    updated = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        updated
    }

    fn drain_warm_terminal_launch_replies(&mut self, cx: &mut Context<Self>) -> bool {
        let mut updated = false;

        loop {
            match self.warm_terminal_launch_receiver.try_recv() {
                Ok(WarmTerminalLaunchReply::Launched {
                    launch_id,
                    runtime,
                    launch_config,
                }) => {
                    if self.canceled_prewarmed_launch_ids.contains(&launch_id) {
                        let mut runtime = LiveTerminalRuntime::from_prepared(runtime);
                        runtime.kill();
                        continue;
                    }

                    let mut runtime = LiveTerminalRuntime::from_prepared(runtime);
                    let Some(launch) = self.prewarmed_terminal_launches.get_mut(&launch_id) else {
                        runtime.kill();
                        continue;
                    };

                    launch.launch_config = launch_config.clone();

                    if let Some(key) = launch.attached_tab.clone() {
                        self.pending_terminal_launches.remove(&key);
                        self.clear_terminal_recent_output(&key);
                        self.terminal_runtime_errors.remove(&key);
                        self.terminal_surface_snapshots
                            .insert(key.clone(), runtime.snapshot());
                        self.live_terminal_runtimes.insert(key.clone(), runtime);
                        self.update_terminal_tab(&key, cx, |tab| {
                            tab.launch_config = launch_config.clone();
                            tab.restore_status = TerminalRestoreStatus::Ready;
                        });
                        updated = true;
                    } else {
                        launch.runtime = Some(runtime);
                    }
                }
                Ok(WarmTerminalLaunchReply::Output { launch_id, bytes }) => {
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
                            if terminal_update.reset_title {
                                self.update_terminal_tab(&key, cx, |tab| {
                                    tab.title = tab.launch_config.default_title();
                                });
                            } else if let Some(title) = terminal_update.title {
                                self.update_terminal_tab(&key, cx, |tab| {
                                    tab.title = title.clone();
                                });
                            }
                            updated = true;
                        } else if self.maybe_retry_claude_restore(&key, cx) {
                            updated = true;
                        }
                        continue;
                    }

                    if let Some(launch) = self.prewarmed_terminal_launches.get_mut(&launch_id) {
                        if let Some(runtime) = launch.runtime.as_mut() {
                            let _ = runtime.apply_output(&bytes);
                        }
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
                    self.canceled_prewarmed_launch_ids.remove(&launch_id);

                    if let Some(key) = attached_key {
                        if self.maybe_retry_claude_restore(&key, cx) {
                            self.live_terminal_runtimes.remove(&key);
                            self.terminal_surface_snapshots.remove(&key);
                            updated = true;
                            continue;
                        }
                        self.pending_terminal_launches.remove(&key);
                        self.terminal_surface_snapshots.remove(&key);
                        self.terminal_runtime_errors.insert(key.clone(), status);
                        self.clear_terminal_recent_output(&key);
                        self.live_terminal_runtimes.remove(&key);
                        self.update_terminal_tab(&key, cx, |tab| {
                            tab.restore_status = TerminalRestoreStatus::Failed;
                        });
                        updated = true;
                    }
                }
                Ok(WarmTerminalLaunchReply::Failed { launch_id, message }) => {
                    let attached_key = self
                        .prewarmed_terminal_launches
                        .get(&launch_id)
                        .and_then(|launch| launch.attached_tab.clone());

                    self.prewarmed_terminal_launches.remove(&launch_id);
                    self.canceled_prewarmed_launch_ids.remove(&launch_id);
                    if self.active_add_agent_warm_launch_id == Some(launch_id) {
                        self.active_add_agent_warm_launch_id = None;
                    }

                    if let Some(key) = attached_key {
                        self.pending_terminal_launches.remove(&key);
                        self.live_terminal_runtimes.remove(&key);
                        self.terminal_surface_snapshots.remove(&key);
                        self.terminal_runtime_errors
                            .insert(key.clone(), message.clone());
                        self.clear_terminal_recent_output(&key);
                        self.update_terminal_tab(&key, cx, |tab| {
                            tab.restore_status = TerminalRestoreStatus::Failed;
                        });
                        self.show_error_toast(message, cx);
                        updated = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        updated
    }

    pub(crate) fn terminal_snapshot_for(
        &self,
        key: &TerminalRuntimeKey,
    ) -> Option<TerminalSurfaceSnapshot> {
        self.terminal_surface_snapshots.get(key).cloned()
    }

    pub(crate) fn terminal_error_for(&self, key: &TerminalRuntimeKey) -> Option<&str> {
        self.terminal_runtime_errors.get(key).map(String::as_str)
    }

    pub(crate) fn terminal_is_pending(&self, key: &TerminalRuntimeKey) -> bool {
        self.pending_terminal_launches.contains(key)
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
        runtime.write_input(bytes).is_ok()
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
        let key = TerminalRuntimeKey {
            section_id: section_id.clone(),
            tab_id,
        };
        self.cancel_prewarmed_launch_for_tab(&key);
        if let Some(mut runtime) = remove_terminal_runtime_state(
            &mut self.live_terminal_runtimes,
            &mut self.terminal_surface_snapshots,
            &mut self.pending_terminal_launches,
            &mut self.terminal_recent_output,
            &mut self.terminal_runtime_errors,
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
        self.last_git_status_refresh = Instant::now() - ACTIVE_GIT_STATUS_REFRESH_INTERVAL;
        self.last_git_metadata_refresh = Instant::now() - ACTIVE_GIT_METADATA_REFRESH_INTERVAL;
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

        let (tx, rx) = mpsc::channel();
        self.project_add_receiver = Some(rx);
        std::thread::spawn(move || {
            let result = crate::project_store::prepare_project(&path);
            let _ = tx.send(ProjectAddReply { result });
        });
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

    fn git_status_refresh_interval(&self) -> Duration {
        ACTIVE_GIT_STATUS_REFRESH_INTERVAL
    }

    fn apply_project_git_state(&mut self, project_id: &str, state: ProjectGitState) -> bool {
        let mut changed = false;

        let ProjectGitState {
            changed_files,
            ahead_count,
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

    fn drain_git_refresh(&mut self) -> bool {
        let Some(receiver) = self.git_refresh_receiver.as_ref() else {
            return false;
        };

        match receiver.try_recv() {
            Ok(reply) => {
                self.git_refresh_in_flight = false;
                self.git_refresh_receiver = None;
                if self
                    .pending_changed_files_git_mutations
                    .contains_key(&reply.project_id)
                {
                    return false;
                }
                self.last_git_status_refresh = Instant::now();
                if reply.include_metadata {
                    self.last_git_metadata_refresh = self.last_git_status_refresh;
                }
                self.apply_project_git_state(&reply.project_id, reply.state)
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.git_refresh_in_flight = false;
                self.git_refresh_receiver = None;
                false
            }
        }
    }

    fn maybe_schedule_active_git_refresh(&mut self, cx: &App) {
        if self.git_refresh_in_flight {
            return;
        }

        let status_due =
            self.last_git_status_refresh.elapsed() >= self.git_status_refresh_interval();
        let metadata_due =
            self.last_git_metadata_refresh.elapsed() >= ACTIVE_GIT_METADATA_REFRESH_INTERVAL;
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

        let include_metadata = metadata_due;
        self.git_refresh_in_flight = true;
        let (tx, rx) = mpsc::channel();
        self.git_refresh_receiver = Some(rx);
        std::thread::spawn(move || {
            let state =
                crate::project_store::read_project_git_state(&project_path, include_metadata);
            let _ = tx.send(GitRefreshReply {
                project_id,
                include_metadata,
                state,
            });
        });
    }

    #[hotpath::measure]
    pub(crate) fn refresh_project_git_state(&mut self, project_id: &str) {
        let Some(project_path) = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
        else {
            return;
        };

        let state = crate::project_store::read_project_git_state(&project_path, true);
        self.apply_project_git_state(project_id, state);
        self.last_git_status_refresh = Instant::now();
        self.last_git_metadata_refresh = self.last_git_status_refresh;
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
    }

    pub(crate) fn submit_new_task_modal(&mut self, cx: &mut Context<Self>) {
        let (
            project_id,
            task_name,
            generated_task_name,
            source_branch,
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
            state.task_name_focused = false;

            (
                state.project_id.clone(),
                state.task_name.trim().to_string(),
                state.generated_task_name.clone(),
                state.source_branch.clone(),
                state.worktree_mode,
                terminal_launch_config_for_selected_agents(&state.selected_agents),
                self.active_new_task_warm_launch_id,
            )
        };

        let Some(project) = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
        else {
            self.show_error_toast("Could not find the selected project.", cx);
            self.cancel_active_new_task_prewarm();
            self.new_task_modal = None;
            return;
        };

        if !worktree_mode {
            let branch_name = crate::project_store::current_branch(&project.path)
                .or_else(|| self.project_store.current_branch_name(&project.id))
                .unwrap_or_else(|| source_branch.clone());

            if branch_name.is_empty() {
                self.show_error_toast(
                    "Could not determine the current branch for the selected project.",
                    cx,
                );
                return;
            }

            let task_name = if task_name.is_empty() {
                generated_task_name.clone()
            } else {
                task_name.clone()
            };
            let task_id = uuid::Uuid::new_v4().to_string();
            self.project_store.insert_task(Task {
                id: task_id.clone(),
                name: task_name.clone(),
                kind: TaskKind::Direct,
                root_project_id: project.id.clone(),
                target_project_id: project.id.clone(),
                branch_name: branch_name.clone(),
                section_id: SectionId::for_task(&project.id, &branch_name, &task_id).store_key(),
                worktree_project_id: None,
                tabs: Vec::new(),
                active_tab_id: String::new(),
                next_tab_id: 0,
                cwd: None,
            });
            self.project_store.save();
            self.expanded_projects.insert(project.repo_id.clone());
            self.project_store
                .set_expanded_projects(&self.expanded_projects);
            let section_id = SectionId::for_task(&project.id, &branch_name, &task_id);
            let project_path = project.path.clone();
            let launch_config = launch_config.clone();
            self.workspace_pane.update(cx, |workspace, cx| {
                workspace.activate_section(
                    section_id,
                    Some(project_path),
                    Some(launch_config.clone()),
                    cx,
                );
            });
            let key = self.active_terminal_key(cx);
            let warm_launch_id = self
                .active_new_task_warm_launch_id
                .take()
                .or(warm_launch_id);
            if let Some(key) = key {
                self.attach_or_start_prewarmed_terminal(
                    warm_launch_id,
                    key,
                    project.path.clone(),
                    launch_config,
                    cx,
                );
            }
            self.mark_git_refresh_stale();
            self.new_task_modal = None;
            self.show_success_toast(
                format!("Opened direct task {} on {}.", task_name, branch_name),
                cx,
            );
            return;
        }

        if let Some(state) = self.new_task_modal.as_mut() {
            state.submitting = true;
        }
        self.cancel_active_new_task_prewarm();
        self.show_info_toast("Creating worktree task...", cx);

        let project_path = project.path.clone();
        let project_name = project.name.clone();
        let (tx, rx) = mpsc::channel();
        self.task_creation_receiver = Some(rx);
        std::thread::spawn(move || {
            let result = crate::project_store::create_task_worktree(
                &project_path,
                &project_name,
                &task_name,
                &generated_task_name,
                &source_branch,
            )
            .map(|created| TaskCreationSuccess {
                original_project_id: project_id,
                project: crate::project_store::prepare_project(&created.path).unwrap_or_else(
                    |_| crate::project_store::PreparedProject {
                        project: crate::project_store::Project {
                            id: uuid::Uuid::new_v4().to_string(),
                            repo_id: uuid::Uuid::new_v4().to_string(),
                            name: created
                                .path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| created.path.display().to_string()),
                            path: created.path.clone(),
                            kind: crate::project_store::ProjectKind::Worktree,
                            checkout: crate::project_store::ProjectCheckoutState::default(),
                            worktree_name: created
                                .path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned()),
                            repo_common_dir: None,
                        },
                        repo: crate::project_store::RepoRecord {
                            id: uuid::Uuid::new_v4().to_string(),
                            common_dir: None,
                            branch_order: Vec::new(),
                            branches_by_name: HashMap::new(),
                        },
                    },
                ),
                branch_name: created.branch_name,
                task_name: created.task_name,
                launch_config,
            })
            .map_err(|message| TaskCreationFailure { message });

            let _ = tx.send(TaskCreationReply { result });
        });
        cx.notify();
    }

    pub(crate) fn active_changed_files(&self, cx: &App) -> Arc<[ChangedFile]> {
        self.workspace_pane
            .read(cx)
            .active_section
            .as_ref()
            .and_then(|section| self.changed_files.get(&section.project_id))
            .cloned()
            .unwrap_or_else(|| Arc::from(Vec::<ChangedFile>::new()))
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
        project_path: std::path::PathBuf,
        mutation: ChangedFilesGitMutation,
    ) {
        let reply_project_id = project_id.to_string();
        let tx = self.changed_files_git_mutation_sender.clone();
        std::thread::spawn(move || {
            let result = match mutation {
                ChangedFilesGitMutation::StageFile { changed } => {
                    crate::project_store::stage_changed_file(&project_path, &changed)
                        .map(|_| crate::project_store::read_project_git_state(&project_path, false))
                }
                ChangedFilesGitMutation::UnstageFile { changed } => {
                    crate::project_store::unstage_changed_file(&project_path, &changed)
                        .map(|_| crate::project_store::read_project_git_state(&project_path, false))
                }
                ChangedFilesGitMutation::StageAll => {
                    crate::project_store::stage_all_changes(&project_path)
                        .map(|_| crate::project_store::read_project_git_state(&project_path, false))
                }
                ChangedFilesGitMutation::UnstageAll => {
                    crate::project_store::unstage_all_changes(&project_path)
                        .map(|_| crate::project_store::read_project_git_state(&project_path, false))
                }
            };

            let _ = tx.send(ChangedFilesGitMutationReply {
                project_id: reply_project_id,
                result,
            });
        });
    }

    fn project_path(&self, project_id: &str) -> Option<std::path::PathBuf> {
        self.project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
    }

    pub(crate) fn changed_files_actions_busy(&self, _project_id: &str) -> bool {
        self.active_git_action.is_some()
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
        if self.active_git_action.is_some() {
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
        if self.active_git_action.is_some() {
            self.show_info_toast("A git action is already running.", cx);
            return;
        }

        let Some((project_id, project_path)) = self.active_project_context(cx) else {
            self.show_error_toast("No active project is selected.", cx);
            return;
        };
        let start_message = match action {
            crate::git_actions::ToolbarGitAction::Commit => {
                "Generating an AI commit message for staged changes..."
            }
            crate::git_actions::ToolbarGitAction::CommitAndPush => {
                "Generating an AI commit message before commit and push..."
            }
            crate::git_actions::ToolbarGitAction::Push { force: false } => {
                "Pushing the current branch..."
            }
            crate::git_actions::ToolbarGitAction::Push { force: true } => {
                "Force-pushing the current branch with lease..."
            }
            crate::git_actions::ToolbarGitAction::CreatePr { draft: false } => {
                "Creating a pull request..."
            }
            crate::git_actions::ToolbarGitAction::CreatePr { draft: true } => {
                "Creating a draft pull request..."
            }
        };
        self.show_info_toast(start_message, cx);

        let (tx, rx) = mpsc::channel();
        self.push_menu_open = false;
        self.create_pr_menu_open = false;
        self.active_git_action = Some(action);
        self.git_action_receiver = Some(rx);
        std::thread::spawn(move || {
            let reply = match crate::git_actions::execute_toolbar_git_action(&project_path, action)
            {
                Ok(outcome) => GitActionReply {
                    project_id: project_id.clone(),
                    refresh_git_state: outcome.refresh_git_state,
                    toast_kind: if outcome.warning {
                        ToastKind::Warning
                    } else {
                        ToastKind::Success
                    },
                    toast_message: outcome.toast_message,
                },
                Err(error) => GitActionReply {
                    project_id: project_id.clone(),
                    refresh_git_state: error.refresh_git_state,
                    toast_kind: ToastKind::Error,
                    toast_message: error.message,
                },
            };
            let _ = tx.send(reply);
        });
        cx.notify();
    }

    fn drain_git_action(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(receiver) = self.git_action_receiver.as_ref() else {
            return false;
        };

        match receiver.try_recv() {
            Ok(reply) => {
                self.active_git_action = None;
                self.git_action_receiver = None;
                if reply.refresh_git_state {
                    self.refresh_project_git_state(&reply.project_id);
                }
                match reply.toast_kind {
                    ToastKind::Success => self.show_success_toast(reply.toast_message, cx),
                    ToastKind::Error => self.show_error_toast(reply.toast_message, cx),
                    ToastKind::Warning => self.show_warning_toast(reply.toast_message, cx),
                    ToastKind::Info => self.show_info_toast(reply.toast_message, cx),
                }
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.active_git_action = None;
                self.git_action_receiver = None;
                self.show_error_toast("The background git action did not complete.", cx);
                true
            }
        }
    }

    fn drain_changed_files_git_mutations(&mut self, cx: &mut Context<Self>) -> bool {
        let mut should_notify = false;

        while let Ok(reply) = self.changed_files_git_mutation_receiver.try_recv() {
            let pending = self
                .pending_changed_files_git_mutations
                .remove(&reply.project_id);
            should_notify = true;

            match reply.result {
                Ok(state) => {
                    let Some(mut pending) = pending else {
                        should_notify |= self.apply_project_git_state(&reply.project_id, state);
                        self.last_git_status_refresh = Instant::now();
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
                        self.last_git_status_refresh = Instant::now();
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

    fn drain_task_creation(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(receiver) = self.task_creation_receiver.as_ref() else {
            return false;
        };

        match receiver.try_recv() {
            Ok(reply) => {
                self.task_creation_receiver = None;
                match reply.result {
                    Ok(success) => {
                        let prepared = success.project.clone();
                        let inserted = self.project_store.insert_prepared_project(prepared.clone());
                        if !inserted {
                            if let Some(state) = self.new_task_modal.as_mut() {
                                state.submitting = false;
                            }
                            self.show_error_toast(
                                "The worktree was created, but the app could not load it.",
                                cx,
                            );
                            return true;
                        }

                        let Some(project) =
                            self.project_store.project(&prepared.project.id).cloned()
                        else {
                            if let Some(state) = self.new_task_modal.as_mut() {
                                state.submitting = false;
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
                        self.workspace_pane.update(cx, |workspace, cx| {
                            workspace.activate_section(
                                section_id,
                                Some(project_path),
                                Some(launch_config.clone()),
                                cx,
                            );
                        });
                        if let Some(key) = self.active_terminal_key(cx) {
                            self.attach_or_start_prewarmed_terminal(
                                None,
                                key,
                                project.path.clone(),
                                launch_config,
                                cx,
                            );
                        }
                        self.mark_git_refresh_stale();

                        self.new_task_modal = None;
                        self.project_store.save();
                        self.show_success_toast(
                            format!(
                                "Created worktree task {} on {}.",
                                success.task_name, success.branch_name
                            ),
                            cx,
                        );
                    }
                    Err(error) => {
                        if let Some(state) = self.new_task_modal.as_mut() {
                            state.submitting = false;
                        }
                        self.show_error_toast(error.message, cx);
                    }
                }
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.task_creation_receiver = None;
                if let Some(state) = self.new_task_modal.as_mut() {
                    state.submitting = false;
                }
                self.show_error_toast("The task creation process did not complete.", cx);
                true
            }
        }
    }

    fn drain_project_add(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(receiver) = self.project_add_receiver.as_ref() else {
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
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.project_add_receiver = None;
                self.show_error_toast("The add project process did not complete.", cx);
                true
            }
        }
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

        let tx = self.project_github_link_sender.clone();
        let project_id = project_id.to_string();
        let project_path = project_path.to_path_buf();
        std::thread::spawn(move || {
            let github_url = crate::git_actions::find_github_repo_url(&project_path);
            let _ = tx.send(ProjectGitHubLinkReply {
                project_id,
                github_url,
            });
        });
    }

    fn drain_project_github_link_lookup(&mut self) -> bool {
        let mut should_notify = false;

        while let Ok(reply) = self.project_github_link_receiver.try_recv() {
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

    pub(crate) fn revert_changed_file(&mut self, project_id: &str, changed: &ChangedFile) -> bool {
        let Some(project_path) = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
        else {
            return false;
        };

        let reverted = crate::project_store::revert_changed_file(&project_path, changed);
        if reverted {
            self.refresh_project_git_state(project_id);
        }
        reverted
    }

    pub(crate) fn revert_changed_files(
        &mut self,
        project_id: &str,
        changed_files: &[ChangedFile],
    ) -> bool {
        let Some(project_path) = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
        else {
            return false;
        };

        let mut reverted_any = false;
        for changed in changed_files {
            reverted_any |= crate::project_store::revert_changed_file(&project_path, changed);
        }

        if reverted_any {
            self.refresh_project_git_state(project_id);
        }

        reverted_any
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
                        this.sync_workspace_layout(cx);
                        cx.notify();
                    });
                    Timer::after(Duration::from_millis(STEP_MS)).await;
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
                        this.sync_workspace_layout(cx);
                        cx.notify();
                    });
                    Timer::after(Duration::from_millis(STEP_MS)).await;
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
        let had_layout_drag = self.drag.take().is_some();

        if had_layout_drag {
            self.clamp_layout(window);
            self.sync_workspace_layout(cx);
            self.project_store
                .set_left_sidebar_open(self.sidebar_is_open());
        }

        if had_toast_drag || had_layout_drag {
            cx.notify();
        }
    }

    fn footer_add_project_button(window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let icon_col = theme::toggle_icon_color(window);
        let hover_bg = gpui::white().opacity(0.06);

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

    fn footer_settings_button(window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let icon_col = theme::toggle_icon_color(window);
        let hover_bg = gpui::white().opacity(0.06);

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

    fn footer_branch_indicator(&self, window: &Window, cx: &App) -> impl IntoElement {
        let icon_col = theme::toggle_icon_color(window);
        let text_col = gpui::white().opacity(0.55);

        if let Some(section) = self.workspace_pane.read(cx).active_section.clone() {
            let name: SharedString = section.branch_name.clone().into();
            div()
                .flex()
                .flex_row()
                .items_center()
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
        let icon_col = theme::toggle_icon_color(window);
        let text_col = gpui::white().opacity(0.55);

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

    fn refresh_timer_interval(&self) -> Duration {
        if !self.pending_terminal_launches.is_empty()
            || !self.live_terminal_runtimes.is_empty()
            || !self.prewarmed_terminal_launches.is_empty()
        {
            TOAST_ANIMATION_REFRESH_INTERVAL
        } else if self.toasts.is_empty() && self.copied_toast.is_none() {
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
                hsla(138. / 360., 0.52, 0.66, 1.),
                hsla(136. / 360., 0.40, 0.24, 0.90),
                hsla(136. / 360., 0.42, 0.34, 0.55),
                "Success",
            ),
            ToastKind::Error => (
                "assets/icons/icons__alert-triangle.svg",
                hsla(0., 0.68, 0.72, 1.),
                hsla(0., 0.40, 0.24, 0.90),
                hsla(0., 0.45, 0.36, 0.58),
                "Error",
            ),
            ToastKind::Warning => (
                "assets/icons/icons__badge-alert.svg",
                hsla(42. / 360., 0.70, 0.68, 1.),
                hsla(40. / 360., 0.42, 0.24, 0.90),
                hsla(42. / 360., 0.46, 0.34, 0.58),
                "Warning",
            ),
            ToastKind::Info => (
                "assets/icons/icons__file_icons__info.svg",
                hsla(208. / 360., 0.62, 0.72, 1.),
                hsla(210. / 360., 0.40, 0.24, 0.90),
                hsla(208. / 360., 0.42, 0.34, 0.58),
                "Info",
            ),
        }
    }

    fn toast_animation_state(toast: &AppToast, now: Instant) -> (f32, f32) {
        let fade_in_progress = if TOAST_FADE_IN.is_zero() {
            1.
        } else {
            (now.saturating_duration_since(toast.shown_at).as_secs_f32()
                / TOAST_FADE_IN.as_secs_f32())
            .clamp(0., 1.)
        };
        let fade_in = fade_in_progress * fade_in_progress * (3. - 2. * fade_in_progress);

        let fade_out_progress = if now >= toast.dismiss_at {
            0.
        } else if TOAST_FADE_OUT.is_zero() {
            1.
        } else {
            (toast
                .dismiss_at
                .saturating_duration_since(now)
                .as_secs_f32()
                / TOAST_FADE_OUT.as_secs_f32())
            .clamp(0., 1.)
        };
        let fade_out = fade_out_progress * fade_out_progress * (3. - 2. * fade_out_progress);

        let opacity = fade_in.min(fade_out);
        let slide_offset = (1. - fade_in) * 14.;
        (opacity, slide_offset)
    }

    fn toast_card(
        &self,
        index: usize,
        toast: &AppToast,
        opacity: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let toast_id = toast.id;
        let (icon_path, icon_color, icon_bg, border_color, tone_label) =
            Self::toast_visuals(toast.kind);
        let text_col = hsla(0., 0., 0.94, 1.);
        let card_bg = rgb(0x202329);
        let copy_hover = gpui::white().opacity(0.06);
        let copied = self.toast_copy_feedback_visible(toast_id);
        let copy_icon = if copied {
            hsla(138. / 360., 0.58, 0.72, 1.)
        } else {
            hsla(0., 0., 0.72, 1.)
        };
        let message = toast.message.clone();
        let copy_message = message.clone();

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
                                    if copied {
                                        "Copied"
                                    } else {
                                        "Copy notification message"
                                    },
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
        if self.toasts.is_empty() {
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

fn choose_initial_section(
    projects: &[crate::project_store::Project],
    section_states: &HashMap<SectionId, SectionState>,
    last_active_section_key: Option<&str>,
) -> Option<SectionId> {
    if let Some(section_id) = last_active_section_key
        .and_then(SectionId::from_store_key)
        .filter(|section_id| section_states.contains_key(section_id))
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
        apply_terminal_session_backfill, choose_initial_section,
        output_mentions_missing_claude_conversation, remove_terminal_runtime_state, SectionId,
        SectionState, trim_to_recent_output_limit, TERMINAL_RECENT_OUTPUT_LIMIT,
    };
    use crate::agents::{
        AgentProviderKind, TerminalLaunchConfig, TerminalRestoreStatus, TerminalSessionKind,
        TerminalSessionRef,
    };
    use crate::project_store::{
        PersistedSectionState, PersistedTerminalTab, Project, ProjectCheckoutState, ProjectKind,
    };
    use crate::terminal_runtime::{TerminalRuntimeKey, TerminalSurfaceSnapshot};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn shell_tab(id: usize, title: &str) -> PersistedTerminalTab {
        PersistedTerminalTab {
            id: id.to_string(),
            title: title.to_string(),
            provider: None,
            launch_config: Some(TerminalLaunchConfig::default()),
            restore_status: TerminalRestoreStatus::NotStarted,
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
            worktree_name: None,
            repo_common_dir: None,
        }
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
                        provider: Some(AgentProviderKind::Codex),
                        launch_config: Some(TerminalLaunchConfig::for_provider(
                            AgentProviderKind::Codex,
                        )),
                        restore_status: TerminalRestoreStatus::NotStarted,
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
    fn section_state_add_tab_with_launch_config_continues_after_restored_next_tab_id() {
        let mut state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: "0".to_string(),
                next_tab_id: 7,
                cwd: Some(PathBuf::from("/tmp/project")),
                tabs: vec![PersistedTerminalTab {
                    id: "0".to_string(),
                    title: "Pi".to_string(),
                    provider: Some(AgentProviderKind::Pi),
                    launch_config: Some(TerminalLaunchConfig::for_provider(AgentProviderKind::Pi)),
                    restore_status: TerminalRestoreStatus::NotStarted,
                }],
            },
            None,
        );

        let id = state
            .add_tab_with_launch_config(TerminalLaunchConfig::for_provider(AgentProviderKind::Pi));

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

        let id = state.add_tab_with_launch_config(TerminalLaunchConfig::for_provider(
            AgentProviderKind::ClaudeCode,
        ));

        assert!(!id.is_empty());
        assert_eq!(state.active_tab, 1);
        assert_eq!(state.tabs[1].title, "Claude Code");
        assert_eq!(
            state.tabs[1].launch_config,
            TerminalLaunchConfig::for_provider(AgentProviderKind::ClaudeCode)
        );
    }

    #[test]
    fn choose_initial_section_prefers_last_active_section_key() {
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
                task_section,
                SectionState::from_persisted(
                    PersistedSectionState {
                        active_tab_id: "0".to_string(),
                        next_tab_id: 1,
                        cwd: Some(project.path.clone()),
                        tabs: vec![PersistedTerminalTab {
                            id: "0".to_string(),
                            title: "Codex".to_string(),
                            provider: Some(AgentProviderKind::Codex),
                            launch_config: Some(TerminalLaunchConfig::for_provider(
                                AgentProviderKind::Codex,
                            )),
                            restore_status: TerminalRestoreStatus::NotStarted,
                        }],
                    },
                    None,
                ),
            ),
        ]);

        let chosen =
            choose_initial_section(&[project], &section_states, Some(&main_section.store_key()));

        assert_eq!(chosen, Some(main_section));
    }

    #[test]
    fn choose_initial_section_prefers_restored_agent_section_without_saved_selection() {
        let project = sample_project("project-1", "main");
        let main_section = SectionId::new(&project.id, "main");
        let task_section = SectionId::for_task(&project.id, "main", "task-1");
        let section_states = HashMap::from([
            (
                main_section,
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
                        active_tab_id: "0".to_string(),
                        next_tab_id: 1,
                        cwd: Some(project.path.clone()),
                        tabs: vec![PersistedTerminalTab {
                            id: "0".to_string(),
                            title: "Claude Code".to_string(),
                            provider: Some(AgentProviderKind::ClaudeCode),
                            launch_config: Some(TerminalLaunchConfig::for_provider(
                                AgentProviderKind::ClaudeCode,
                            )),
                            restore_status: TerminalRestoreStatus::NotStarted,
                        }],
                    },
                    None,
                ),
            ),
        ]);

        let chosen = choose_initial_section(&[project], &section_states, None);

        assert_eq!(chosen, Some(task_section));
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
                    provider: Some(AgentProviderKind::ClaudeCode),
                    launch_config: Some(TerminalLaunchConfig::for_provider(
                        AgentProviderKind::ClaudeCode,
                    )),
                    restore_status: TerminalRestoreStatus::NotStarted,
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
                        provider: Some(AgentProviderKind::Codex),
                        launch_config: Some(TerminalLaunchConfig::for_provider(
                            AgentProviderKind::Codex,
                        )),
                        restore_status: TerminalRestoreStatus::Launching,
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
        assert!(output_mentions_missing_claude_conversation(
            "Error: No conversation found for session abc123"
        ));
        assert!(!output_mentions_missing_claude_conversation(
            "Error: network request failed"
        ));
    }

    #[test]
    fn trim_to_recent_output_limit_preserves_utf8_boundaries() {
        let mut buffer = format!("é{}", "a".repeat(TERMINAL_RECENT_OUTPUT_LIMIT - 1));

        trim_to_recent_output_limit(&mut buffer);

        assert_eq!(buffer.len(), TERMINAL_RECENT_OUTPUT_LIMIT - 1);
        assert!(buffer.is_char_boundary(0));
        assert_eq!(buffer.chars().next(), Some('a'));
    }
}

// ── Render ───────────────────────────────────────────────────────────

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
                        Timer::after(interval).await;
                        let next_interval = handle.update(async_cx, |this, cx| {
                            let mut should_notify = false;
                            should_notify |= this.drain_git_action(cx);
                            should_notify |= this.drain_changed_files_git_mutations(cx);
                            should_notify |= this.drain_git_refresh();
                            should_notify |= this.drain_task_creation(cx);
                            should_notify |= this.drain_project_add(cx);
                            should_notify |= this.drain_project_github_link_lookup();
                            should_notify |= this.drain_terminal_launch_replies(cx);
                            should_notify |= this.drain_warm_terminal_launch_replies(cx);
                            should_notify |= this.tick_toasts();
                            this.maybe_schedule_active_git_refresh(cx);
                            if should_notify {
                                cx.notify();
                            }
                            this.refresh_timer_interval()
                        });
                        let Ok(next_interval) = next_interval else {
                            break;
                        };
                        interval = next_interval;
                    }
                })
                .detach();
        }
        // Scale the entire UI based on the zoom level (font_size relative to default 13.0).
        let scale = self.font_size / 13.0;
        window.set_rem_size(px(16.0 * scale));

        // ── Settings page (replaces normal layout) ─────────────
        if self.settings_open {
            let settings = self.render_settings_page(window, cx);

            #[cfg(target_os = "macos")]
            {
                return AppInputHost::new(
                    div()
                        .flex()
                        .flex_col()
                        .relative()
                        .size_full()
                        .track_focus(&self.focus_handle)
                        .bg(theme::chrome_bg(window))
                        .on_key_down(cx.listener(Self::handle_global_key_down))
                        .on_action(cx.listener(Self::zoom_in))
                        .on_action(cx.listener(Self::zoom_out))
                        .on_action(cx.listener(Self::zoom_reset))
                        .child(settings)
                        .child(self.toast_layer(cx)),
                    self.focus_handle.clone(),
                    view.clone(),
                );
            }

            #[cfg(not(target_os = "macos"))]
            {
                return AppInputHost::new(
                    div()
                        .flex()
                        .flex_col()
                        .relative()
                        .size_full()
                        .track_focus(&self.focus_handle)
                        .on_key_down(cx.listener(Self::handle_global_key_down))
                        .on_action(cx.listener(Self::zoom_in))
                        .on_action(cx.listener(Self::zoom_out))
                        .on_action(cx.listener(Self::zoom_reset))
                        .child(settings)
                        .child(self.toast_layer(cx)),
                    self.focus_handle.clone(),
                    view.clone(),
                );
            }
        }

        // ── Normal main layout ──────────────────────────────────
        self.clamp_layout(window);
        self.sync_workspace_layout(cx);
        self.ensure_active_terminal_runtime(window, cx);
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
            .bg(theme::chrome_bg(window))
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
                    .child(Self::footer_settings_button(window, cx))
                    .child(Self::footer_add_project_button(window, cx)),
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

        #[cfg(target_os = "macos")]
        {
            AppInputHost::new(
                div()
                    .flex()
                    .flex_col()
                    .relative()
                    .size_full()
                    .track_focus(&self.focus_handle)
                    .bg(theme::chrome_bg(window))
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_key_down(cx.listener(Self::handle_global_key_down))
                    .on_action(cx.listener(Self::zoom_in))
                    .on_action(cx.listener(Self::zoom_out))
                    .on_action(cx.listener(Self::zoom_reset))
                    .child(Self::mac_title_strip(window, cx, busy))
                    .child(main)
                    .child(footer)
                    .child(self.project_menu_overlay(sw, cx))
                    .child(self.sidebar_task_menu_overlay(window, cx))
                    .child(self.new_task_modal_overlay(cx))
                    .child(self.add_agent_modal_overlay(cx))
                    .child(self.project_remove_confirm_modal(cx))
                    .child(self.sidebar_task_delete_confirm_modal(cx))
                    .child(self.toast_layer(cx)),
                self.focus_handle.clone(),
                view,
            )
        }

        #[cfg(not(target_os = "macos"))]
        {
            AppInputHost::new(
                div()
                    .flex()
                    .flex_col()
                    .relative()
                    .size_full()
                    .track_focus(&self.focus_handle)
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_key_down(cx.listener(Self::handle_global_key_down))
                    .on_action(cx.listener(Self::zoom_in))
                    .on_action(cx.listener(Self::zoom_out))
                    .on_action(cx.listener(Self::zoom_reset))
                    .child(main)
                    .child(footer)
                    .child(self.project_menu_overlay(sw, cx))
                    .child(self.sidebar_task_menu_overlay(window, cx))
                    .child(self.new_task_modal_overlay(cx))
                    .child(self.add_agent_modal_overlay(cx))
                    .child(self.project_remove_confirm_modal(cx))
                    .child(self.sidebar_task_delete_confirm_modal(cx))
                    .child(self.toast_layer(cx)),
                self.focus_handle.clone(),
                view,
            )
        }
    }
}
