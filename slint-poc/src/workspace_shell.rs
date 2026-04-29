//! Slint workspace shell projection.
//!
//! GPUI source of truth: `desktop/src/left_sidebar.rs` (project tree),
//! `desktop/src/panels.rs` (terminal panel), and `desktop/src/app.rs`
//! (active project/task/branch labels). This module owns the unified
//! Slint shell projection — sidebar rows, project rows, task rows, tab
//! chips, the terminal panel state card, and the active project/task
//! labels — so they all derive from the same `frame::ProjectSummary`
//! snapshot.
//!
//! Phase A scope: the `WorkspaceShellModel` struct, the
//! `set_workspace_tree` AppWindow mutator, the `workspace_shell_model`
//! projection function, and the supporting `TerminalPanelModel`
//! projection plus pure helpers (`active_tab_for_task`,
//! `sidebar_tree_rows`). Sidebar callbacks (rename, menu actions,
//! footer add-project) remain in lib.rs because they couple to daemon
//! dispatch; they extract together with the rest of the sidebar
//! callbacks in a later slice.
//!
//! Port-review references:
//! - `docs/architecture/reviews/slint-sidebar-port-review.md`
//! - `docs/architecture/reviews/slint-terminal-workspace-port-review.md`

use std::collections::{HashMap, HashSet};

use crate::util::{
    compact_path, initials, project_accent_color, project_kind_label, provider_label,
    restore_status_label, task_metadata, worktree_name,
};
use crate::{
    frame, AppWindow, MenuEntry, ProjectSidebarEntry, SidebarTreeEntry, TaskSidebarEntry,
    TerminalTabChip,
};

const SIDEBAR_TREE_TOP: i32 = 40;
const SIDEBAR_PROJECT_ROW_HEIGHT: i32 = 36;
const SIDEBAR_TASK_ROW_HEIGHT: i32 = 46;

pub(crate) type ProjectGithubUrls = HashMap<String, Option<String>>;

#[derive(Default)]
pub(crate) struct WorkspaceShellModel {
    pub(crate) sidebar_rows: Vec<SidebarTreeEntry>,
    pub(crate) project_rows: Vec<ProjectSidebarEntry>,
    pub(crate) task_rows: Vec<TaskSidebarEntry>,
    pub(crate) tab_chips: Vec<TerminalTabChip>,
    pub(crate) active_project_id: String,
    pub(crate) active_project_name: String,
    pub(crate) active_task_name: String,
    pub(crate) active_branch_name: String,
    pub(crate) active_worktree_name: String,
    pub(crate) active_project_path: String,
    pub(crate) active_project_github_url: String,
    pub(crate) terminal_panel_state: String,
    pub(crate) terminal_panel_title: String,
    pub(crate) terminal_panel_body: String,
    pub(crate) terminal_panel_project: String,
    pub(crate) terminal_panel_branch: String,
    pub(crate) terminal_panel_task: String,
    pub(crate) terminal_panel_tab: String,
    pub(crate) terminal_panel_cwd: String,
    pub(crate) terminal_error_details: String,
    pub(crate) project_summary: String,
}

pub(crate) fn set_workspace_tree(
    app_weak: &slint::Weak<AppWindow>,
    projects: &[frame::ProjectSummary],
    active_section_id: &str,
    active_tab_id: &str,
    project_github_urls: &ProjectGithubUrls,
    collapsed_project_groups: &HashSet<String>,
) {
    let model = workspace_shell_model(
        projects,
        active_section_id,
        active_tab_id,
        project_github_urls,
        collapsed_project_groups,
    );
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_sidebar_rows(slint::ModelRc::new(slint::VecModel::from(
                model.sidebar_rows,
            )));
            app.set_project_rows(slint::ModelRc::new(slint::VecModel::from(
                model.project_rows,
            )));
            app.set_task_rows(slint::ModelRc::new(slint::VecModel::from(model.task_rows)));
            app.set_tab_chips(slint::ModelRc::new(slint::VecModel::from(model.tab_chips)));
            app.set_active_project_id(model.active_project_id.into());
            app.set_active_project_name(model.active_project_name.into());
            app.set_active_task_name(model.active_task_name.into());
            app.set_active_branch_name(model.active_branch_name.into());
            app.set_active_worktree_name(model.active_worktree_name.into());
            app.set_active_project_path(model.active_project_path.into());
            app.set_active_project_github_url(model.active_project_github_url.into());
            app.set_terminal_panel_state(model.terminal_panel_state.into());
            app.set_terminal_panel_title(model.terminal_panel_title.into());
            app.set_terminal_panel_body(model.terminal_panel_body.into());
            app.set_terminal_panel_project(model.terminal_panel_project.into());
            app.set_terminal_panel_branch(model.terminal_panel_branch.into());
            app.set_terminal_panel_task(model.terminal_panel_task.into());
            app.set_terminal_panel_tab(model.terminal_panel_tab.into());
            app.set_terminal_panel_cwd(model.terminal_panel_cwd.into());
            app.set_terminal_error_details(model.terminal_error_details.into());
            app.set_project_summary(model.project_summary.into());
        }
    });
}

