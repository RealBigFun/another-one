//! Slint right inspector pure data helpers.
//!
//! GPUI source of truth: `desktop/src/right_sidebar.rs`. This module owns
//! the Slint-side flattening of changed files, recent commits, pull-request
//! checks, and branch-compare views into the `RightInspectorRow` shape the
//! Slint UI consumes. It also owns the inspector's row-height constants
//! and the `InspectorCommitFileChangesState` variant tracking per-commit
//! file-change requests.
//!
//! Phase A scope: pure data shapers only — no `AppWindow` mutation helpers,
//! no daemon dispatching. The `set_right_inspector_*` functions still live
//! in lib.rs because they couple to workspace shell wiring; they extract
//! together with the rest of the right-inspector callbacks in a later
//! slice.
//!
//! Port-review reference:
//! `docs/architecture/reviews/slint-right-inspector-port-review.md`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::{frame, RightInspectorRow};

pub(crate) const RIGHT_INSPECTOR_SECTION_ROW_HEIGHT: i32 = 44;
pub(crate) const RIGHT_INSPECTOR_FILE_ROW_HEIGHT: i32 = 34;
pub(crate) const RIGHT_INSPECTOR_COMMIT_ROW_HEIGHT: i32 = 42;
pub(crate) const RIGHT_INSPECTOR_CHECK_ROW_HEIGHT: i32 = 46;

#[derive(Clone, Debug)]
pub(crate) enum InspectorCommitFileChangesState {
    Loading,
    Loaded(Vec<frame::BranchCompareFileWire>),
    Failed,
}

pub(crate) fn right_inspector_rows_for_changed_files_with_collapsed(
    project_id: &str,
    files: &[frame::ChangedFileWire],
    collapsed_sections: &HashSet<String>,
    pending_actions: &HashSet<String>,
) -> (Vec<RightInspectorRow>, String) {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut staged_additions = 0;
    let mut staged_deletions = 0;
    let mut unstaged_additions = 0;
    let mut unstaged_deletions = 0;

    for file in files {
        if changed_file_has_staged_changes(file) {
            staged_additions += file.staged_additions.max(0);
            staged_deletions += file.staged_deletions.max(0);
            staged.push(file);
        }
        if changed_file_has_unstaged_changes(file) {
            unstaged_additions += file.unstaged_additions.max(0);
            unstaged_deletions += file.unstaged_deletions.max(0);
            unstaged.push(file);
        }
    }

    let mut rows = Vec::new();
    let mut row_y = 0;
    if !staged.is_empty() {
        let expanded = !collapsed_sections.contains("staged");
        let pending = pending_actions.contains("section:staged");
        rows.push(right_inspector_section_row_with_expanded(
            project_id,
            "staged",
            "Staged Changes",
            staged.len(),
            staged_additions,
            staged_deletions,
            row_y,
            expanded,
            pending,
        ));
        row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;
        if expanded {
            for file in &staged {
                rows.push(right_inspector_file_row(
                    project_id,
                    "staged",
                    file,
                    row_y,
                    pending_actions,
                ));
                row_y += RIGHT_INSPECTOR_FILE_ROW_HEIGHT;
            }
        }
    }
    if !unstaged.is_empty() {
        let expanded = !collapsed_sections.contains("unstaged");
        let pending = pending_actions.contains("section:unstaged");
        rows.push(right_inspector_section_row_with_expanded(
            project_id,
            "unstaged",
            "Changes",
            unstaged.len(),
            unstaged_additions,
            unstaged_deletions,
            row_y,
            expanded,
            pending,
        ));
        row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;
        if expanded {
            for file in &unstaged {
                rows.push(right_inspector_file_row(
                    project_id,
                    "unstaged",
                    file,
                    row_y,
                    pending_actions,
                ));
                row_y += RIGHT_INSPECTOR_FILE_ROW_HEIGHT;
            }
        }
    }

    let summary = format!("{} staged, {} unstaged", staged.len(), unstaged.len());
    (rows, summary)
}

pub(crate) fn right_inspector_section_row(
    project_id: &str,
    group: &str,
    title: &str,
    file_count: usize,
    additions: i32,
    deletions: i32,
    row_y: i32,
) -> RightInspectorRow {
    right_inspector_section_row_with_expanded(
        project_id, group, title, file_count, additions, deletions, row_y, true, false,
    )
}

