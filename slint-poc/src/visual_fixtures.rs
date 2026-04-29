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
use crate::{
    frame, AppWindow, MenuEntry, ProjectSidebarEntry, SegmentEntry, SidebarTreeEntry,
    TaskSidebarEntry, TerminalTabChip,
};

/// Seed the default Slint shell model with sample projects/tasks/tabs so the
/// app renders something coherent before the daemon publishes a real
/// `ProjectSummary` snapshot. This is the GPUI-equivalent of the desktop app's
/// initial empty-state copy; it gets replaced as soon as the worker thread
/// emits its first `set_workspace_tree` call.
pub(crate) fn seed_shell_model(app: &AppWindow) {
    app.set_sidebar_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        SidebarTreeEntry {
            kind: "project".into(),
            id: "project:another-one".into(),
            group_id: "another-one".into(),
            project_id: "another-one".into(),
            task_id: "".into(),
            row_y: 40,
            row_height: 36,
            name: "another-one".into(),
            branch: "slint-daemon-poc-clean".into(),
            metadata: "".into(),
            path: "~/.another-one/worktrees/another-one".into(),
            github_url: "https://github.com/RealBigFun/another-one".into(),
            initials: "A".into(),
            accent: project_accent_color("another-one"),
            active: false,
            expanded: true,
            has_children: true,
            task_count_label: "3".into(),
            pinned: false,
            worktree: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        SidebarTreeEntry {
            kind: "task".into(),
            id: "task:slint-build".into(),
            group_id: "another-one".into(),
            project_id: "another-one".into(),
            task_id: "slint-build".into(),
            row_y: 76,
            row_height: 46,
            name: "Slint build".into(),
            branch: "slint-daemon-poc-clean".into(),
            metadata: "active | +0 -0".into(),
            path: "".into(),
            github_url: "".into(),
            initials: "S".into(),
            accent: project_accent_color("slint-build"),
            active: true,
            expanded: false,
            has_children: false,
            task_count_label: "".into(),
            pinned: true,
            worktree: true,
            running: true,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        SidebarTreeEntry {
            kind: "task".into(),
            id: "task:terminal-ready".into(),
            group_id: "another-one".into(),
            project_id: "another-one".into(),
            task_id: "terminal-ready".into(),
            row_y: 122,
            row_height: 46,
            name: "Terminal readiness".into(),
            branch: "terminal-production".into(),
            metadata: "in progress | renderer".into(),
            path: "".into(),
            github_url: "".into(),
            initials: "T".into(),
            accent: project_accent_color("terminal-ready"),
            active: false,
            expanded: false,
            has_children: false,
            task_count_label: "".into(),
            pinned: false,
            worktree: true,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        SidebarTreeEntry {
            kind: "task".into(),
            id: "task:style-system".into(),
            group_id: "another-one".into(),
            project_id: "another-one".into(),
            task_id: "style-system".into(),
            row_y: 168,
            row_height: 46,
            name: "Style system".into(),
            branch: "gpui baseline".into(),
            metadata: "blocked | visual corpus".into(),
            path: "".into(),
            github_url: "".into(),
            initials: "G".into(),
            accent: project_accent_color("style-system"),
            active: false,
            expanded: false,
            has_children: false,
            task_count_label: "".into(),
            pinned: false,
            worktree: true,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        SidebarTreeEntry {
            kind: "project".into(),
            id: "project:daemon-sandbox".into(),
            group_id: "daemon-sandbox".into(),
            project_id: "daemon-sandbox".into(),
            task_id: "".into(),
            row_y: 214,
            row_height: 36,
            name: "daemon-sandbox".into(),
            branch: "daemon transport".into(),
            metadata: "".into(),
            path: "daemon-sandbox".into(),
            github_url: "https://github.com/RealBigFun/another-one".into(),
            initials: "D".into(),
            accent: project_accent_color("daemon-sandbox"),
            active: false,
            expanded: true,
            has_children: false,
            task_count_label: "0".into(),
            pinned: false,
            worktree: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        SidebarTreeEntry {
            kind: "project".into(),
            id: "project:slint-platform".into(),
            group_id: "slint-platform".into(),
            project_id: "slint-platform".into(),
            task_id: "".into(),
            row_y: 250,
            row_height: 36,
            name: "slint-platform".into(),
            branch: "platform traits".into(),
            metadata: "".into(),
            path: "slint-platform".into(),
            github_url: "".into(),
            initials: "S".into(),
            accent: project_accent_color("slint-platform"),
            active: false,
            expanded: false,
            has_children: true,
            task_count_label: "2".into(),
            pinned: false,
            worktree: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
    ])));
    app.set_project_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        ProjectSidebarEntry {
            id: "another-one".into(),
            name: "another-one".into(),
            path: "~/.another-one/worktrees/another-one".into(),
            branch: "slint-daemon-poc-clean".into(),
            initials: "A".into(),
            accent: project_accent_color("another-one"),
            active: true,
            loading: false,
            error: false,
            expanded: true,
            task_count_label: "3".into(),
        },
        ProjectSidebarEntry {
            id: "daemon-sandbox".into(),
            name: "daemon-sandbox".into(),
            path: "daemon-sandbox".into(),
            branch: "daemon transport".into(),
            initials: "D".into(),
            accent: project_accent_color("daemon-sandbox"),
            active: false,
            loading: false,
            error: false,
            expanded: true,
            task_count_label: "1".into(),
        },
        ProjectSidebarEntry {
            id: "slint-platform".into(),
            name: "slint-platform".into(),
            path: "slint-platform".into(),
            branch: "platform traits".into(),
            initials: "S".into(),
            accent: project_accent_color("slint-platform"),
            active: false,
            loading: false,
            error: false,
            expanded: false,
            task_count_label: "2".into(),
        },
    ])));
    app.set_task_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        TaskSidebarEntry {
            id: "slint-build".into(),
            title: "Slint build".into(),
            branch: "slint-daemon-poc-clean".into(),
            metadata: "active | +0 -0".into(),
            initials: "S".into(),
            accent: project_accent_color("slint-build"),
            active: true,
            pinned: true,
            running: true,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        TaskSidebarEntry {
            id: "terminal-ready".into(),
            title: "Terminal readiness".into(),
            branch: "terminal-production".into(),
            metadata: "in progress | renderer".into(),
            initials: "T".into(),
            accent: project_accent_color("terminal-ready"),
            active: false,
            pinned: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        TaskSidebarEntry {
            id: "style-system".into(),
            title: "Style system".into(),
            branch: "gpui baseline".into(),
            metadata: "blocked | visual corpus".into(),
            initials: "G".into(),
            accent: project_accent_color("style-system"),
            active: false,
            pinned: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
    ])));
    app.set_tab_chips(slint::ModelRc::new(slint::VecModel::from(vec![
        TerminalTabChip {
            id: "main".into(),
            title: "Codex".into(),
            provider: "codex".into(),
            restore_status: "ready".into(),
            failure_message: "".into(),
            failure_details: "".into(),
            active: true,
            running: true,
            pinned: false,
        },
        TerminalTabChip {
            id: "shell".into(),
            title: "Shell".into(),
            provider: "shell".into(),
            restore_status: "not-started".into(),
            failure_message: "".into(),
            failure_details: "".into(),
            active: false,
            running: false,
            pinned: false,
        },
    ])));
}

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
