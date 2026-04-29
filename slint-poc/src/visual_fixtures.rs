//! Deterministic visual-state fixtures used for capture parity.
//!
//! GPUI source of truth: the visual fidelity gate at
//! `docs/architecture/slint-visual-fidelity-gate.md` and the per-surface
//! port reviews under `docs/architecture/reviews/`. Fixture states are
//! selected through the `ANOTHERONE_SLINT_VISUAL_STATE` environment
//! variable so screenshots can be deterministic in CI and local capture
//! runs.
//!
//! Phase A scope: covers settings, right-inspector, layout-collapsed,
//! new-task modal, toast-error, and component-state fixtures. The
//! terminal-fidelity fixture remains in lib.rs because it depends on
//! terminal renderer types that have not been extracted yet
//! (`AlacrittySnapshot`, `TerminalCellPoint`, `selection_spans_for_points`,
//! `apply_terminal_surface`, `TERMINAL_FIDELITY_FIXTURE`).

use std::collections::{HashMap, HashSet};

use crate::right_inspector::{
    right_inspector_commit_key, right_inspector_rows_for_checks,
    right_inspector_rows_for_commits_with_expansions, right_inspector_rows_for_compare,
    InspectorCommitFileChangesState,
};
use crate::util::project_accent_color;
use crate::workspace_shell::sidebar_task_menu_entries;
use crate::{frame, AppWindow, MenuEntry, ProjectSidebarEntry, SegmentEntry, TaskSidebarEntry};

pub(crate) fn seed_visual_state_fixture(app: &AppWindow) {
    let Ok(state) = std::env::var("ANOTHERONE_SLINT_VISUAL_STATE") else {
        return;
    };

    match state.as_str() {
        "new-task-modal" => {
            app.set_modal_open(true);
            app.set_new_task_name("Review GPUI parity".into());
            app.set_new_task_branch("slint-visual-fixture".into());
        }
        "toast-error" => {
            app.set_toast_kind("error".into());
            app.set_toast_message("Could not open terminal link".into());
            app.set_toast_detail(
                "https://example.test returned an unsupported platform action".into(),
            );
        }
        "layout-collapsed" => {
            app.set_left_sidebar_open(false);
            app.set_right_inspector_open(false);
            app.set_resource_popover_open(true);
        }
        "right-inspector-commits" => {
            seed_right_inspector_commits_fixture(app);
        }
        "right-inspector-checks" => {
            seed_right_inspector_checks_fixture(app);
        }
        "right-inspector-compare" => {
            seed_right_inspector_compare_fixture(app);
        }
        "settings-general" => {
            seed_settings_visual_fixture(app, "general", "Settings General visual gate")
        }
        "settings-agents" => {
            seed_settings_visual_fixture(app, "agents", "Settings Agents visual gate")
        }
        "settings-open-in" => {
            seed_settings_visual_fixture(app, "open-in", "Settings Open In visual gate")
        }
        "settings-git-actions" => {
            seed_settings_visual_fixture(app, "git-actions", "Settings Git Actions visual gate");
        }
        "settings-keybindings" => {
            seed_settings_visual_fixture(app, "keybindings", "Settings Keybindings visual gate");
        }
        "settings-mcp" => seed_settings_visual_fixture(app, "mcp", "Settings MCP visual gate"),
        _ => {}
    }
}

fn seed_settings_visual_fixture(app: &AppWindow, section: &str, task_name: &str) {
    app.set_settings_open(true);
    app.set_settings_active_section(section.into());
    app.set_active_project_name("settings-fixture".into());
    app.set_active_task_name(task_name.into());
    app.set_active_branch_name("slint-daemon-poc-clean".into());
    app.set_active_worktree_name("fixture-mode".into());
    app.set_project_summary("fixture".into());
    app.set_left_sidebar_open(true);
    app.set_right_inspector_open(false);
    app.set_toast_kind("info".into());
    app.set_toast_message("".into());
    app.set_toast_detail("".into());
}

