//! Terminal target resolution helpers shared by the sidebar and the
//! terminal workspace.
//!
//! A `TerminalTarget` identifies which section + tab the Slint client
//! attaches to. The sidebar uses these helpers to resolve a clicked task
//! into a target; the terminal workspace uses them to decide which tab is
//! attachable when daemon snapshots refresh.
//!
//! GPUI source of truth: `desktop/src/panels.rs` `WorkspacePane`'s
//! per-section state plus `desktop/src/app.rs` task-launcher target
//! resolution. Slint mirrors the same identity (section_id + tab_id) so a
//! daemon refresh maps cleanly back to the active client tab.

use crate::frame;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalTarget {
    pub(crate) section_id: String,
    pub(crate) tab_id: String,
}

pub(crate) fn first_attachable_target(
    projects: &[frame::ProjectSummary],
) -> Option<TerminalTarget> {
    projects
        .iter()
        .find_map(|project| project.tasks.iter().find_map(target_for_task))
}

pub(crate) fn target_for_task_id(
    projects: &[frame::ProjectSummary],
    task_id: &str,
) -> Option<TerminalTarget> {
    projects
        .iter()
        .flat_map(|project| &project.tasks)
        .find(|task| task.id == task_id)
        .and_then(target_for_task)
}

pub(crate) fn target_for_tab_id(
    projects: &[frame::ProjectSummary],
    active_section_id: &str,
    tab_id: &str,
) -> Option<TerminalTarget> {
    projects
        .iter()
        .flat_map(|project| &project.tasks)
        .find(|task| {
            task.section_id == active_section_id && task.tabs.iter().any(|tab| tab.id == tab_id)
        })
        .or_else(|| {
            projects
                .iter()
                .flat_map(|project| &project.tasks)
                .find(|task| task.tabs.iter().any(|tab| tab.id == tab_id))
        })
        .map(|task| TerminalTarget {
            section_id: task.section_id.clone(),
            tab_id: tab_id.to_string(),
        })
}

pub(crate) fn target_still_exists(
    projects: &[frame::ProjectSummary],
    target: &TerminalTarget,
) -> bool {
    projects.iter().any(|project| {
        project.tasks.iter().any(|task| {
            task.section_id == target.section_id
                && task.tabs.iter().any(|tab| tab.id == target.tab_id)
        })
    })
}

pub(crate) fn project_id_for_target(
    projects: &[frame::ProjectSummary],
    target: &TerminalTarget,
) -> Option<String> {
    task_project_for_target(projects, target).map(|(project, task)| {
        if task.target_project_id.is_empty() {
            project.id.clone()
        } else {
            task.target_project_id.clone()
        }
    })
}

pub(crate) fn active_project_id_for_open_in(
    projects: &[frame::ProjectSummary],
    target: &TerminalTarget,
    selected_project_id: &str,
) -> Option<String> {
    let selected_project_id = selected_project_id.trim();
    if !selected_project_id.is_empty()
        && projects
            .iter()
            .any(|project| project.id == selected_project_id)
    {
        return Some(selected_project_id.to_string());
    }

    project_id_for_target(projects, target)
}

pub(crate) fn normalized_source_branch(
    projects: &[frame::ProjectSummary],
    target: &TerminalTarget,
    requested_branch: &str,
) -> Option<String> {
    let requested_branch = requested_branch.trim();
    if !requested_branch.is_empty() {
        return Some(requested_branch.to_string());
    }

    task_project_for_target(projects, target).and_then(|(project, task)| {
        if !task.branch_name.is_empty() {
            Some(task.branch_name.clone())
        } else {
            project.current_branch.clone()
        }
    })
}

pub(crate) fn task_project_for_target<'a>(
    projects: &'a [frame::ProjectSummary],
    target: &TerminalTarget,
) -> Option<(&'a frame::ProjectSummary, &'a frame::TaskSummary)> {
    projects.iter().find_map(|project| {
        project
            .tasks
            .iter()
            .find(|task| task.section_id == target.section_id)
            .map(|task| (project, task))
    })
}

pub(crate) fn target_for_task(task: &frame::TaskSummary) -> Option<TerminalTarget> {
    let tab = task
        .tabs
        .iter()
        .find(|tab| tab.id == task.active_tab_id)
        .or_else(|| task.tabs.first())?;
    Some(TerminalTarget {
        section_id: task.section_id.clone(),
        tab_id: tab.id.clone(),
    })
}
