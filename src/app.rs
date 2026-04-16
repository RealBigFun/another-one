//! Application state, core event handlers, animation, and `Render` impl.

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    actions, div, hsla, prelude::*, px, rems, rgb, svg, AnyElement, AnyView, App, Bounds,
    ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, Focusable, GlobalElementId, InspectorElementId, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, SharedString, Timer,
    UTF16Selection, WeakEntity, Window,
};

actions!(zoom, [ZoomIn, ZoomOut, ZoomReset]);

use crate::agents::{
    prepare_launch_config_for_spawn, terminal_launch_config_for_selected_agents, AgentProviderKind,
    ResumeTarget, TerminalLaunchConfig, TerminalLaunchKind, AGENTS,
};
use crate::layout::*;
use crate::project_store::{
    ChangedFile, DirectTask, PersistedSectionState, PersistedTerminalTab, ProjectGitState,
    ProjectStore,
};
use crate::terminal::TerminalInstance;
use crate::theme;

const ACTIVE_TERMINAL_FRAME_INTERVAL: Duration = Duration::from_millis(80);
const ACTIVE_TERMINAL_TYPING_FRAME_INTERVAL: Duration = Duration::from_millis(33);
const ACTIVE_TERMINAL_TYPING_GRACE: Duration = Duration::from_millis(180);
const ACTIVE_GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(4);
const BUSY_TERMINAL_GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(12);
const ACTIVE_GIT_METADATA_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const TERMINAL_CAPTURE_POLL_INTERVAL: Duration = Duration::from_millis(500);
const TERMINAL_CAPTURE_POLL_LIMIT: usize = 16;
const TOAST_ANIMATION_REFRESH_INTERVAL: Duration = Duration::from_millis(16);
const TERMINAL_BUSY_GRACE: Duration = Duration::from_secs(2);
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
    pub id: usize,
    pub title: String,
    pub launch_config: TerminalLaunchConfig,
    /// Real terminal instance (PTY + grid).
    pub terminal: Option<TerminalInstance>,
    launch_failed: bool,
    resume_capture_pending: bool,
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
            tabs: vec![TerminalTab::new(0, launch_config)],
            active_tab: 0,
            next_tab_id: 1,
            cwd,
        }
    }

    pub fn add_tab(&mut self) -> usize {
        let launch_config = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.launch_config.fresh_clone_for_new_tab())
            .unwrap_or_default();
        self.add_tab_with_launch_config(launch_config)
    }

    pub fn add_tab_with_launch_config(&mut self, launch_config: TerminalLaunchConfig) -> usize {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(TerminalTab::new(id, launch_config));
        self.active_tab = self.tabs.len() - 1;
        id
    }

    pub fn close_tab(&mut self, index: usize) -> Option<usize> {
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
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.launch_failed = false;
        }
        true
    }

    pub fn active_tab_id(&self) -> usize {
        self.tabs
            .get(self.active_tab)
            .map(|tab| tab.id)
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
            tabs.push(TerminalTab::new(0, TerminalLaunchConfig::default()));
        }

        let max_tab_id = tabs.iter().map(|tab| tab.id).max().unwrap_or(0);
        let active_tab = tabs
            .iter()
            .position(|tab| tab.id == persisted.active_tab_id)
            .unwrap_or_else(|| tabs.len().saturating_sub(1));

        Self {
            tabs,
            active_tab,
            next_tab_id: persisted.next_tab_id.max(max_tab_id + 1),
            cwd: persisted.cwd.or(fallback_cwd),
        }
    }
}

impl TerminalTab {
    fn new(id: usize, launch_config: TerminalLaunchConfig) -> Self {
        let title = launch_config.default_title();
        Self {
            id,
            title,
            launch_config,
            terminal: None,
            launch_failed: false,
            resume_capture_pending: false,
        }
    }

    fn to_persisted(&self) -> PersistedTerminalTab {
        PersistedTerminalTab {
            id: self.id,
            title: self.title.clone(),
            kind: self.launch_config.kind,
            provider: self.launch_config.provider,
            launch_argv: self.launch_config.persisted_launch_argv(),
            resume_target: self.launch_config.resume_target.clone(),
        }
    }