fn seed_right_inspector_commits_fixture(app: &AppWindow) {
    let project_id = "fixture-project";
    let view = frame::RecentCommitsWire {
        current_branch: Some("slint-daemon-poc-clean".to_string()),
        has_more: true,
        commits: vec![
            frame::CommitWire {
                id: "7f7a8e697f9c4d4e".to_string(),
                short_id: "7f7a8e6".to_string(),
                subject: "fix(daemon): support slint terminal tab controls".to_string(),
                author_name: "Mason".to_string(),
                authored_relative: "9 minutes ago".to_string(),
            },
            frame::CommitWire {
                id: "2f0ad9e4a41f3d0c".to_string(),
                short_id: "2f0ad9e".to_string(),
                subject: "fix(slint): dial daemon ticket".to_string(),
                author_name: "Mason".to_string(),
                authored_relative: "34 minutes ago".to_string(),
            },
            frame::CommitWire {
                id: "99f77110f79c59ef".to_string(),
                short_id: "99f7711".to_string(),
                subject: "feat(slint): persist appearance preference".to_string(),
                author_name: "Mason".to_string(),
                authored_relative: "1 hour ago".to_string(),
            },
        ],
    };
    let expanded_key = right_inspector_commit_key(project_id, "7f7a8e697f9c4d4e");
    let expanded = HashSet::from([expanded_key.clone()]);
    let file_states = HashMap::from([(
        expanded_key,
        InspectorCommitFileChangesState::Loaded(vec![
            frame::BranchCompareFileWire {
                path: "desktop/src/daemon_host.rs".to_string(),
                original_path: None,
                status: "M".to_string(),
                additions: 186,
                deletions: 2,
            },
            frame::BranchCompareFileWire {
                path: "desktop/src/app.rs".to_string(),
                original_path: None,
                status: "M".to_string(),
                additions: 21,
                deletions: 0,
            },
        ]),
    )]);
    let rows = right_inspector_rows_for_commits_with_expansions(
        project_id,
        &view,
        &expanded,
        &file_states,
    );
    seed_right_inspector_fixture_shell(app, "Commits mode visual gate");
    app.set_right_inspector_mode("commits".into());
    app.set_right_inspector_state("dirty".into());
    app.set_right_inspector_title("Recent commits".into());
    app.set_right_inspector_summary("3 recent commits; more are available.".into());
    app.set_right_inspector_detail("Branch: slint-daemon-poc-clean".into());
    app.set_right_inspector_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
}

fn seed_right_inspector_checks_fixture(app: &AppWindow) {
    let project_id = "fixture-project";
    let checks = vec![
        frame::Check {
            name: "Build Linux x86_64".to_string(),
            state: "failure".to_string(),
            bucket: frame::CheckBucket::Fail,
            description: Some("cargo check --workspace failed on formatting gate".to_string()),
            link: Some("https://github.example/checks/build-linux".to_string()),
            duration_text: Some("2m 14s".to_string()),
        },
        frame::Check {
            name: "Unit Tests".to_string(),
            state: "pending".to_string(),
            bucket: frame::CheckBucket::Pending,
            description: Some("cargo test -p slint-poc --lib".to_string()),
            link: Some("https://github.example/checks/unit-tests".to_string()),
            duration_text: Some("running".to_string()),
        },
        frame::Check {
            name: "Visual Diff".to_string(),
            state: "skipping".to_string(),
            bucket: frame::CheckBucket::Skipping,
            description: Some("waiting for matched GPUI capture".to_string()),
            link: None,
            duration_text: Some("queued".to_string()),
        },
        frame::Check {
            name: "Rustfmt".to_string(),
            state: "success".to_string(),
            bucket: frame::CheckBucket::Pass,
            description: Some("cargo fmt --check".to_string()),
            link: Some("https://github.example/checks/rustfmt".to_string()),
            duration_text: Some("18s".to_string()),
        },
    ];
    let rows = right_inspector_rows_for_checks(project_id, &checks);
    seed_right_inspector_fixture_shell(app, "Checks mode visual gate");
    app.set_right_inspector_mode("checks".into());
    app.set_right_inspector_state("dirty".into());
    app.set_right_inspector_title("Pull request checks".into());
    app.set_right_inspector_summary("1 failing, 1 pending, 1 skipped, 1 passing.".into());
    app.set_right_inspector_detail("PR #97 · slint-daemon-poc-clean".into());
    app.set_right_inspector_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
}

