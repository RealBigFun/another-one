//! Slint terminal panel + project-overview AppWindow mutators.
//!
//! GPUI source of truth: `desktop/src/panels.rs` (terminal renderer
//! surface application) and `desktop/src/project_page.rs` (project
//! overview header). This module owns the small `AppWindow` mutators
//! that publish terminal surface snapshots, terminal selection, terminal
//! status text, and project-overview placeholder labels — plus the
//! `TerminalSurface` POD that the AlacrittySnapshot in lib.rs hands to
//! both Slint event-loop schedulers and synchronous fixture callers.
//!
//! Phase A scope: AppWindow mutators only. The terminal renderer
//! itself (`spawn_terminal_worker`, `AlacrittySnapshot`, color
//! resolution) stays in lib.rs and will move with the bigger
//! `terminal_renderer.rs` extraction in a later slice.

use crate::util::{project_kind_label, worktree_name};
use crate::{
    frame, AppWindow, TerminalBackgroundSpan, TerminalCursorSpan, TerminalLinkSpan,
    TerminalSelectionSpan, TerminalTextRun,
};

#[derive(Default)]
pub(crate) struct TerminalSurface {
    pub(crate) text_runs: Vec<TerminalTextRun>,
    pub(crate) background_spans: Vec<TerminalBackgroundSpan>,
    pub(crate) cursor_spans: Vec<TerminalCursorSpan>,
    pub(crate) link_spans: Vec<TerminalLinkSpan>,
}

pub(crate) fn set_terminal_status(app_weak: &slint::Weak<AppWindow>, status: impl Into<String>) {
    let app_weak = app_weak.clone();
    let status = status.into();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_terminal_status(status.into());
        }
    });
}

pub(crate) fn set_project_overview_placeholder(
    app_weak: &slint::Weak<AppWindow>,
    project: &frame::ProjectSummary,
    github_url: &str,
) {
    let app_weak = app_weak.clone();
    let project_id = project.id.clone();
    let project_name = project.name.clone();
    let branch_name = project
        .current_branch
        .as_deref()
        .unwrap_or_else(|| project_kind_label(project.kind))
        .to_string();
    let worktree_name = worktree_name(&project.path);
    let project_path = project.path.clone();
    let github_url = github_url.to_string();
    let status = format!("project overview: {project_name} (Slint project page parity pending)");
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_active_project_id(project_id.into());
            app.set_active_project_name(project_name.into());
            app.set_active_task_name("Project overview".into());
            app.set_active_branch_name(branch_name.into());
            app.set_active_worktree_name(worktree_name.into());
            app.set_active_project_path(project_path.into());
            app.set_active_project_github_url(github_url.into());
            app.set_terminal_status(status.into());
        }
    });
}

pub(crate) fn set_terminal_surface(app_weak: &slint::Weak<AppWindow>, surface: TerminalSurface) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_terminal_background_spans(slint::ModelRc::new(slint::VecModel::from(
                surface.background_spans,
            )));
            app.set_terminal_cursor_spans(slint::ModelRc::new(slint::VecModel::from(
                surface.cursor_spans,
            )));
            app.set_terminal_link_spans(slint::ModelRc::new(slint::VecModel::from(
                surface.link_spans,
            )));
            app.set_terminal_runs(slint::ModelRc::new(slint::VecModel::from(
                surface.text_runs,
            )));
        }
    });
}

pub(crate) fn set_terminal_selection(
    app_weak: &slint::Weak<AppWindow>,
    spans: Vec<TerminalSelectionSpan>,
) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_terminal_selection_spans(slint::ModelRc::new(slint::VecModel::from(spans)));
        }
    });
}

pub(crate) fn apply_terminal_surface(app: &AppWindow, surface: TerminalSurface) {
    app.set_terminal_background_spans(slint::ModelRc::new(slint::VecModel::from(
        surface.background_spans,
    )));
    app.set_terminal_cursor_spans(slint::ModelRc::new(slint::VecModel::from(
        surface.cursor_spans,
    )));
    app.set_terminal_link_spans(slint::ModelRc::new(slint::VecModel::from(
        surface.link_spans,
    )));
    app.set_terminal_runs(slint::ModelRc::new(slint::VecModel::from(
        surface.text_runs,
    )));
}
