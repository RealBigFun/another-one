//! Cross-cutting pure formatters used by multiple surface modules.
//!
//! These helpers shape `frame::*` daemon types into the small string and
//! color values that the Slint UI consumes. They live here (rather than in
//! a single surface module) because both `left_sidebar.rs` and
//! `terminal_workspace.rs` need them — pulling them up here avoids one
//! surface depending on another.

use crate::frame;

const PROJECT_ACCENTS: [u32; 8] = [
    0x5b4a9e, 0x2e7d6f, 0xb85c38, 0x3a6ea5, 0x8b5e3c, 0x7b2d5f, 0x4a7c4b, 0x9c5151,
];

/// Format restore status enum values as kebab-case labels for Slint state
/// strings. Uses `Debug` formatting to avoid naming the GPUI restore-status
/// enum directly — we only consume it from `frame` snapshots.
pub(crate) fn restore_status_label(status: &impl std::fmt::Debug) -> &'static str {
    match format!("{status:?}").as_str() {
        "NotStarted" => "not-started",
        "Launching" => "launching",
        "Failed" => "failed",
        _ => "ready",
    }
}

pub(crate) fn project_kind_label(kind: frame::ProjectKind) -> &'static str {
    match kind {
        frame::ProjectKind::Root => "root",
        frame::ProjectKind::Worktree => "worktree",
    }
}

pub(crate) fn task_metadata(task: &frame::TaskSummary) -> String {
    let mut parts = Vec::new();
    if task.name != task.branch_name {
        parts.push(task.branch_name.clone());
    }
    if !task.last_commit_relative.is_empty() {
        parts.push(task.last_commit_relative.clone());
    }
    let mut metadata = parts.join(" • ");
    if task.lines_added != 0 || task.lines_removed != 0 {
        if !metadata.is_empty() {
            metadata.push_str(" • ");
        }
        metadata.push_str(&format!("+{} -{}", task.lines_added, task.lines_removed));
    }
    metadata
}

pub(crate) fn provider_label(provider: frame::AgentProvider) -> &'static str {
    match provider {
        frame::AgentProvider::ClaudeCode => "claude-code",
        frame::AgentProvider::CursorAgent => "cursor-agent",
        frame::AgentProvider::Codex => "codex",
        frame::AgentProvider::Pi => "pi",
        frame::AgentProvider::Gemini => "gemini",
        frame::AgentProvider::OpenCode => "opencode",
        frame::AgentProvider::Amp => "amp",
        frame::AgentProvider::RovoDev => "rovo-dev",
        frame::AgentProvider::Forge => "forge",
        frame::AgentProvider::Shell => "shell",
    }
}

pub(crate) fn compact_path(path: &str) -> String {
    let mut parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .rev()
        .take(3)
        .collect::<Vec<_>>();
    parts.reverse();
    if parts.is_empty() {
        path.to_string()
    } else {
        format!(".../{}", parts.join("/"))
    }
}

pub(crate) fn worktree_name(path: &str) -> String {
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("workspace")
        .to_string()
}

pub(crate) fn initials(label: &str) -> String {
    label
        .chars()
        .find(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase().to_string())
        .unwrap_or_else(|| "A".to_string())
}

pub(crate) fn project_accent_color(id: &str) -> slint::Color {
    let hash = id.bytes().fold(0_u32, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(byte as u32)
    });
    let color = PROJECT_ACCENTS[(hash as usize) % PROJECT_ACCENTS.len()];
    slint::Color::from_argb_encoded(0xff000000 | color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_path_keeps_last_three_segments() {
        assert_eq!(
            compact_path("/home/user/projects/another-one"),
            ".../user/projects/another-one"
        );
        assert_eq!(compact_path(""), "");
        assert_eq!(compact_path("foo"), ".../foo");
    }

    #[test]
    fn worktree_name_uses_last_non_empty_segment() {
        assert_eq!(worktree_name("/home/user/work/"), "work");
        assert_eq!(worktree_name("project"), "project");
        assert_eq!(worktree_name(""), "workspace");
    }

    #[test]
    fn initials_picks_first_alphanumeric_uppercased() {
        assert_eq!(initials("hello"), "H");
        assert_eq!(initials("  Project"), "P");
        assert_eq!(initials("--7th try"), "7");
        assert_eq!(initials(""), "A");
    }

    #[test]
    fn project_accent_color_is_deterministic() {
        let a1 = project_accent_color("project-a");
        let a2 = project_accent_color("project-a");
        let b = project_accent_color("project-b");
        assert_eq!(a1, a2);
        // Two distinct ids may collide but should both be in the palette range.
        assert_ne!(a1, slint::Color::from_argb_encoded(0));
        assert_ne!(b, slint::Color::from_argb_encoded(0));
    }
}