fn seed_right_inspector_compare_fixture(app: &AppWindow) {
    let project_id = "fixture-project";
    let view = frame::BranchCompareWire {
        current_branch: Some("slint-daemon-poc-clean".to_string()),
        target_branch: "daemon-transport-foundation".to_string(),
        files: vec![
            frame::BranchCompareFileWire {
                path: "slint-poc/src/lib.rs".to_string(),
                original_path: None,
                status: "M".to_string(),
                additions: 84,
                deletions: 12,
            },
            frame::BranchCompareFileWire {
                path: "slint-poc/ui/app.slint".to_string(),
                original_path: None,
                status: "M".to_string(),
                additions: 42,
                deletions: 7,
            },
            frame::BranchCompareFileWire {
                path: "docs/architecture/slint-view-viewport-contracts.md".to_string(),
                original_path: Some("docs/architecture/viewports.md".to_string()),
                status: "R".to_string(),
                additions: 9,
                deletions: 3,
            },
        ],
    };
    let rows = right_inspector_rows_for_compare(project_id, &view);
    seed_right_inspector_fixture_shell(app, "Compare mode visual gate");
    app.set_right_inspector_compare_available(true);
    app.set_right_inspector_compare_target("daemon-transport-foundation".into());
    app.set_right_inspector_mode("compare".into());
    app.set_right_inspector_state("dirty".into());
    app.set_right_inspector_title("Branch compare".into());
    app.set_right_inspector_summary("3 files changed against daemon-transport-foundation.".into());
    app.set_right_inspector_detail("Read-only branch diff".into());
    app.set_right_inspector_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
}

fn seed_right_inspector_fixture_shell(app: &AppWindow, task_name: &str) {
    app.set_active_project_name("right-inspector-fixture".into());
    app.set_active_task_name(task_name.into());
    app.set_active_branch_name("slint-daemon-poc-clean".into());
    app.set_active_worktree_name("fixture-mode".into());
    app.set_project_summary("fixture".into());
    app.set_left_sidebar_open(true);
    app.set_right_inspector_open(true);
}