pub(crate) fn right_inspector_section_row_with_expanded(
    project_id: &str,
    group: &str,
    title: &str,
    file_count: usize,
    additions: i32,
    deletions: i32,
    row_y: i32,
    expanded: bool,
    pending: bool,
) -> RightInspectorRow {
    RightInspectorRow {
        kind: "section".into(),
        group: group.into(),
        id: format!("section:{group}").into(),
        project_id: project_id.into(),
        path: "".into(),
        original_path: "".into(),
        row_y,
        row_height: RIGHT_INSPECTOR_SECTION_ROW_HEIGHT,
        title: title.into(),
        parent_dir: "".into(),
        status: "".into(),
        status_color: slint::Color::from_argb_encoded(0xff949494),
        additions_label: diff_label(additions, true).into(),
        deletions_label: diff_label(deletions, false).into(),
        file_count_label: file_count.to_string().into(),
        can_stage: group == "unstaged",
        can_unstage: group == "staged",
        untracked: false,
        expanded,
        pending,
    }
}

pub(crate) fn right_inspector_file_row(
    project_id: &str,
    group: &str,
    file: &frame::ChangedFileWire,
    row_y: i32,
    pending_actions: &HashSet<String>,
) -> RightInspectorRow {
    let status = changed_file_status_char(file, group);
    let (file_name, parent_dir) = file_name_and_parent(&file.path);
    let (additions, deletions) = if group == "staged" {
        (file.staged_additions, file.staged_deletions)
    } else {
        (file.unstaged_additions, file.unstaged_deletions)
    };

    let id = right_inspector_changed_file_row_id(group, file.original_path.as_deref(), &file.path);

    RightInspectorRow {
        kind: "file".into(),
        group: group.into(),
        id: id.clone().into(),
        project_id: project_id.into(),
        path: file.path.clone().into(),
        original_path: file.original_path.clone().unwrap_or_default().into(),
        row_y,
        row_height: RIGHT_INSPECTOR_FILE_ROW_HEIGHT,
        title: file_name.into(),
        parent_dir: parent_dir.into(),
        status: status.to_string().into(),
        status_color: changed_file_status_color(status),
        additions_label: diff_label(additions, true).into(),
        deletions_label: diff_label(deletions, false).into(),
        file_count_label: "".into(),
        can_stage: changed_file_has_unstaged_changes(file),
        can_unstage: changed_file_has_staged_changes(file),
        untracked: file.untracked,
        expanded: false,
        pending: pending_actions.contains(&id),
    }
}

pub(crate) fn right_inspector_rows_for_commits_with_expansions(
    project_id: &str,
    view: &frame::RecentCommitsWire,
    expanded_commits: &HashSet<String>,
    file_change_states: &HashMap<String, InspectorCommitFileChangesState>,
) -> Vec<RightInspectorRow> {
    let mut rows = Vec::new();
    let mut row_y = 0;
    let branch = view.current_branch.as_deref().unwrap_or("current branch");
    rows.push(right_inspector_section_row(
        project_id,
        "commits",
        &format!("Recent commits on {branch}"),
        view.commits.len(),
        0,
        0,
        row_y,
    ));
    row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;

    for commit in &view.commits {
        let commit_key = right_inspector_commit_key(project_id, &commit.id);
        let expanded = expanded_commits.contains(&commit_key);
        rows.push(RightInspectorRow {
            kind: "commit".into(),
            group: "commits".into(),
            id: format!("commit:{}", commit.id).into(),
            project_id: project_id.into(),
            path: commit.id.clone().into(),
            original_path: "".into(),
            row_y,
            row_height: RIGHT_INSPECTOR_COMMIT_ROW_HEIGHT,
            title: commit.subject.clone().into(),
            parent_dir: format!("{} - {}", commit.author_name, commit.authored_relative).into(),
            status: commit.short_id.clone().into(),
            status_color: slint::Color::from_argb_encoded(0xff949494),
            additions_label: "".into(),
            deletions_label: "".into(),
            file_count_label: "".into(),
            can_stage: false,
            can_unstage: false,
            untracked: false,
            expanded,
            pending: false,
        });
        row_y += RIGHT_INSPECTOR_COMMIT_ROW_HEIGHT;

        if expanded {
            let state = file_change_states.get(&commit_key);
            rows.extend(right_inspector_commit_detail_rows(
                project_id, commit, state, &mut row_y,
            ));
        }
    }

    rows
}

fn right_inspector_commit_detail_rows(
    project_id: &str,
    commit: &frame::CommitWire,
    state: Option<&InspectorCommitFileChangesState>,
    row_y: &mut i32,
) -> Vec<RightInspectorRow> {
    let mut rows = Vec::new();
    let (title, count, additions, deletions) = match state {
        Some(InspectorCommitFileChangesState::Loaded(files)) if !files.is_empty() => {
            let additions = files.iter().map(|file| file.additions.max(0)).sum();
            let deletions = files.iter().map(|file| file.deletions.max(0)).sum();
            let title = if files.len() == 1 {
                "1 file changed".to_string()
            } else {
                format!("{} files changed", files.len())
            };
            (title, files.len(), additions, deletions)
        }
        Some(InspectorCommitFileChangesState::Loaded(_)) => {
            ("No file changes in this commit.".to_string(), 0, 0, 0)
        }
        Some(InspectorCommitFileChangesState::Failed) => {
            ("Couldn't load file changes.".to_string(), 0, 0, 0)
        }
        _ => ("Loading file changes...".to_string(), 0, 0, 0),
    };

    rows.push(right_inspector_section_row(
        project_id,
        "commit-files",
        &title,
        count,
        additions,
        deletions,
        *row_y,
    ));
    *row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;

    if let Some(InspectorCommitFileChangesState::Loaded(files)) = state {
        for file in files {
            rows.push(right_inspector_commit_file_row(
                project_id, &commit.id, file, *row_y,
            ));
            *row_y += RIGHT_INSPECTOR_FILE_ROW_HEIGHT;
        }
    }

    rows
}

