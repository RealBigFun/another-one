use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::{
    AppWindow, SettingsAgentRow, SettingsGeneralRow, SettingsGitActionPanel, SettingsMcpRow,
    SettingsNavEntry, SettingsOpenInRow, SettingsShortcutRow,
};

pub(crate) const SETTINGS_SECTION_IDS: [&str; 6] = [
    "general",
    "agents",
    "open-in",
    "git-actions",
    "keybindings",
    "mcp",
];

pub(crate) const SETTINGS_SECTION_LABELS: [&str; 6] = [
    "General",
    "Agents",
    "Open In",
    "Git Actions",
    "Keybindings",
    "MCP",
];

pub(crate) fn seed_settings_model(app: &AppWindow) {
    app.set_settings_active_section("general".into());
    app.set_settings_nav_entries(model(settings_nav_entries()));
    app.set_settings_general_rows(model(settings_general_rows()));
    app.set_settings_agent_rows(model(settings_agent_rows()));
    app.set_settings_open_in_rows(model(settings_open_in_rows()));
    app.set_settings_git_action_panels(model(settings_git_action_panels()));
    app.set_settings_shortcut_rows(model(settings_shortcut_rows()));
    app.set_settings_mcp_rows(model(settings_mcp_rows()));
}

pub(crate) fn wire_settings_callbacks(app: &AppWindow) {
    let action_app = app.as_weak();
    app.on_settings_action_requested(move |scope, id| {
        if let Some(app) = action_app.upgrade() {
            show_settings_toast(
                &app,
                "info",
                format!("Settings action queued: {scope} / {id}"),
            );
        }
    });

    let toggle_app = app.as_weak();
    app.on_settings_toggle_requested(move |scope, id, enabled| {
        if let Some(app) = toggle_app.upgrade() {
            let state = if enabled { "enabled" } else { "disabled" };
            show_settings_toast(&app, "info", format!("{scope} {id} {state}."));
        }
    });
}

fn model<T: Clone + 'static>(model: VecModel<T>) -> ModelRc<T> {
    ModelRc::new(model)
}

fn settings_nav_entries() -> VecModel<SettingsNavEntry> {
    VecModel::from(
        SETTINGS_SECTION_IDS
            .into_iter()
            .zip(SETTINGS_SECTION_LABELS)
            .map(|(id, label)| SettingsNavEntry {
                id: shared(id),
                label: shared(label),
            })
            .collect::<Vec<_>>(),
    )
}

fn settings_general_rows() -> VecModel<SettingsGeneralRow> {
    VecModel::from(vec![
        SettingsGeneralRow {
            id: shared("sidebar-git-metadata"),
            title: shared("Sidebar git metadata"),
            detail: shared("Show relative commit time and +/- line counts in task rows."),
            status: shared("Enabled"),
            enabled: true,
            action_label: shared("Toggle"),
        },
        SettingsGeneralRow {
            id: shared("build"),
            title: shared("Build"),
            detail: shared("Installed build identity. The GPUI baseline exposes short SHA, profile, cargo version, and copyable full SHA."),
            status: shared("Static"),
            enabled: false,
            action_label: shared(""),
        },
        SettingsGeneralRow {
            id: shared("updates"),
            title: shared("Updates"),
            detail: shared("Check for updates and install a downloaded update when one is ready."),
            status: shared("Idle"),
            enabled: true,
            action_label: shared("Check"),
        },
    ])
}

fn settings_agent_rows() -> VecModel<SettingsAgentRow> {
    VecModel::from(vec![
        agent_row("claude-code", "Claude Code", "No extra args", true, false),
        agent_row("codex", "Codex", "No extra args", true, false),
        agent_row("cursor", "Cursor Agent", "No extra args", true, false),
        agent_row("gemini", "Gemini", "No extra args", true, false),
        agent_row("pi", "Pi", "No extra args", true, true),
        agent_row("opencode", "OpenCode", "No extra args", true, false),
        agent_row("amp", "Amp", "No extra args", true, false),
        agent_row("rovo-dev", "Rovo Dev", "No extra args", true, false),
        agent_row("forge", "Forge", "No extra args", true, false),
    ])
}

fn settings_open_in_rows() -> VecModel<SettingsOpenInRow> {
    VecModel::from(vec![
        open_in_row("cursor", "Cursor", "Open the project directory in Cursor."),
        open_in_row("zed", "Zed", "Open the project directory in Zed."),
        open_in_row(
            "vscode",
            "VS Code",
            "Open the project directory in VS Code.",
        ),
        open_in_row(
            "file-manager",
            if cfg!(target_os = "macos") {
                "Finder"
            } else {
                "File Manager"
            },
            if cfg!(target_os = "macos") {
                "Open the project directory in Finder."
            } else {
                "Open the project directory in the system file manager."
            },
        ),
    ])
}

fn settings_git_action_panels() -> VecModel<SettingsGitActionPanel> {
    VecModel::from(vec![
        SettingsGitActionPanel {
            id: shared("commit"),
            title: shared("Commit message instructions"),
            detail: shared("Currently using the default built-in template."),
            placeholder: shared("Paste commit generation instructions here."),
            script_preview: shared("Generate a concise conventional commit message for the staged git diff. Return only the commit message."),
            custom: false,
        },
        SettingsGitActionPanel {
            id: shared("pull-request"),
            title: shared("PR title/body instructions"),
            detail: shared("Currently using the default built-in template."),
            placeholder: shared("Paste PR title/body instructions here."),
            script_preview: shared("Return only the PR title/body content: first line title, second line blank, remaining lines body."),
            custom: false,
        },
    ])
}