pub(crate) fn workspace_shell_model(
    projects: &[frame::ProjectSummary],
    active_section_id: &str,
    active_tab_id: &str,
    project_github_urls: &ProjectGithubUrls,
    collapsed_project_groups: &HashSet<String>,
) -> WorkspaceShellModel {
    let active_project = projects
        .iter()
        .find(|project| {
            project
                .tasks
                .iter()
                .any(|task| task.section_id == active_section_id)
        })
        .or_else(|| projects.first());
    let active_task = active_project.and_then(|project| {
        project
            .tasks
            .iter()
            .find(|task| task.section_id == active_section_id)
            .or_else(|| project.tasks.first())
    });

    let project_rows = projects
        .iter()
        .take(3)
        .map(|project| {
            let active = project
                .tasks
                .iter()
                .any(|task| task.section_id == active_section_id);
            ProjectSidebarEntry {
                id: project.id.clone().into(),
                name: project.name.clone().into(),
                path: compact_path(&project.path).into(),
                branch: project
                    .current_branch
                    .as_deref()
                    .unwrap_or_else(|| project_kind_label(project.kind))
                    .into(),
                initials: initials(&project.name).into(),
                accent: project_accent_color(&project.id),
                active,
                loading: false,
                error: false,
                expanded: active,
                task_count_label: project.tasks.len().to_string().into(),
            }
        })
        .collect::<Vec<_>>();
    let sidebar_rows = sidebar_tree_rows(
        projects,
        active_section_id,
        project_github_urls,
        collapsed_project_groups,
    );

    let mut task_entries = projects
        .iter()
        .flat_map(|project| {
            project.tasks.iter().map(move |task| {
                let running = task.tabs.iter().any(|tab| tab.running);
                TaskSidebarEntry {
                    id: task.id.clone().into(),
                    title: task.name.clone().into(),
                    branch: task.branch_name.clone().into(),
                    metadata: task_metadata(task).into(),
                    initials: initials(&task.name).into(),
                    accent: project_accent_color(&project.id),
                    active: task.section_id == active_section_id,
                    pinned: task.pinned,
                    running,
                    loading: false,
                    error: false,
                    editing: false,
                    delete_confirm: false,
                }
            })
        })
        .collect::<Vec<_>>();
    task_entries.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| right.pinned.cmp(&left.pinned))
            .then_with(|| left.title.cmp(&right.title))
    });
    task_entries.truncate(7);

    let tab_chips = active_task
        .map(|task| {
            task.tabs
                .iter()
                .take(5)
                .map(|tab| TerminalTabChip {
                    id: tab.id.clone().into(),
                    title: tab
                        .fixed_title
                        .as_deref()
                        .unwrap_or(tab.title.as_str())
                        .to_string()
                        .into(),
                    provider: tab.provider.map(provider_label).unwrap_or("shell").into(),
                    restore_status: restore_status_label(&tab.restore_status).into(),
                    failure_message: tab.failure_message.clone().unwrap_or_default().into(),
                    failure_details: tab.failure_details.clone().unwrap_or_default().into(),
                    active: tab.id == active_tab_id,
                    running: tab.running,
                    pinned: tab.pinned,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let active_tab = active_task.and_then(|task| active_tab_for_task(task, active_tab_id));
    let terminal_panel = terminal_panel_model(active_project, active_task, active_tab);

    WorkspaceShellModel {
        sidebar_rows,
        project_rows,
        task_rows: task_entries,
        tab_chips,
        active_project_id: active_project
            .map(|project| project.id.clone())
            .unwrap_or_default(),
        active_project_name: active_project
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "another-one".to_string()),
        active_task_name: active_task
            .map(|task| task.name.clone())
            .unwrap_or_else(|| "No active task".to_string()),
        active_branch_name: active_task
            .map(|task| task.branch_name.clone())
            .or_else(|| active_project.and_then(|project| project.current_branch.clone()))
            .unwrap_or_else(|| "detached".to_string()),
        active_worktree_name: active_project
            .map(|project| worktree_name(&project.path))
            .unwrap_or_else(|| "workspace".to_string()),
        active_project_path: active_project
            .map(|project| project.path.clone())
            .unwrap_or_default(),
        active_project_github_url: active_project
            .and_then(|project| project_github_urls.get(&project.id))
            .and_then(|url| url.clone())
            .unwrap_or_default(),
        terminal_panel_state: terminal_panel.state,
        terminal_panel_title: terminal_panel.title,
        terminal_panel_body: terminal_panel.body,
        terminal_panel_project: terminal_panel.project,
        terminal_panel_branch: terminal_panel.branch,
        terminal_panel_task: terminal_panel.task,
        terminal_panel_tab: terminal_panel.tab,
        terminal_panel_cwd: terminal_panel.cwd,
        terminal_error_details: terminal_panel.error_details,
        project_summary: String::new(),
    }
}

struct TerminalPanelModel {
    state: String,
    title: String,
    body: String,
    project: String,
    branch: String,
    task: String,
    tab: String,
    cwd: String,
    error_details: String,
}

fn terminal_panel_model(
    project: Option<&frame::ProjectSummary>,
    task: Option<&frame::TaskSummary>,
    tab: Option<&frame::TabSummary>,
) -> TerminalPanelModel {
    let project_label = project
        .map(|project| project.id.clone())
        .unwrap_or_else(|| "Not available".to_string());
    let branch_label = task
        .map(|task| task.branch_name.clone())
        .or_else(|| project.and_then(|project| project.current_branch.clone()))
        .unwrap_or_else(|| "Not available".to_string());
    let task_label = task
        .map(|task| task.id.clone())
        .unwrap_or_else(|| "Not available".to_string());
    let tab_label = tab
        .map(|tab| tab.id.clone())
        .unwrap_or_else(|| "Not available".to_string());
    let cwd_label = project
        .map(|project| project.path.clone())
        .unwrap_or_else(|| "Not available".to_string());

    let (state, title, body, error_details) = match (task, tab) {
        (None, _) => (
            "empty",
            "Select a branch to get started",
            "Open a task from the project tree to attach a terminal.",
            "",
        ),
        (Some(_), None) => (
            "empty",
            "No active tabs",
            "This task has no open tabs. Add an agent tab to start working.",
            "",
        ),
        (Some(_), Some(tab)) => match restore_status_label(&tab.restore_status) {
            "launching" => (
                "launching",
                "Launching terminal",
                "The tab was created immediately and its PTY is launching in the background.",
                "",
            ),
            "failed" => (
                "failed",
                "Terminal launch failed",
                tab.failure_message
                    .as_deref()
                    .unwrap_or("The daemon reported a terminal launch failure."),
                tab.failure_details.as_deref().unwrap_or_default(),
            ),
            "not-started" => (
                "lazy",
                "Lazy restore",
                "This restored tab has metadata only. Opening it triggers launch or resume on demand.",
                "",
            ),
            _ => ("ready", "", "", ""),
        },
    };

    TerminalPanelModel {
        state: state.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        project: project_label,
        branch: branch_label,
        task: task_label,
        tab: tab_label,
        cwd: cwd_label,
        error_details: error_details.to_string(),
    }
}

pub(crate) fn sidebar_task_menu_entries(pinned: bool) -> Vec<MenuEntry> {
    vec![
        MenuEntry {
            id: if pinned { "unpin" } else { "pin" }.into(),
            label: if pinned { "Unpin" } else { "Pin" }.into(),
            shortcut: "".into(),
            selected: pinned,
            disabled: false,
            destructive: false,
        },
        MenuEntry {
            id: "new-task-from-branch".into(),
            label: "New task from current branch".into(),
            shortcut: "".into(),
            selected: false,
            disabled: false,
            destructive: false,
        },
        MenuEntry {
            id: "rename".into(),
            label: "Rename".into(),
            shortcut: "".into(),
            selected: false,
            disabled: false,
            destructive: false,
        },
        MenuEntry {
            id: "delete".into(),
            label: "Delete".into(),
            shortcut: "".into(),
            selected: false,
            disabled: false,
            destructive: true,
        },
    ]
}

pub(crate) fn active_tab_for_task<'a>(
    task: &'a frame::TaskSummary,
    active_tab_id: &str,
) -> Option<&'a frame::TabSummary> {
    task.tabs
        .iter()
        .find(|tab| tab.id == active_tab_id)
        .or_else(|| task.tabs.iter().find(|tab| tab.id == task.active_tab_id))
        .or_else(|| task.tabs.first())
}

