//! Slint resource indicator + popover plumbing.
//!
//! GPUI source of truth: `desktop/src/resource_indicator.rs` and
//! `core/src/resource_usage.rs`. The daemon publishes
//! `frame::ResourceUsageSnapshotWire` snapshots; this module shapes them
//! into the AppWindow properties the resource indicator and popover
//! consume.
//!
//! Phase A scope: `set_resource_usage` (AppWindow mutator) plus the two
//! pure data shapers (`resource_sessions_summary`, `resource_usage_rows`).
//! Project/task/session collapse persistence and outside-click parity
//! remain TODO under bd `another-one-y4n.8`.
//!
//! Port-review reference:
//! `docs/architecture/reviews/slint-overlays-port-review.md` (resource
//! popover section).

use crate::{frame, AppWindow, ResourceUsageEntry};

pub(crate) fn set_resource_usage(
    app_weak: &slint::Weak<AppWindow>,
    snapshot: frame::ResourceUsageSnapshotWire,
) {
    let app_cpu = format!("{:.1}%", snapshot.app.cpu_percent);
    let app_memory = another_one_core::resource_usage::format_memory(snapshot.app.memory_bytes);
    let summary = format!("app {app_cpu} / {app_memory}");
    let session_count = snapshot.session_count.to_string();
    let (sessions_state, sessions_summary) = resource_sessions_summary(&snapshot);
    let rows = resource_usage_rows(&snapshot);
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_resource_summary(summary.into());
            app.set_resource_app_cpu_label(app_cpu.into());
            app.set_resource_app_memory_label(app_memory.into());
            app.set_resource_session_count_label(session_count.into());
            app.set_resource_sessions_state(sessions_state.into());
            app.set_resource_sessions_summary(sessions_summary.into());
            app.set_resource_session_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
        }
    });
}

pub(crate) fn resource_sessions_summary(
    snapshot: &frame::ResourceUsageSnapshotWire,
) -> (String, String) {
    if snapshot.session_count == 0 || snapshot.projects.is_empty() {
        return (
            "empty".to_string(),
            "No active terminal sessions".to_string(),
        );
    }

    let cpu = format!(
        "{:.1}%",
        snapshot.total_cpu_percent - snapshot.app.cpu_percent
    );
    let memory = another_one_core::resource_usage::format_memory(
        snapshot
            .total_memory_bytes
            .saturating_sub(snapshot.app.memory_bytes),
    );
    (
        "dirty".to_string(),
        format!(
            "{} active terminal sessions across {} projects · {cpu} / {memory}",
            snapshot.session_count,
            snapshot.projects.len()
        ),
    )
}

pub(crate) fn resource_usage_rows(
    snapshot: &frame::ResourceUsageSnapshotWire,
) -> Vec<ResourceUsageEntry> {
    let mut rows = Vec::new();
    for project in &snapshot.projects {
        rows.push(ResourceUsageEntry {
            kind: "project".into(),
            label: project.label.clone().into(),
            detail: "".into(),
            cpu_label: format!("{:.1}%", project.cpu_percent).into(),
            memory_label: another_one_core::resource_usage::format_memory(project.memory_bytes)
                .into(),
            indent: 0,
        });
        for task in &project.tasks {
            rows.push(ResourceUsageEntry {
                kind: "task".into(),
                label: task.label.clone().into(),
                detail: "".into(),
                cpu_label: format!("{:.1}%", task.cpu_percent).into(),
                memory_label: another_one_core::resource_usage::format_memory(task.memory_bytes)
                    .into(),
                indent: 1,
            });
            for session in &task.sessions {
                rows.push(ResourceUsageEntry {
                    kind: "session".into(),
                    label: session.label.clone().into(),
                    detail: "".into(),
                    cpu_label: format!("{:.1}%", session.cpu_percent).into(),
                    memory_label: another_one_core::resource_usage::format_memory(
                        session.memory_bytes,
                    )
                    .into(),
                    indent: 2,
                });
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the Slint resource module to the GPUI resource indicator and the
    /// shared core sampler so renames force a Slint review.
    #[test]
    fn slint_resource_module_pins_to_gpui_and_core_symbols() {
        let gpui = include_str!("../../desktop/src/resource_indicator.rs");
        assert!(
            gpui.contains("resource_indicator_overlay"),
            "GPUI resource overlay symbol missing or renamed"
        );
        let core = include_str!("../../core/src/resource_usage.rs");
        assert!(
            core.contains("fn format_memory"),
            "core resource_usage::format_memory missing or renamed"
        );
    }

    #[test]
    fn resource_sessions_summary_handles_empty_snapshot() {
        let snapshot = frame::ResourceUsageSnapshotWire::default();
        let (state, summary) = resource_sessions_summary(&snapshot);
        assert_eq!(state, "empty");
        assert!(summary.contains("No active terminal sessions"));
    }
}