fn right_inspector_commit_file_row(
    project_id: &str,
    commit_id: &str,
    file: &frame::BranchCompareFileWire,
    row_y: i32,
) -> RightInspectorRow {
    let mut row = right_inspector_compare_file_row(project_id, file, row_y);
    row.group = "commit-file".into();
    row.id = format!("commit-file:{commit_id}:{}", file.path).into();
    row
}

pub(crate) fn right_inspector_commit_key(project_id: &str, commit_id: &str) -> String {
    format!("{project_id}:{commit_id}")
}

pub(crate) fn right_inspector_changed_file_row_id(
    group: &str,
    original_path: Option<&str>,
    path: &str,
) -> String {
    format!("{group}:{}:{path}", original_path.unwrap_or(""))
}

pub(crate) fn right_inspector_rows_for_checks(
    project_id: &str,
    checks: &[frame::Check],
) -> Vec<RightInspectorRow> {
    let mut checks = checks.iter().collect::<Vec<_>>();
    checks.sort_by(|left, right| {
        check_bucket_priority(&left.bucket)
            .cmp(&check_bucket_priority(&right.bucket))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    let mut rows = Vec::new();
    let mut row_y = 0;
    rows.push(right_inspector_section_row(
        project_id,
        "checks",
        "Pull request checks",
        checks.len(),
        0,
        0,
        row_y,
    ));
    row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;

    for check in checks {
        let (bucket, color) = check_bucket_visual(&check.bucket);
        rows.push(RightInspectorRow {
            kind: "check".into(),
            group: bucket.into(),
            id: format!("check:{}", check.name).into(),
            project_id: project_id.into(),
            path: check.link.clone().unwrap_or_default().into(),
            original_path: "".into(),
            row_y,
            row_height: RIGHT_INSPECTOR_CHECK_ROW_HEIGHT,
            title: check.name.clone().into(),
            parent_dir: check
                .description
                .clone()
                .unwrap_or_else(|| check.state.clone())
                .into(),
            status: check.state.clone().into(),
            status_color: color,
            additions_label: check.duration_text.clone().unwrap_or_default().into(),
            deletions_label: "".into(),
            file_count_label: "".into(),
            can_stage: false,
            can_unstage: false,
            untracked: false,
            expanded: false,
            pending: false,
        });
        row_y += RIGHT_INSPECTOR_CHECK_ROW_HEIGHT;
    }

    rows
}

pub(crate) fn right_inspector_rows_for_compare(
    project_id: &str,
    view: &frame::BranchCompareWire,
) -> Vec<RightInspectorRow> {
    let mut rows = Vec::new();
    let mut row_y = 0;
    let additions = view.files.iter().map(|file| file.additions.max(0)).sum();
    let deletions = view.files.iter().map(|file| file.deletions.max(0)).sum();
    let current_branch = view.current_branch.as_deref().unwrap_or("current branch");
    rows.push(right_inspector_section_row(
        project_id,
        "compare",
        &format!("Comparing {current_branch} against {}", view.target_branch),
        view.files.len(),
        additions,
        deletions,
        row_y,
    ));
    row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;

    for file in &view.files {
        rows.push(right_inspector_compare_file_row(project_id, file, row_y));
        row_y += RIGHT_INSPECTOR_FILE_ROW_HEIGHT;
    }

    rows
}

fn right_inspector_compare_file_row(
    project_id: &str,
    file: &frame::BranchCompareFileWire,
    row_y: i32,
) -> RightInspectorRow {
    let status = status_char(&file.status);
    let (file_name, parent_dir) = file_name_and_parent(&file.path);
    let parent_label = file
        .original_path
        .as_ref()
        .map(|original_path| format!("Renamed from {original_path}"))
        .unwrap_or(parent_dir);

    RightInspectorRow {
        kind: "file".into(),
        group: "compare".into(),
        id: format!(
            "compare:{}:{}",
            file.original_path.as_deref().unwrap_or(""),
            file.path
        )
        .into(),
        project_id: project_id.into(),
        path: file.path.clone().into(),
        original_path: file.original_path.clone().unwrap_or_default().into(),
        row_y,
        row_height: RIGHT_INSPECTOR_FILE_ROW_HEIGHT,
        title: file_name.into(),
        parent_dir: parent_label.into(),
        status: status.to_string().into(),
        status_color: changed_file_status_color(status),
        additions_label: diff_label(file.additions, true).into(),
        deletions_label: diff_label(file.deletions, false).into(),
        file_count_label: "".into(),
        can_stage: false,
        can_unstage: false,
        untracked: false,
        expanded: false,
        pending: false,
    }
}

fn check_bucket_priority(bucket: &impl std::fmt::Debug) -> u8 {
    match format!("{bucket:?}").as_str() {
        "Fail" => 0,
        "Pending" => 1,
        "Pass" => 2,
        _ => 3,
    }
}

fn check_bucket_visual(bucket: &impl std::fmt::Debug) -> (&'static str, slint::Color) {
    match format!("{bucket:?}").as_str() {
        "Pass" => ("pass", slint::Color::from_argb_encoded(0xff8bd99c)),
        "Fail" => ("fail", slint::Color::from_argb_encoded(0xffe58b95)),
        "Pending" => ("pending", slint::Color::from_argb_encoded(0xffe6c36d)),
        _ => ("skipped", slint::Color::from_argb_encoded(0xff8f8f8f)),
    }
}

fn changed_file_has_staged_changes(file: &frame::ChangedFileWire) -> bool {
    let status = status_char(&file.index_status);
    status != ' ' && status != '?'
}

fn changed_file_has_unstaged_changes(file: &frame::ChangedFileWire) -> bool {
    let status = status_char(&file.worktree_status);
    file.untracked || (status != ' ' && status != '?')
}

fn changed_file_status_char(file: &frame::ChangedFileWire, group: &str) -> char {
    let raw = if group == "staged" {
        status_char(&file.index_status)
    } else if file.untracked {
        'A'
    } else {
        status_char(&file.worktree_status)
    };

    match raw {
        '?' | ' ' => 'A',
        other => other,
    }
}

fn changed_file_status_color(status: char) -> slint::Color {
    let rgb = match status {
        'A' => 0x85dda0,
        'D' => 0xe47777,
        'R' | 'C' => 0x86c6e9,
        _ => 0xe6cc4d,
    };
    slint::Color::from_argb_encoded(0xff000000 | rgb)
}

fn status_char(status: &str) -> char {
    status.chars().next().unwrap_or(' ')
}

fn diff_label(value: i32, positive: bool) -> String {
    if value <= 0 {
        String::new()
    } else if positive {
        format!("+{value}")
    } else {
        format!("-{value}")
    }
}

fn file_name_and_parent(path: &str) -> (String, String) {
    let path_ref = Path::new(path);
    let file_name = path_ref
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string();
    let parent = path_ref
        .parent()
        .and_then(|parent| parent.to_str())
        .filter(|parent| !parent.is_empty() && *parent != ".")
        .unwrap_or_default()
        .to_string();
    (file_name, parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the Slint right-inspector pure helpers to the GPUI rendering owner
    /// so a rename in `desktop/src/right_sidebar.rs` forces a Slint-side
    /// review. The full behavior matrix is in the port-review doc; this test
    /// only checks the rendering function and the daemon wire types this
    /// module shapes into rows.
    #[test]
    fn slint_right_inspector_pins_to_gpui_right_sidebar_symbols() {
        let gpui = include_str!("../../desktop/src/right_sidebar.rs");
        let frame = include_str!("../../daemon-sandbox/src/frame.rs");
        assert!(
            gpui.contains("fn changed_files_panel"),
            "GPUI right-inspector renderer missing or renamed: fn changed_files_panel"
        );
        for wire in ["ChangedFileWire", "RecentCommitsWire", "BranchCompareWire"] {
            assert!(
                frame.contains(wire),
                "Daemon wire type missing or renamed: {wire}; Slint right-inspector reshapes it."
            );
        }
    }

    #[test]
    fn diff_label_omits_zero_and_signs_value() {
        assert_eq!(diff_label(0, true), "");
        assert_eq!(diff_label(0, false), "");
        assert_eq!(diff_label(7, true), "+7");
        assert_eq!(diff_label(7, false), "-7");
        assert_eq!(diff_label(-1, true), "");
    }

    #[test]
    fn file_name_and_parent_handles_root_and_nested() {
        let (name, parent) = file_name_and_parent("README.md");
        assert_eq!(name, "README.md");
        assert_eq!(parent, "");

        let (name, parent) = file_name_and_parent("desktop/src/right_sidebar.rs");
        assert_eq!(name, "right_sidebar.rs");
        assert_eq!(parent, "desktop/src");
    }
}