fn settings_shortcut_rows() -> VecModel<SettingsShortcutRow> {
    VecModel::from(vec![
        shortcut_row("cycle-projects", "Cycle Projects", "Cmd-O"),
        shortcut_row(
            "new-tab-in-current-task",
            "New Tab in Current Task",
            "Cmd-N",
        ),
        shortcut_row("new-task", "New Task", "Cmd-T"),
        shortcut_row(
            "close-current-tab",
            "Close Current Tab",
            if cfg!(target_os = "macos") {
                "Cmd-W"
            } else {
                "Ctrl-W"
            },
        ),
        shortcut_row("next-tab", "Next Tab", "Cmd-Shift-]"),
        shortcut_row("previous-tab", "Previous Tab", "Cmd-Shift-["),
        shortcut_row("next-task", "Next Task", "Cmd-Alt-Down"),
        shortcut_row("previous-task", "Previous Task", "Cmd-Alt-Up"),
    ])
}

fn settings_mcp_rows() -> VecModel<SettingsMcpRow> {
    VecModel::from(vec![
        mcp_row(
            "context7",
            "Context7",
            "Catalog server for library docs and code examples.",
            "catalog",
            "Claude Cursor Codex",
            false,
        ),
        mcp_row(
            "filesystem",
            "Filesystem",
            "Built-in daemon entry for local project file access.",
            "daemon",
            "Claude Cursor Gemini",
            true,
        ),
        mcp_row(
            "custom-json",
            "Custom entry (JSON)",
            "Custom transports, env, and headers are edited in ~/.config/another-one/mcp.json.",
            "custom",
            "manual",
            false,
        ),
    ])
}

fn agent_row(
    id: &str,
    label: &str,
    args_label: &str,
    enabled: bool,
    default_agent: bool,
) -> SettingsAgentRow {
    SettingsAgentRow {
        id: shared(id),
        label: shared(label),
        detail: shared(format!(
            "Extra argv tokens passed to {label} on every launch and resume."
        )),
        args_label: shared(args_label),
        enabled,
        default_agent,
    }
}

fn open_in_row(id: &str, label: &str, detail: &str) -> SettingsOpenInRow {
    SettingsOpenInRow {
        id: shared(id),
        label: shared(label),
        detail: shared(detail),
        enabled: true,
    }
}

fn shortcut_row(id: &str, label: &str, binding: &str) -> SettingsShortcutRow {
    SettingsShortcutRow {
        id: shared(id),
        label: shared(label),
        binding: shared(binding),
        capturing: false,
    }
}

fn mcp_row(
    id: &str,
    label: &str,
    detail: &str,
    source: &str,
    provider_summary: &str,
    installed: bool,
) -> SettingsMcpRow {
    SettingsMcpRow {
        id: shared(id),
        label: shared(label),
        detail: shared(detail),
        source: shared(source),
        provider_summary: shared(provider_summary),
        installed,
    }
}

fn show_settings_toast(app: &AppWindow, kind: &str, message: impl Into<SharedString>) {
    app.set_toast_kind(kind.into());
    app.set_toast_message(message.into());
    app.set_toast_detail("".into());
}

fn shared(value: impl Into<SharedString>) -> SharedString {
    value.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    const GPUI_SETTINGS: &str = include_str!("../../desktop/src/settings_page.rs");
    const GPUI_MCP: &str = include_str!("../../desktop/src/mcp_page.rs");
    const CORE_AGENTS: &str = include_str!("../../core/src/agents.rs");
    const CORE_SHORTCUTS: &str = include_str!("../../core/src/shortcuts.rs");
    const SLINT_SETTINGS: &str = include_str!("../ui/settings.slint");

    #[test]
    fn section_contract_matches_gpui_order() {
        assert_eq!(
            SETTINGS_SECTION_LABELS,
            [
                "General",
                "Agents",
                "Open In",
                "Git Actions",
                "Keybindings",
                "MCP"
            ]
        );

        for label in SETTINGS_SECTION_LABELS {
            assert!(
                GPUI_SETTINGS.contains(&format!("\"{label}\"")),
                "missing GPUI section label {label}"
            );
            assert!(
                SLINT_SETTINGS.contains(&format!("title: \"{label}\""))
                    || SLINT_SETTINGS.contains(&format!("text: \"{label}\"")),
                "missing Slint section label {label}"
            );
        }
    }

    #[test]
    fn settings_model_preserves_gpui_agent_and_shortcut_labels() {
        for label in [
            "Claude Code",
            "Codex",
            "Cursor Agent",
            "Gemini",
            "Pi",
            "OpenCode",
            "Amp",
            "Rovo Dev",
            "Forge",
        ] {
            assert!(CORE_AGENTS.contains(&format!("label: \"{label}\"")));
            assert!(settings_agent_rows()
                .iter()
                .any(|row| row.label.as_str() == label));
        }

        for label in [
            "Cycle Projects",
            "New Tab in Current Task",
            "New Task",
            "Close Current Tab",
            "Next Tab",
            "Previous Tab",
            "Next Task",
            "Previous Task",
        ] {
            assert!(CORE_SHORTCUTS.contains(&format!("\"{label}\"")));
            assert!(settings_shortcut_rows()
                .iter()
                .any(|row| row.label.as_str() == label));
        }
    }

    #[test]
    fn mcp_provider_summary_tracks_gpui_supported_harnesses() {
        for provider in ["Claude", "Cursor", "Codex", "Gemini", "OpenCode", "Amp"] {
            assert!(GPUI_MCP.contains(&format!("\"{provider}\"")));
        }

        assert!(settings_mcp_rows()
            .iter()
            .any(|row| row.provider_summary.as_str().contains("Codex")));
    }
}