pub(crate) fn sidebar_tree_rows(
    projects: &[frame::ProjectSummary],
    active_section_id: &str,
    project_github_urls: &ProjectGithubUrls,
    collapsed_project_groups: &HashSet<String>,
) -> Vec<SidebarTreeEntry> {
    let mut rows = Vec::new();
    let mut row_y = SIDEBAR_TREE_TOP;

    for project in projects {
        let mut tasks = project.tasks.iter().collect::<Vec<_>>();
        tasks.sort_by(|left, right| {
            right
                .pinned
                .cmp(&left.pinned)
                .then_with(|| left.name.cmp(&right.name))
        });
        let has_children = !tasks.is_empty();
        let expanded = has_children && !collapsed_project_groups.contains(&project.id);

        rows.push(SidebarTreeEntry {
            kind: "project".into(),
            id: format!("project:{}", project.id).into(),
            group_id: project.id.clone().into(),
            project_id: project.id.clone().into(),
            task_id: "".into(),
            row_y,
            row_height: SIDEBAR_PROJECT_ROW_HEIGHT,
            name: project.name.clone().into(),
            branch: project
                .current_branch
                .as_deref()
                .unwrap_or_else(|| project_kind_label(project.kind))
                .into(),
            metadata: "".into(),
            path: compact_path(&project.path).into(),
            github_url: project_github_urls
                .get(&project.id)
                .and_then(|url| url.as_deref())
                .unwrap_or_default()
                .into(),
            initials: initials(&project.name).into(),
            accent: project_accent_color(&project.id),
            active: false,
            expanded,
            has_children,
            task_count_label: tasks.len().to_string().into(),
            pinned: false,
            worktree: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        });
        row_y += SIDEBAR_PROJECT_ROW_HEIGHT;

        if expanded {
            for task in tasks {
                let running = task.tabs.iter().any(|tab| tab.running);
                let worktree =
                    !task.target_project_id.is_empty() && task.target_project_id != project.id;
                rows.push(SidebarTreeEntry {
                    kind: "task".into(),
                    id: format!("task:{}", task.id).into(),
                    group_id: project.id.clone().into(),
                    project_id: project.id.clone().into(),
                    task_id: task.id.clone().into(),
                    row_y,
                    row_height: SIDEBAR_TASK_ROW_HEIGHT,
                    name: task.name.clone().into(),
                    branch: task.branch_name.clone().into(),
                    metadata: task_metadata(task).into(),
                    path: "".into(),
                    github_url: "".into(),
                    initials: initials(&task.name).into(),
                    accent: project_accent_color(&project.id),
                    active: task.section_id == active_section_id,
                    expanded: false,
                    has_children: false,
                    task_count_label: "".into(),
                    pinned: task.pinned,
                    worktree,
                    running,
                    loading: false,
                    error: false,
                    editing: false,
                    delete_confirm: false,
                });
                row_y += SIDEBAR_TASK_ROW_HEIGHT;
            }
        }
    }

    rows
}