pub(crate) fn seed_component_state_fixture(app: &AppWindow) {
    app.set_component_fixture_mode(true);
    app.set_active_project_name("component-fixture".into());
    app.set_active_task_name("Base component states".into());
    app.set_active_branch_name("slint-component-fixture".into());
    app.set_active_worktree_name("fixture-mode".into());
    app.set_project_summary("fixture rows".into());
    app.set_project_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        ProjectSidebarEntry {
            id: "project-active".into(),
            name: "active project".into(),
            path: "~/another-one".into(),
            branch: "slint-daemon-poc-clean".into(),
            initials: "A".into(),
            accent: project_accent_color("project-active"),
            active: true,
            loading: false,
            error: false,
            expanded: true,
            task_count_label: "5".into(),
        },
        ProjectSidebarEntry {
            id: "project-loading".into(),
            name: "loading project".into(),
            path: "~/daemon".into(),
            branch: "waiting".into(),
            initials: "L".into(),
            accent: project_accent_color("project-loading"),
            active: false,
            loading: true,
            error: false,
            expanded: true,
            task_count_label: "...".into(),
        },
        ProjectSidebarEntry {
            id: "project-error".into(),
            name: "errored project".into(),
            path: "~/missing".into(),
            branch: "unavailable".into(),
            initials: "E".into(),
            accent: project_accent_color("project-error"),
            active: false,
            loading: false,
            error: true,
            expanded: false,
            task_count_label: "0".into(),
        },
    ])));
    app.set_task_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        TaskSidebarEntry {
            id: "task-active".into(),
            title: "Active running task".into(),
            branch: "feature/slint".into(),
            metadata: "active | +12 -3".into(),
            initials: "R".into(),
            accent: project_accent_color("task-active"),
            active: true,
            pinned: true,
            running: true,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        TaskSidebarEntry {
            id: "task-editing".into(),
            title: "Rename in progress".into(),
            branch: "feature/edit".into(),
            metadata: "editing".into(),
            initials: "E".into(),
            accent: project_accent_color("task-editing"),
            active: false,
            pinned: false,
            running: false,
            loading: false,
            error: false,
            editing: true,
            delete_confirm: false,
        },
        TaskSidebarEntry {
            id: "task-loading".into(),
            title: "Loading daemon state".into(),
            branch: "feature/loading".into(),
            metadata: "loading".into(),
            initials: "L".into(),
            accent: project_accent_color("task-loading"),
            active: false,
            pinned: false,
            running: false,
            loading: true,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        TaskSidebarEntry {
            id: "task-delete".into(),
            title: "Delete confirmation".into(),
            branch: "feature/delete".into(),
            metadata: "confirm delete".into(),
            initials: "D".into(),
            accent: project_accent_color("task-delete"),
            active: false,
            pinned: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: true,
        },
        TaskSidebarEntry {
            id: "task-error".into(),
            title: "Errored task".into(),
            branch: "feature/error".into(),
            metadata: "failed".into(),
            initials: "X".into(),
            accent: project_accent_color("task-error"),
            active: false,
            pinned: false,
            running: false,
            loading: false,
            error: true,
            editing: false,
            delete_confirm: false,
        },
    ])));
    app.set_fixture_segments(slint::ModelRc::new(slint::VecModel::from(vec![
        SegmentEntry {
            id: "light".into(),
            label: "Light".into(),
            selected: false,
            disabled: false,
        },
        SegmentEntry {
            id: "dark".into(),
            label: "Dark".into(),
            selected: true,
            disabled: false,
        },
        SegmentEntry {
            id: "system".into(),
            label: "System".into(),
            selected: false,
            disabled: true,
        },
    ])));
    app.set_fixture_menu_entries(slint::ModelRc::new(slint::VecModel::from(vec![
        MenuEntry {
            id: "open".into(),
            label: "Open task".into(),
            shortcut: "Enter".into(),
            selected: false,
            disabled: false,
            destructive: false,
        },
        MenuEntry {
            id: "pin".into(),
            label: "Pin task".into(),
            shortcut: "P".into(),
            selected: true,
            disabled: false,
            destructive: false,
        },
        MenuEntry {
            id: "disabled".into(),
            label: "Unavailable".into(),
            shortcut: "".into(),
            selected: false,
            disabled: true,
            destructive: false,
        },
        MenuEntry {
            id: "delete".into(),
            label: "Delete task".into(),
            shortcut: "Del".into(),
            selected: false,
            disabled: false,
            destructive: true,
        },
    ])));
    app.set_titlebar_open_in_entries(slint::ModelRc::new(slint::VecModel::from(vec![
        MenuEntry {
            id: "__open-in-loading".into(),
            label: "Loading apps".into(),
            shortcut: "".into(),
            selected: false,
            disabled: true,
            destructive: false,
        },
    ])));
    app.set_sidebar_project_menu_entries(slint::ModelRc::new(slint::VecModel::from(vec![
        MenuEntry {
            id: "sort-recent".into(),
            label: "Recent activity".into(),
            shortcut: "".into(),
            selected: true,
            disabled: false,
            destructive: false,
        },
        MenuEntry {
            id: "sort-most".into(),
            label: "Most activity".into(),
            shortcut: "".into(),
            selected: false,
            disabled: false,
            destructive: false,
        },
        MenuEntry {
            id: "sort-manual".into(),
            label: "Manual".into(),
            shortcut: "".into(),
            selected: false,
            disabled: false,
            destructive: false,
        },
    ])));
    app.set_sidebar_task_pin_menu_entries(slint::ModelRc::new(slint::VecModel::from(
        sidebar_task_menu_entries(false),
    )));
    app.set_sidebar_task_unpin_menu_entries(slint::ModelRc::new(slint::VecModel::from(
        sidebar_task_menu_entries(true),
    )));
}

#[cfg(test)]
mod tests {
    /// Pin Slint visual fixture state names to the GPUI visual fidelity gate
    /// "Screenshot Pair Naming" list. If the doc renames or drops a state
    /// this module covers, the test fails so the fixture stays aligned with
    /// the corpus pair.
    #[test]
    fn slint_visual_fixtures_match_visual_fidelity_gate_pair_names() {
        let gate = include_str!("../../docs/architecture/slint-visual-fidelity-gate.md");
        for name in [
            "modal/new-task",
            "toast/error",
            "right-sidebar/commits",
            "settings/agents",
        ] {
            assert!(
                gate.contains(name),
                "Visual fidelity gate no longer references pair name: {name}"
            );
        }
    }
}