    fn from_persisted(persisted: PersistedTerminalTab) -> Self {
        let mut launch_config = if let Some(provider) = persisted.provider {
            TerminalLaunchConfig::for_provider(provider)
        } else {
            TerminalLaunchConfig::default()
        };
        launch_config.kind = persisted.kind;
        if persisted.kind == TerminalLaunchKind::Agent {
            launch_config.launch_argv = if persisted.launch_argv.is_empty() {
                launch_config.launch_argv.clone()
            } else {
                persisted.launch_argv.clone()
            };
            if let Some(resume_target) = persisted.resume_target.clone() {
                launch_config = launch_config.with_resume_target(resume_target);
            }
        } else {
            launch_config.launch_argv = Vec::new();
        }

        Self {
            id: persisted.id,
            title: persisted.title,
            launch_config,
            terminal: None,
            launch_failed: false,
            resume_capture_pending: false,
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
    result: Result<crate::project_store::Project, String>,
}

struct TerminalSpawnReply {
    section_id: SectionId,
    tab_id: usize,
    result: Result<TerminalInstance, String>,
}

struct TerminalMetadataReply {
    section_id: SectionId,
    tab_id: usize,
    launch_config: Option<TerminalLaunchConfig>,
    capture_finished: bool,
}

#[derive(Debug, Default)]
struct CodexSessionCaptureState {
    index_offsets: HashMap<PathBuf, u64>,
    session_paths: HashMap<String, PathBuf>,
}

struct ProjectGitHubLinkReply {
    project_id: String,
    github_url: Option<String>,
}

struct TaskCreationSuccess {
    original_project_id: String,
    project: crate::project_store::Project,
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
    pub(crate) is_worktree: bool,
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
    pub(crate) task_id: Option<String>,
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

#[derive(Debug, Clone)]
pub(crate) struct TerminalViewport {
    pub(crate) section_id: SectionId,
    pub(crate) origin_x: f32,
    pub(crate) origin_y: f32,
    pub(crate) cell_w: f32,
    pub(crate) cell_h: f32,
    pub(crate) cols: usize,
    pub(crate) rows: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TerminalSelectionDrag {
    pub(crate) section_id: SectionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HoveredTerminalLink {
    pub(crate) section_id: SectionId,
    pub(crate) row: usize,
    pub(crate) col: usize,
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
    /// Per-section terminal tab state.
    pub(crate) section_states: HashMap<SectionId, SectionState>,
    /// Last rendered terminal viewport geometry for pointer selection.
    pub(crate) terminal_viewport: Option<TerminalViewport>,
    /// Hovered terminal link cell, if any.
    pub(crate) hovered_terminal_link: Option<HoveredTerminalLink>,
    /// Active terminal mouse selection drag, if any.
    pub(crate) terminal_selection_drag: Option<TerminalSelectionDrag>,
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
            terminal_viewport: None,
            hovered_terminal_link: None,
            terminal_selection_drag: None,
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
        let mut changed = self.active_project_page.as_deref() != Some(project_id.as_str())
            || self.active_section.is_some();
        changed |= self.clear_terminal_viewport_state();
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
        let mut changed =
            self.active_section.as_ref() != Some(&section_id) || self.active_project_page.is_some();
        changed |= self.clear_terminal_viewport_state();
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

        changed |= self.clear_terminal_viewport_state();

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

        changed |= self.clear_terminal_viewport_state();

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
    ) -> bool {
        let added = self
            .section_states
            .get_mut(section_id)
            .is_some_and(|state| {
                state.add_tab_with_launch_config(launch_config.clone());
                true
            });
        if added {
            self.persist_section_state(section_id, cx);
            cx.notify();
        }
        added
    }

    pub(crate) fn close_tab(
        &mut self,
        section_id: &SectionId,
        tab_index: usize,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let removed_tab_id = self
            .section_states
            .get_mut(section_id)
            .and_then(|state| state.close_tab(tab_index));
        if removed_tab_id.is_some() {
            if let Some(tab_id) = removed_tab_id {
                self.cleanup_removed_tab(section_id, tab_id, cx);
            }
            self.persist_section_state(section_id, cx);
            cx.notify();
        }
        removed_tab_id
    }

    pub(crate) fn note_terminal_input_activity(&self, cx: &mut Context<Self>) {
        let _ = self
            .app
            .update(cx, |app, _| app.note_terminal_input_activity());
    }

    pub(crate) fn ensure_active_terminal_spawned(
        &self,
        section_id: &SectionId,
        cx: &mut Context<Self>,
    ) {
        let app = self.app.clone();
        let section_id = section_id.clone();
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, app_cx| {
                app.ensure_active_terminal_spawned(&section_id, app_cx);
            });
        });
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

    fn cleanup_removed_tab(&self, section_id: &SectionId, tab_id: usize, cx: &mut Context<Self>) {
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
            app.open_new_task_modal(&project_id);
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
                app.open_add_agent_modal(section_id.clone(), selected_agent_id.clone());
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

    pub(crate) fn update_terminal_viewport(
        &mut self,
        section_id: &SectionId,
        cell_w: f32,
        cell_h: f32,
        cols: usize,
        rows: usize,
    ) {
        let top_offset = if cfg!(target_os = "macos") {
            TITLEBAR_CHROME_H
        } else {
            0.0
        };

        self.terminal_viewport = Some(TerminalViewport {
            section_id: section_id.clone(),
            origin_x: self.sidebar_w + GUTTER + 8.0,
            origin_y: top_offset + 36.0 + 6.0,
            cell_w,
            cell_h,
            cols,
            rows,
        });
    }

    pub(crate) fn clear_terminal_viewport(&mut self, cx: &mut Context<Self>) {
        if self.clear_terminal_viewport_state() {
            cx.notify();
        }
    }

    fn clear_terminal_viewport_state(&mut self) -> bool {
        self.terminal_viewport.take().is_some()
            || self.hovered_terminal_link.take().is_some()
            || self.terminal_selection_drag.take().is_some()
    }

    pub(crate) fn terminal_cell_at_position(
        &self,
        position: Point<Pixels>,
    ) -> Option<(SectionId, usize, usize)> {
        let viewport = self.terminal_viewport.as_ref()?;
        if viewport.cols == 0 || viewport.rows == 0 {
            return None;
        }

        let local_x = f32::from(position.x) - viewport.origin_x;
        let local_y = f32::from(position.y) - viewport.origin_y;

        let col = (local_x / viewport.cell_w).floor() as i32;
        let row = (local_y / viewport.cell_h).floor() as i32;

        let col = col.clamp(0, viewport.cols.saturating_sub(1) as i32) as usize;
        let row = row.clamp(0, viewport.rows.saturating_sub(1) as i32) as usize;

        Some((viewport.section_id.clone(), row, col))
    }

    pub(crate) fn handle_terminal_mouse_move(
        &mut self,
        ev: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.update_terminal_selection_drag(ev, cx) {
            self.set_hovered_terminal_link(None, cx);
            return true;
        }

        self.update_hovered_terminal_link(ev, cx);
        false
    }

    pub(crate) fn finish_terminal_drag(&mut self) -> bool {
        self.terminal_selection_drag.take().is_some()
    }

    fn update_terminal_selection_drag(
        &mut self,
        ev: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(drag) = self.terminal_selection_drag.as_ref() else {
            return false;
        };
        if !ev.dragging() {
            return false;
        }

        let Some((section_id, row, col)) = self.terminal_cell_at_position(ev.position) else {
            return false;
        };
        if section_id != drag.section_id {
            return false;
        }

        let Some(state) = self.section_states.get(&section_id) else {
            return false;
        };
        let Some(tab) = state.tabs.get(state.active_tab) else {
            return false;
        };
        let Some(terminal) = tab.terminal.as_ref() else {
            return false;
        };

        terminal.update_selection(row, col);
        cx.notify();
        true
    }

    pub(crate) fn set_hovered_terminal_link(
        &mut self,
        hovered: Option<HoveredTerminalLink>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.hovered_terminal_link == hovered {
            return false;
        }

        self.hovered_terminal_link = hovered;
        cx.notify();
        true
    }

    fn update_hovered_terminal_link(
        &mut self,
        ev: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let hovered = if ev.dragging() || self.terminal_selection_drag.is_some() {
            None
        } else {
            self.terminal_cell_at_position(ev.position)
                .and_then(|(section_id, row, col)| {
                    let state = self.section_states.get(&section_id)?;
                    let tab = state.tabs.get(state.active_tab)?;
                    let terminal = tab.terminal.as_ref()?;
                    terminal
                        .link_hint_at_viewport_cell(row, col)
                        .map(|_| HoveredTerminalLink {
                            section_id,
                            row,
                            col,
                        })
                })
        };

        self.set_hovered_terminal_link(hovered, cx)
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
    /// Sender used by background terminal spawns.
    terminal_spawn_sender: mpsc::Sender<TerminalSpawnReply>,
    /// Receiver for background terminal spawns.
    terminal_spawn_receiver: mpsc::Receiver<TerminalSpawnReply>,
    /// Sender used by background launch metadata capture tasks.
    terminal_metadata_sender: mpsc::Sender<TerminalMetadataReply>,
    /// Receiver for background launch metadata capture tasks.
    terminal_metadata_receiver: mpsc::Receiver<TerminalMetadataReply>,
    /// Active terminal spawns keyed by section + tab id.
    pending_terminal_spawns: HashSet<(SectionId, usize)>,
    /// Discovery-based launches waiting for a resume target, keyed by section + tab id.
    pending_terminal_captures: HashSet<(SectionId, usize)>,
    /// Shared incremental state for Codex resume-target discovery.
    codex_session_capture_state: Arc<Mutex<CodexSessionCaptureState>>,
    /// Sender used by background project GitHub-link lookups.
    project_github_link_sender: mpsc::Sender<ProjectGitHubLinkReply>,
    /// Receiver for background project GitHub-link lookups.
    project_github_link_receiver: mpsc::Receiver<ProjectGitHubLinkReply>,
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
    /// Terminal font size (adjusted by Cmd+/Cmd- zoom).
    pub(crate) font_size: f32,
    /// Last time the active terminal received local keyboard input.
    pub(crate) last_terminal_input: Cell<Instant>,
    /// Last time changed-file state was refreshed from git.
    pub(crate) last_git_status_refresh: Instant,
    /// Last time branch/worktree metadata was refreshed from git.
    pub(crate) last_git_metadata_refresh: Instant,
    /// Last time PTY-driven terminal output triggered a repaint.
    pub(crate) last_terminal_output_redraw: Instant,
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
        match self.text_input_target() {
            TextInputTarget::NewTaskModal => self
                .new_task_modal
                .as_ref()
                .map(|state| text_for_utf16_range(&state.task_name, range, adjusted_range)),
            TextInputTarget::SidebarTaskRename => self
                .sidebar_task_rename
                .as_ref()
                .map(|state| text_for_utf16_range(&state.task_name, range, adjusted_range)),
            TextInputTarget::Terminal | TextInputTarget::Blocked => None,
        }
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        match self.text_input_target() {
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
            TextInputTarget::Terminal => terminal_selected_text_range(self, cx),
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
        match self.text_input_target() {
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
                    self.note_terminal_input_activity();
                    let replacement = text.to_string();
                    self.workspace_pane.update(cx, |workspace, _| {
                        let Some(section_id) = workspace.active_section.as_ref() else {
                            return;
                        };
                        let Some(state) = workspace.section_states.get(section_id) else {
                            return;
                        };
                        let Some(tab) = state.tabs.get(state.active_tab) else {
                            return;
                        };
                        let Some(terminal) = tab.terminal.as_ref() else {
                            return;
                        };
                        terminal.write_to_pty(replacement.as_bytes());
                    });
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
        match self.text_input_target() {
            TextInputTarget::NewTaskModal | TextInputTarget::SidebarTaskRename => {
                self.replace_text_in_range(range, new_text, _window, cx);
                self.marked_text = if new_text.is_empty() {
                    None
                } else {
                    Some(new_text.to_string())
                };
                return;
            }
            TextInputTarget::Terminal | TextInputTarget::Blocked => {}
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

fn terminal_selected_text_range(app: &AnotherOneApp, cx: &App) -> Option<UTF16Selection> {
    let workspace = app.workspace_pane.read(cx);
    let alt_screen = workspace
        .active_section
        .as_ref()
        .and_then(|sid| workspace.section_states.get(sid))
        .and_then(|state| state.tabs.get(state.active_tab))
        .and_then(|tab| tab.terminal.as_ref())
        .and_then(|terminal| {
            terminal.term.lock().ok().map(|t| {
                t.mode()
                    .contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
            })
        })
        .unwrap_or(false);

    if alt_screen {
        None
    } else {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
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
    fn text_input_target(&self) -> TextInputTarget {
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

        TextInputTarget::Terminal
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
        let (terminal_spawn_sender, terminal_spawn_receiver) = mpsc::channel();
        let (terminal_metadata_sender, terminal_metadata_receiver) = mpsc::channel();
        let codex_session_capture_state = Arc::new(Mutex::new(CodexSessionCaptureState::default()));
        let (project_github_link_sender, project_github_link_receiver) = mpsc::channel();
        let (changed_files_git_mutation_sender, changed_files_git_mutation_receiver) =
            mpsc::channel();
        let expanded = store.ui.expanded_project_ids.clone().unwrap_or_else(|| {
            let mut expanded = HashSet::new();
            if let Some(first) = store.projects.first() {
                expanded.insert(first.id.clone());
            }
            expanded
        });
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
            store.ui.last_active_section_key.as_deref(),
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

        let mut app = Self {
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
            terminal_spawn_sender,
            terminal_spawn_receiver,
            terminal_metadata_sender,
            terminal_metadata_receiver,
            pending_terminal_spawns: HashSet::new(),
            pending_terminal_captures: HashSet::new(),
            codex_session_capture_state,
            project_github_link_sender,
            project_github_link_receiver,
            project_github_link_requests: HashSet::new(),
            project_github_link_checked: HashSet::new(),
            settings_open: false,
            settings_section: crate::settings_page::SettingsSection::Agents,
            marked_text: None,
            add_agent_modal: None,
            sidebar_task_last_click: None,
            font_size: initial_font_size,
            last_terminal_input: Cell::new(Instant::now() - ACTIVE_TERMINAL_TYPING_GRACE),
            last_git_status_refresh: Instant::now() - ACTIVE_GIT_STATUS_REFRESH_INTERVAL,
            last_git_metadata_refresh: Instant::now() - ACTIVE_GIT_METADATA_REFRESH_INTERVAL,
            last_terminal_output_redraw: Instant::now() - ACTIVE_TERMINAL_FRAME_INTERVAL,
        };

        if let Some(section_id) = app.workspace_pane.read(cx).active_section.clone() {
            app.ensure_active_terminal_spawned(&section_id, cx);
        }

        app
    }

    pub(crate) fn note_terminal_input_activity(&self) {
        self.last_terminal_input.set(Instant::now());
    }

    fn set_last_active_section_key(&mut self, section_key: Option<String>) {
        self.project_store.set_last_active_section_key(section_key);
    }

    fn persist_section_state(&mut self, section_id: &SectionId, persisted: PersistedSectionState) {
        self.project_store
            .set_terminal_section(section_id.store_key(), persisted);
    }

    fn persist_section_state_from_workspace(&mut self, section_id: &SectionId, cx: &App) {
        let persisted = self
            .workspace_pane
            .read(cx)
            .section_states
            .get(section_id)
            .map(SectionState::to_persisted);

        if let Some(persisted) = persisted {
            self.persist_section_state(section_id, persisted);
        } else {
            self.project_store
                .remove_terminal_section(&section_id.store_key());
        }
    }

    fn remove_persisted_sections(&mut self, section_ids: &HashSet<SectionId>) {
        let section_keys = section_ids
            .iter()
            .map(SectionId::store_key)
            .collect::<HashSet<_>>();
        self.project_store.remove_terminal_sections(&section_keys);
    }

    fn cleanup_removed_tab(&mut self, section_id: &SectionId, tab_id: usize) {
        self.pending_terminal_spawns
            .remove(&(section_id.clone(), tab_id));
        self.pending_terminal_captures
            .remove(&(section_id.clone(), tab_id));
    }

    fn cleanup_removed_sections(&mut self, section_ids: &HashSet<SectionId>) {
        self.pending_terminal_spawns
            .retain(|(section_id, _)| !section_ids.contains(section_id));
        self.pending_terminal_captures
            .retain(|(section_id, _)| !section_ids.contains(section_id));
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

    fn active_terminal_frame_interval(&self) -> Duration {
        if self.last_terminal_input.get().elapsed() < ACTIVE_TERMINAL_TYPING_GRACE {
            ACTIVE_TERMINAL_TYPING_FRAME_INTERVAL
        } else {
            ACTIVE_TERMINAL_FRAME_INTERVAL
        }
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

    pub(crate) fn ensure_active_terminal_spawned(
        &mut self,
        section_id: &SectionId,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace_pane.read(cx);
        let Some((tab_id, cwd, launch_config)) =
            workspace.section_states.get(section_id).and_then(|state| {
                state
                    .tabs
                    .get(state.active_tab)
                    .filter(|tab| tab.terminal.is_none() && !tab.launch_failed)
                    .map(|tab| (tab.id, state.cwd.clone(), tab.launch_config.clone()))
            })
        else {
            return;
        };

        if !self
            .pending_terminal_spawns
            .insert((section_id.clone(), tab_id))
        {
            return;
        }

        if launch_requires_background_resume_capture(&launch_config) {
            self.pending_terminal_captures
                .insert((section_id.clone(), tab_id));
            self.workspace_pane.update(cx, |workspace, _| {
                let Some(state) = workspace.section_states.get_mut(section_id) else {
                    return;
                };
                let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };
                tab.resume_capture_pending = true;
            });
        }

        let tx = self.terminal_spawn_sender.clone();
        let metadata_tx = self.terminal_metadata_sender.clone();
        let codex_capture_state = Arc::clone(&self.codex_session_capture_state);
        let section_id = section_id.clone();
        std::thread::spawn(move || {
            let launch_started_at = std::time::SystemTime::now();
            let prepared_launch_config =
                match prepare_launch_config_for_spawn(&launch_config, cwd.as_deref()) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        let _ = tx.send(TerminalSpawnReply {
                            section_id,
                            tab_id,
                            result: Err(error),
                        });
                        return;
                    }
                };
            let send_metadata_before_spawn = matches!(
                prepared_launch_config.provider,
                Some(AgentProviderKind::CursorAgent)
            ) && prepared_launch_config.resume_target
                != launch_config.resume_target;

            if send_metadata_before_spawn {
                let _ = metadata_tx.send(TerminalMetadataReply {
                    section_id: section_id.clone(),
                    tab_id,
                    launch_config: Some(prepared_launch_config.clone()),
                    capture_finished: true,
                });
            }

            let result = TerminalInstance::new(80, 24, cwd.as_deref(), &prepared_launch_config)
                .map_err(|error| format!("Could not start the terminal: {error}"));

            if result.is_ok() && !send_metadata_before_spawn {
                if prepared_launch_config.resume_target != launch_config.resume_target {
                    let _ = metadata_tx.send(TerminalMetadataReply {
                        section_id: section_id.clone(),
                        tab_id,
                        launch_config: Some(prepared_launch_config.clone()),
                        capture_finished: true,
                    });
                }

                if launch_requires_background_resume_capture(&prepared_launch_config) {
                    let metadata_tx = metadata_tx.clone();
                    let section_id = section_id.clone();
                    let cwd = cwd.clone();
                    let provider = prepared_launch_config
                        .provider
                        .expect("discovery-based capture requires a provider");
                    std::thread::spawn(move || {
                        let launch_config = capture_resume_target_for_launch_with_state(
                            provider,
                            cwd.as_deref(),
                            launch_started_at,
                            &prepared_launch_config,
                            &codex_capture_state,
                        );
                        let _ = metadata_tx.send(TerminalMetadataReply {
                            section_id,
                            tab_id,
                            launch_config,
                            capture_finished: true,
                        });
                    });
                }
            }

            let _ = tx.send(TerminalSpawnReply {
                section_id,
                tab_id,
                result,
            });
        });
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

    fn flush_pending_terminal_resizes(&mut self, cx: &mut Context<Self>) -> bool {
        self.workspace_pane.update(cx, |workspace, _| {
            let mut resized = false;
            for section in workspace.section_states.values_mut() {
                for tab in &mut section.tabs {
                    if let Some(terminal) = tab.terminal.as_mut() {
                        resized |= terminal.flush_resize();
                    }
                }
            }
            resized
        })
    }

    fn should_notify_active_terminal(&mut self, cx: &App) -> bool {
        let workspace = self.workspace_pane.read(cx);
        let Some(section_id) = workspace.active_section.clone() else {
            return false;
        };

        let frame_interval = self.active_terminal_frame_interval();
        let Some(terminal) = workspace
            .section_states
            .get(&section_id)
            .and_then(|section| section.tabs.get(section.active_tab))
            .and_then(|tab| tab.terminal.as_ref())
        else {
            return false;
        };

        if !terminal.is_dirty() {
            return false;
        }

        if self.last_terminal_output_redraw.elapsed() < frame_interval {
            return false;
        }

        self.last_terminal_output_redraw = Instant::now();
        terminal.take_dirty()
    }

    fn git_status_refresh_interval(&self) -> Duration {
        if self.last_terminal_output_redraw.elapsed() < TERMINAL_BUSY_GRACE {
            BUSY_TERMINAL_GIT_STATUS_REFRESH_INTERVAL
        } else {
            ACTIVE_GIT_STATUS_REFRESH_INTERVAL
        }
    }

    fn apply_project_git_state(&mut self, project_id: &str, state: ProjectGitState) -> bool {
        let mut changed = false;

        let ProjectGitState {
            changed_files,
            ahead_count,
            metadata,
        } = state;

        if let Some(metadata) = metadata {
            if let Some(project) = self
                .project_store
                .projects
                .iter_mut()
                .find(|project| project.id == project_id)
            {
                if project.branches != metadata.branches {
                    project.branches = metadata.branches;
                    changed = true;
                }
                if project.worktree_name != metadata.worktree_name {
                    project.worktree_name = metadata.worktree_name;
                    changed = true;
                }
                if project.repo_common_dir != metadata.repo_common_dir {
                    project.repo_common_dir = metadata.repo_common_dir;
                    changed = true;
                }
            }
        }

        if let Some(project) = self
            .project_store
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
        {
            if let Some(current_branch) =
                project.branches.iter_mut().find(|branch| branch.is_current)
            {
                if current_branch.ahead_count != ahead_count {
                    current_branch.ahead_count = ahead_count;
                    changed = true;
                }
            }
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

    fn activate_project_section(
        &mut self,
        project_id: &str,
        branch_name: &str,
        cwd: std::path::PathBuf,
        launch_config: Option<TerminalLaunchConfig>,
        cx: &mut Context<Self>,
    ) {
        let section_id = SectionId::new(project_id, branch_name);
        self.workspace_pane.update(cx, |workspace, cx| {
            workspace.activate_section(section_id, Some(cwd), launch_config, cx);
        });
        self.mark_git_refresh_stale();
    }

    pub(crate) fn submit_new_task_modal(&mut self, cx: &mut Context<Self>) {
        let (
            project_id,
            task_name,
            generated_task_name,
            source_branch,
            worktree_mode,
            launch_config,
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
            self.new_task_modal = None;
            return;
        };

        if !worktree_mode {
            let branch_name = crate::project_store::current_branch(&project.path)
                .or_else(|| {
                    project
                        .branches
                        .iter()
                        .find(|branch| branch.is_current)
                        .or_else(|| project.branches.first())
                        .map(|branch| branch.name.clone())
                })
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
            self.project_store
                .direct_tasks
                .entry(project.id.clone())
                .or_default()
                .push(DirectTask {
                    id: task_id.clone(),
                    name: task_name.clone(),
                    branch_name: branch_name.clone(),
                });
            self.project_store.save();
            self.expanded_projects.insert(project.id.clone());
            self.project_store
                .set_expanded_projects(&self.expanded_projects);
            let section_id = SectionId::for_task(&project.id, &branch_name, &task_id);
            let project_path = project.path.clone();
            let launch_config = launch_config.clone();
            self.workspace_pane.update(cx, |workspace, cx| {
                workspace.activate_section(section_id, Some(project_path), Some(launch_config), cx);
            });
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
                    |_| crate::project_store::Project {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: created
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| created.path.display().to_string()),
                        path: created.path.clone(),
                        settings: crate::project_store::ProjectSettings::default(),
                        worktrees: Vec::new(),
                        branches: Vec::new(),
                        worktree_name: None,
                        repo_common_dir: None,
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
                        let project = success.project.clone();
                        let inserted = self.project_store.insert_project(project.clone());
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

                        self.expanded_projects
                            .insert(success.original_project_id.clone());
                        self.project_store
                            .set_expanded_projects(&self.expanded_projects);
                        self.activate_project_section(
                            &project.id,
                            &success.branch_name,
                            project.path.clone(),
                            Some(success.launch_config),
                            cx,
                        );
                        self.new_task_modal = None;
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
                        let project_name = project.name.clone();
                        let project_id = project.id.clone();
                        let added = self.project_store.insert_project(project.clone());
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

    fn drain_terminal_metadata(&mut self, cx: &mut Context<Self>) -> bool {
        let mut should_notify = false;

        while let Ok(reply) = self.terminal_metadata_receiver.try_recv() {
            let section_id = reply.section_id.clone();
            let launch_config = reply.launch_config.clone();
            let capture_finished = reply.capture_finished;
            let updated = self.workspace_pane.update(cx, |workspace, cx| {
                let Some(state) = workspace.section_states.get_mut(&section_id) else {
                    return false;
                };
                let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == reply.tab_id) else {
                    return false;
                };

                if let Some(launch_config) = launch_config.as_ref() {
                    tab.launch_config = launch_config.clone();
                    tab.launch_failed = false;
                }
                if capture_finished {
                    tab.resume_capture_pending = false;
                }
                cx.notify();
                true
            });

            if capture_finished {
                self.pending_terminal_captures
                    .remove(&(section_id.clone(), reply.tab_id));
            }

            if updated {
                if launch_config.is_some() {
                    self.persist_section_state_from_workspace(&section_id, cx);
                }
                should_notify = true;
            }
        }

        should_notify
    }

    fn drain_terminal_spawn(&mut self, cx: &mut Context<Self>) -> bool {
        let mut should_notify = false;

        while let Ok(reply) = self.terminal_spawn_receiver.try_recv() {
            self.pending_terminal_spawns
                .remove(&(reply.section_id.clone(), reply.tab_id));

            match reply.result {
                Ok(terminal) => {
                    let section_id = reply.section_id.clone();
                    let tab_id = reply.tab_id;
                    should_notify |= self.workspace_pane.update(cx, |workspace, cx| {
                        let Some(state) = workspace.section_states.get_mut(&section_id) else {
                            return false;
                        };
                        let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                            return false;
                        };
                        tab.terminal = Some(terminal);
                        tab.launch_failed = false;
                        cx.notify();
                        true
                    });
                }
                Err(error) => {
                    self.pending_terminal_captures
                        .remove(&(reply.section_id.clone(), reply.tab_id));

                    let section_id = reply.section_id.clone();
                    let tab_id = reply.tab_id;
                    self.workspace_pane.update(cx, |workspace, cx| {
                        let Some(state) = workspace.section_states.get_mut(&section_id) else {
                            return;
                        };
                        let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                            return;
                        };
                        tab.launch_failed = true;
                        tab.resume_capture_pending = false;
                        cx.notify();
                    });
                    self.show_error_toast(error, cx);
                    should_notify = true;
                }
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

        if self.workspace_pane.update(cx, |workspace, cx| {
            workspace.handle_terminal_mouse_move(ev, cx)
        }) {
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
        let had_terminal_drag = self
            .workspace_pane
            .update(cx, |workspace, _| workspace.finish_terminal_drag());
        let had_layout_drag = self.drag.take().is_some();

        if had_layout_drag {
            self.clamp_layout(window);
            self.sync_workspace_layout(cx);
            self.project_store
                .set_left_sidebar_open(self.sidebar_is_open());
        }

        if had_toast_drag || had_terminal_drag || had_layout_drag {
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
        if self.toasts.is_empty() && self.copied_toast.is_none() {
            self.active_terminal_frame_interval()
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexSessionCandidate {
    session_id: String,
    cwd: PathBuf,
    modified_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PiSessionCandidate {
    session_path: PathBuf,
    cwd: PathBuf,
    modified_at: SystemTime,
}

fn launch_requires_background_resume_capture(launch_config: &TerminalLaunchConfig) -> bool {
    matches!(
        launch_config.provider,
        Some(provider)
            if launch_config.kind == TerminalLaunchKind::Agent
                && provider.is_discovery_based()
                && launch_config.resume_target.is_none()
    )
}

fn capture_resume_target_for_launch_with_state(
    provider: AgentProviderKind,
    cwd: Option<&Path>,
    launch_started_at: SystemTime,
    launch_config: &TerminalLaunchConfig,
    codex_capture_state: &Arc<Mutex<CodexSessionCaptureState>>,
) -> Option<TerminalLaunchConfig> {
    let cwd = cwd?;
    let resume_target = match provider {
        AgentProviderKind::Codex => {
            capture_codex_resume_target(cwd, launch_started_at, codex_capture_state)
        }
        AgentProviderKind::Pi => capture_pi_resume_target(cwd, launch_started_at),
        _ => None,
    }?;

    Some(launch_config.with_resume_target(resume_target))
}

fn capture_codex_resume_target(
    cwd: &Path,
    launch_started_at: SystemTime,
    capture_state: &Arc<Mutex<CodexSessionCaptureState>>,
) -> Option<ResumeTarget> {
    let home = dirs::home_dir()?;
    let index_path = home.join(".codex").join("session_index.jsonl");
    let sessions_root = home.join(".codex").join("sessions");
    let mut pending_session_ids = Vec::new();
    let mut seen_session_ids = HashSet::new();

    for _ in 0..TERMINAL_CAPTURE_POLL_LIMIT {
        let new_session_ids = {
            let mut state = capture_state.lock().ok()?;
            read_new_codex_index_ids(&index_path, &mut state)
        };
        for session_id in new_session_ids {
            if seen_session_ids.insert(session_id.clone()) {
                pending_session_ids.push(session_id);
            }
        }

        let target = {
            let mut state = capture_state.lock().ok()?;
            find_codex_resume_target(
                &pending_session_ids,
                &sessions_root,
                cwd,
                launch_started_at,
                &mut state,
            )
        };

        if let Some(target) = target {
            return Some(target);
        }
        std::thread::sleep(TERMINAL_CAPTURE_POLL_INTERVAL);
    }

    None
}

fn capture_pi_resume_target(cwd: &Path, launch_started_at: SystemTime) -> Option<ResumeTarget> {
    let home = dirs::home_dir()?;
    let slug = cwd_slug(cwd)?;
    let sessions_dir = home.join(".pi").join("agent").join("sessions").join(slug);

    for _ in 0..TERMINAL_CAPTURE_POLL_LIMIT {
        if let Some(target) = find_pi_resume_target(&sessions_dir, cwd, launch_started_at) {
            return Some(target);
        }
        std::thread::sleep(TERMINAL_CAPTURE_POLL_INTERVAL);
    }

    None
}

fn find_codex_resume_target(
    session_ids: &[String],
    sessions_root: &Path,
    cwd: &Path,
    launch_started_at: SystemTime,
    capture_state: &mut CodexSessionCaptureState,
) -> Option<ResumeTarget> {
    let candidates = session_ids
        .iter()
        .filter_map(|session_id| {
            let path = resolve_codex_session_path(session_id, sessions_root, capture_state)?;
            parse_codex_session_candidate(&path, launch_started_at)
        })
        .collect::<Vec<_>>();

    match_codex_resume_target(session_ids, &candidates, cwd, launch_started_at)
}

fn match_codex_resume_target(
    index_ids: &[String],
    candidates: &[CodexSessionCandidate],
    cwd: &Path,
    launch_started_at: SystemTime,
) -> Option<ResumeTarget> {
    let mut recent_candidates = candidates
        .iter()
        .filter(|candidate| {
            candidate.cwd == cwd
                && candidate
                    .modified_at
                    .duration_since(launch_started_at)
                    .is_ok()
        })
        .collect::<Vec<_>>();
    recent_candidates.sort_by_key(|candidate| candidate.modified_at);

    for session_id in index_ids.iter().rev() {
        if let Some(candidate) = recent_candidates
            .iter()
            .rev()
            .find(|candidate| &candidate.session_id == session_id)
        {
            return Some(ResumeTarget::id(candidate.session_id.clone()));
        }
    }

    recent_candidates
        .last()
        .map(|candidate| ResumeTarget::id(candidate.session_id.clone()))
}

fn read_new_codex_index_ids(
    index_path: &Path,
    capture_state: &mut CodexSessionCaptureState,
) -> Vec<String> {
    let Ok(file) = std::fs::File::open(index_path) else {
        return Vec::new();
    };
    let Ok(metadata) = file.metadata() else {
        return Vec::new();
    };

    let offset = capture_state
        .index_offsets
        .entry(index_path.to_path_buf())
        .or_insert(0);
    if *offset > metadata.len() {
        *offset = 0;
    }

    let mut reader = BufReader::new(file);
    if reader.seek(SeekFrom::Start(*offset)).is_err() {
        return Vec::new();
    }

    let mut ids = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let Ok(bytes_read) = reader.read_line(&mut line) else {
            return Vec::new();
        };
        if bytes_read == 0 {
            break;
        }
        if let Some(session_id) = parse_codex_index_id(line.trim()) {
            ids.push(session_id);
        }
    }

    if let Ok(position) = reader.stream_position() {
        *offset = position;
    }

    ids
}

fn parse_codex_index_id(line: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()?
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn resolve_codex_session_path(
    session_id: &str,
    sessions_root: &Path,
    capture_state: &mut CodexSessionCaptureState,
) -> Option<PathBuf> {
    if let Some(path) = capture_state.session_paths.get(session_id) {
        if path.exists() {
            return Some(path.clone());
        }
        capture_state.session_paths.remove(session_id);
    }

    let path = find_codex_session_file(sessions_root, session_id)?;
    capture_state
        .session_paths
        .insert(session_id.to_string(), path.clone());
    Some(path)
}

fn find_codex_session_file(root: &Path, session_id: &str) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return None;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_codex_session_file(&path, session_id) {
                return Some(found);
            }
            continue;
        }

        if path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(file_stem) = file_name.strip_suffix(".jsonl") else {
            continue;
        };
        if file_stem.ends_with(session_id) {
            return Some(path);
        }
    }

    None
}

fn parse_codex_session_candidate(
    path: &Path,
    launch_started_at: SystemTime,
) -> Option<CodexSessionCandidate> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified_at = metadata.modified().ok()?;
    if modified_at.duration_since(launch_started_at).is_err() {
        return None;
    }

    let contents = std::fs::read_to_string(path).ok()?;
    for line in contents.lines().take(20) {
        let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
        if value.get("type").and_then(serde_json::Value::as_str) != Some("session_meta") {
            continue;
        }
        let payload = value.get("payload")?;
        let session_id = payload.get("id")?.as_str()?.to_string();
        let cwd = PathBuf::from(payload.get("cwd")?.as_str()?);
        return Some(CodexSessionCandidate {
            session_id,
            cwd,
            modified_at,
        });
    }

    None
}

fn find_pi_resume_target(
    sessions_dir: &Path,
    cwd: &Path,
    launch_started_at: SystemTime,
) -> Option<ResumeTarget> {
    let candidates = read_pi_session_candidates(sessions_dir, launch_started_at);
    match_pi_resume_target(&candidates, cwd, launch_started_at)
}

fn match_pi_resume_target(
    candidates: &[PiSessionCandidate],
    cwd: &Path,
    launch_started_at: SystemTime,
) -> Option<ResumeTarget> {
    let mut matching = candidates
        .iter()
        .filter(|candidate| {
            candidate.cwd == cwd
                && candidate
                    .modified_at
                    .duration_since(launch_started_at)
                    .is_ok()
        })
        .collect::<Vec<_>>();
    matching.sort_by_key(|candidate| candidate.modified_at);

    matching
        .first()
        .map(|candidate| ResumeTarget::path(candidate.session_path.display().to_string()))
}

fn read_pi_session_candidates(
    sessions_dir: &Path,
    launch_started_at: SystemTime,
) -> Vec<PiSessionCandidate> {
    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|path| parse_pi_session_candidate(&path, launch_started_at))
        .collect()
}

fn parse_pi_session_candidate(
    path: &Path,
    launch_started_at: SystemTime,
) -> Option<PiSessionCandidate> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified_at = metadata.modified().ok()?;
    if modified_at.duration_since(launch_started_at).is_err() {
        return None;
    }

    let first_line = std::fs::read_to_string(path)
        .ok()?
        .lines()
        .next()?
        .to_string();
    let value = serde_json::from_str::<serde_json::Value>(&first_line).ok()?;
    if value.get("type").and_then(serde_json::Value::as_str) != Some("session") {
        return None;
    }

    Some(PiSessionCandidate {
        session_path: path.to_path_buf(),
        cwd: PathBuf::from(value.get("cwd")?.as_str()?),
        modified_at,
    })
}

fn cwd_slug(cwd: &Path) -> Option<String> {
    let parts = cwd
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            std::path::Component::Prefix(prefix) => {
                Some(prefix.as_os_str().to_string_lossy().into_owned())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return None;
    }

    Some(format!("--{}--", parts.join("-")))
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
                .filter(|tab| tab.launch_config.kind == TerminalLaunchKind::Agent)
                .map(|_| section_id.clone())
        })
    {
        return Some(section_id);
    }

    projects.first().and_then(|project| {
        project
            .branches
            .first()
            .map(|branch| SectionId::new(&project.id, &branch.name))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        choose_initial_section, cwd_slug, launch_requires_background_resume_capture,
        match_codex_resume_target, match_pi_resume_target, read_new_codex_index_ids,
        CodexSessionCandidate, CodexSessionCaptureState, PiSessionCandidate, SectionId,
        SectionState,
    };
    use crate::agents::{
        AgentProviderKind, ResumeTarget, TerminalLaunchConfig, TerminalLaunchKind,
    };
    use crate::project_store::{
        Branch, PersistedSectionState, PersistedTerminalTab, Project, ProjectSettings, Worktree,
    };
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, UNIX_EPOCH};

    fn shell_tab(id: usize, title: &str) -> PersistedTerminalTab {
        PersistedTerminalTab {
            id,
            title: title.to_string(),
            kind: TerminalLaunchKind::Shell,
            provider: None,
            launch_argv: Vec::new(),
            resume_target: None,
        }
    }

    fn sample_project(id: &str, branch_name: &str) -> Project {
        Project {
            id: id.to_string(),
            name: format!("Project {id}"),
            path: PathBuf::from(format!("/tmp/{id}")),
            settings: ProjectSettings::default(),
            worktrees: Vec::<Worktree>::new(),
            branches: vec![Branch {
                name: branch_name.to_string(),
                lines_added: 0,
                lines_removed: 0,
                ahead_count: 0,
                last_commit_relative: "now".to_string(),
                is_default: true,
                is_current: true,
            }],
            worktree_name: None,
            repo_common_dir: None,
        }
    }

    #[test]
    fn section_state_restores_active_tab_and_clamps_next_tab_id() {
        let state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: 99,
                next_tab_id: 1,
                cwd: None,
                tabs: vec![
                    shell_tab(0, "Terminal"),
                    PersistedTerminalTab {
                        id: 4,
                        title: "Codex".to_string(),
                        kind: TerminalLaunchKind::Agent,
                        provider: Some(AgentProviderKind::Codex),
                        launch_argv: vec!["codex".to_string()],
                        resume_target: Some(ResumeTarget::id("session-1")),
                    },
                ],
            },
            Some(PathBuf::from("/tmp/project")),
        );

        assert_eq!(state.active_tab, 1);
        assert_eq!(state.next_tab_id, 5);
        assert_eq!(state.cwd, Some(PathBuf::from("/tmp/project")));
    }

    #[test]
    fn section_state_add_tab_continues_after_restored_next_tab_id() {
        let mut state = SectionState::from_persisted(
            PersistedSectionState {
                active_tab_id: 0,
                next_tab_id: 7,
                cwd: Some(PathBuf::from("/tmp/project")),
                tabs: vec![PersistedTerminalTab {
                    id: 0,
                    title: "Pi".to_string(),
                    kind: TerminalLaunchKind::Agent,
                    provider: Some(AgentProviderKind::Pi),
                    launch_argv: vec!["pi".to_string()],
                    resume_target: Some(ResumeTarget::path("/tmp/pi-session.jsonl")),
                }],
            },
            None,
        );

        let id = state.add_tab();

        assert_eq!(id, 7);
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

        assert_eq!(id, 1);
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
                        active_tab_id: 0,
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
                        active_tab_id: 0,
                        next_tab_id: 1,
                        cwd: Some(project.path.clone()),
                        tabs: vec![PersistedTerminalTab {
                            id: 0,
                            title: "Codex".to_string(),
                            kind: TerminalLaunchKind::Agent,
                            provider: Some(AgentProviderKind::Codex),
                            launch_argv: vec!["codex".to_string()],
                            resume_target: Some(ResumeTarget::id("session-1")),
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
                        active_tab_id: 0,
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
                        active_tab_id: 0,
                        next_tab_id: 1,
                        cwd: Some(project.path.clone()),
                        tabs: vec![PersistedTerminalTab {
                            id: 0,
                            title: "Claude Code".to_string(),
                            kind: TerminalLaunchKind::Agent,
                            provider: Some(AgentProviderKind::ClaudeCode),
                            launch_argv: vec!["claude".to_string(), "--resume".to_string()],
                            resume_target: Some(ResumeTarget::id("session-1")),
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
    fn codex_matcher_prefers_indexed_new_session_for_cwd() {
        let cwd = Path::new("/tmp/project");
        let launch_started_at = UNIX_EPOCH + Duration::from_secs(100);
        let candidates = vec![
            CodexSessionCandidate {
                session_id: "old".to_string(),
                cwd: cwd.to_path_buf(),
                modified_at: UNIX_EPOCH + Duration::from_secs(90),
            },
            CodexSessionCandidate {
                session_id: "wrong-cwd".to_string(),
                cwd: PathBuf::from("/tmp/elsewhere"),
                modified_at: UNIX_EPOCH + Duration::from_secs(160),
            },
            CodexSessionCandidate {
                session_id: "wanted".to_string(),
                cwd: cwd.to_path_buf(),
                modified_at: UNIX_EPOCH + Duration::from_secs(170),
            },
        ];

        let target = match_codex_resume_target(
            &["old".to_string(), "wanted".to_string()],
            &candidates,
            cwd,
            launch_started_at,
        );

        assert_eq!(target, Some(ResumeTarget::id("wanted")));
    }

    #[test]
    fn codex_matcher_falls_back_to_newest_matching_session() {
        let cwd = Path::new("/tmp/project");
        let launch_started_at = UNIX_EPOCH + Duration::from_secs(100);
        let candidates = vec![
            CodexSessionCandidate {
                session_id: "older".to_string(),
                cwd: cwd.to_path_buf(),
                modified_at: UNIX_EPOCH + Duration::from_secs(120),
            },
            CodexSessionCandidate {
                session_id: "newest".to_string(),
                cwd: cwd.to_path_buf(),
                modified_at: UNIX_EPOCH + Duration::from_secs(180),
            },
        ];

        let target = match_codex_resume_target(&[], &candidates, cwd, launch_started_at);

        assert_eq!(target, Some(ResumeTarget::id("newest")));
    }

    #[test]
    fn pi_matcher_chooses_first_new_file_for_cwd() {
        let cwd = Path::new("/tmp/project");
        let launch_started_at = UNIX_EPOCH + Duration::from_secs(100);
        let candidates = vec![
            PiSessionCandidate {
                session_path: PathBuf::from("/tmp/late.jsonl"),
                cwd: cwd.to_path_buf(),
                modified_at: UNIX_EPOCH + Duration::from_secs(150),
            },
            PiSessionCandidate {
                session_path: PathBuf::from("/tmp/first.jsonl"),
                cwd: cwd.to_path_buf(),
                modified_at: UNIX_EPOCH + Duration::from_secs(110),
            },
        ];

        let target = match_pi_resume_target(&candidates, cwd, launch_started_at);

        assert_eq!(target, Some(ResumeTarget::path("/tmp/first.jsonl")));
    }

    #[test]
    fn fresh_discovery_based_launch_requires_background_capture() {
        assert!(launch_requires_background_resume_capture(
            &TerminalLaunchConfig::for_provider(AgentProviderKind::Codex)
        ));
        assert!(launch_requires_background_resume_capture(
            &TerminalLaunchConfig::for_provider(AgentProviderKind::Pi)
        ));
        assert!(!launch_requires_background_resume_capture(
            &TerminalLaunchConfig::for_provider(AgentProviderKind::ClaudeCode)
        ));
        assert!(!launch_requires_background_resume_capture(
            &TerminalLaunchConfig::for_provider(AgentProviderKind::Codex)
                .with_resume_target(ResumeTarget::id("session-1"))
        ));
    }

    #[test]
    fn codex_index_reader_only_returns_new_appended_ids() {
        let root = std::env::temp_dir().join(format!("another-one-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temp test directory should be created");
        let index_path = root.join("session_index.jsonl");
        let mut capture_state = CodexSessionCaptureState::default();

        fs::write(
            &index_path,
            concat!(
                "{\"id\":\"first\"}\n",
                "{\"broken\":true}\n",
                "{\"id\":\"second\"}\n"
            ),
        )
        .expect("initial index should be written");

        let first_read = read_new_codex_index_ids(&index_path, &mut capture_state);
        assert_eq!(first_read, vec!["first".to_string(), "second".to_string()]);

        let second_read = read_new_codex_index_ids(&index_path, &mut capture_state);
        assert!(second_read.is_empty());

        fs::write(
            &index_path,
            concat!(
                "{\"id\":\"first\"}\n",
                "{\"broken\":true}\n",
                "{\"id\":\"second\"}\n",
                "not-json\n",
                "{\"id\":\"third\"}\n"
            ),
        )
        .expect("appended index should be written");

        let third_read = read_new_codex_index_ids(&index_path, &mut capture_state);
        assert_eq!(third_read, vec!["third".to_string()]);

        fs::remove_dir_all(&root).expect("temp test directory should be removed");
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
                            let mut should_notify = this.flush_pending_terminal_resizes(cx);
                            should_notify |= this.should_notify_active_terminal(cx);
                            should_notify |= this.drain_git_action(cx);
                            should_notify |= this.drain_changed_files_git_mutations(cx);
                            should_notify |= this.drain_git_refresh();
                            should_notify |= this.drain_task_creation(cx);
                            should_notify |= this.drain_project_add(cx);
                            should_notify |= this.drain_terminal_metadata(cx);
                            should_notify |= this.drain_terminal_spawn(cx);
                            should_notify |= this.drain_project_github_link_lookup();
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
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_key_down(cx.listener(Self::handle_global_key_down))
                    .on_action(cx.listener(Self::zoom_in))
                    .on_action(cx.listener(Self::zoom_out))
                    .on_action(cx.listener(Self::zoom_reset))
                    .child(main)
                    .child(footer)
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
