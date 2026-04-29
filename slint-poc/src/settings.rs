use std::{cell::RefCell, collections::HashSet, rc::Rc};

use daemon_sandbox::frame;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tokio::sync::mpsc;

use crate::{
    AppWindow, SettingsAgentRow, SettingsGeneralRow, SettingsGitActionPanel, SettingsMcpRow,
    SettingsNavEntry, SettingsOpenInRow, SettingsShortcutRow, SlintClientEvent,
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

const DEFAULT_AGENT_ID: &str = "pi";

pub(crate) const SETTINGS_SHORTCUT_IDS: [&str; 8] = [
    "cycle-projects",
    "new-tab-in-current-task",
    "new-task",
    "close-current-tab",
    "next-tab",
    "previous-tab",
    "next-task",
    "previous-task",
];

const DEFAULT_COMMIT_SCRIPT: &str = concat!(
    "Generate a git commit message for these staged changes.\n",
    "Return only the commit message text.\n",
    "Rules:\n",
    "- Prefer Conventional Commit style when it fits.\n",
    "- First line must be a concise subject in imperative mood.\n",
    "- Keep the subject under 72 characters.\n",
    "- Add a blank line plus a short body only if it materially helps.\n",
    "- No markdown fences, no commentary, no quotes.\n"
);

const DEFAULT_PR_SCRIPT: &str = concat!(
    "Generate a GitHub pull request title and body for these branch changes.\n",
    "Focus on the substance of the change, not the git mechanics.\n",
    "Rules:\n",
    "- Write a concise, specific PR title.\n",
    "- The body should summarize what changed and any important reviewer context.\n",
    "- Mention notable user-visible behavior changes, refactors, fixes, or follow-up context when relevant.\n",
    "- Keep the body skimmable and avoid filler.\n"
);

pub(crate) fn seed_settings_model(app: &AppWindow) {
    app.set_settings_nav_entries(model(settings_nav_entries()));
    SettingsState::baseline().apply_to(app);
}

pub(crate) fn wire_settings_callbacks(
    app: &AppWindow,
    settings_event_tx: mpsc::UnboundedSender<SlintClientEvent>,
) {
    let state = Rc::new(RefCell::new(SettingsState::baseline()));

    let nav_state = Rc::clone(&state);
    let nav_app = app.as_weak();
    app.on_settings_nav_selected(move |section| {
        if let Some(app) = nav_app.upgrade() {
            let feedback = {
                let mut state = nav_state.borrow_mut();
                state.sync_from(&app);
                let feedback = state.select_section(section.as_str());
                state.apply_to(&app);
                feedback
            };
            if let Some(feedback) = feedback {
                feedback.show(&app);
            }
        }
    });

    let action_state = Rc::clone(&state);
    let action_app = app.as_weak();
    let action_tx = settings_event_tx.clone();
    app.on_settings_action_requested(move |scope, id| {
        if let Some(app) = action_app.upgrade() {
            let (request, feedback) = {
                let mut state = action_state.borrow_mut();
                state.sync_from(&app);
                let request = state.request_for_action(scope.as_str(), id.as_str());
                let feedback = state.handle_action(scope.as_str(), id.as_str());
                state.apply_to(&app);
                (request, feedback)
            };
            feedback.show(&app);
            send_settings_request(&action_tx, request, &app);
        }
    });

    let toggle_state = Rc::clone(&state);
    let toggle_app = app.as_weak();
    let toggle_tx = settings_event_tx;
    app.on_settings_toggle_requested(move |scope, id, enabled| {
        if let Some(app) = toggle_app.upgrade() {
            let (request, feedback) = {
                let mut state = toggle_state.borrow_mut();
                state.sync_from(&app);
                let request = state.request_for_toggle(scope.as_str(), id.as_str(), enabled);
                let feedback = state.handle_toggle(scope.as_str(), id.as_str(), enabled);
                state.apply_to(&app);
                (request, feedback)
            };
            feedback.show(&app);
            send_settings_request(&toggle_tx, request, &app);
        }
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SettingsRequest {
    SetAgentEnabled { agent_id: String, enabled: bool },
    SetDefaultAgent { agent_id: String },
    SetOpenInAppEnabled { app_id: String, enabled: bool },
    ResetGitActionScript { script_id: String },
    ResetShortcut { action_id: String },
    ClearShortcut { action_id: String },
    ResetAllShortcuts,
    McpAddFromCatalog { catalog_id: String },
    McpRemove { entry_id: String },
}

#[derive(Clone)]
struct SettingsState {
    active_section: SharedString,
    general_rows: Vec<SettingsGeneralRow>,
    agent_rows: Vec<SettingsAgentRow>,
    open_in_rows: Vec<SettingsOpenInRow>,
    git_action_panels: Vec<SettingsGitActionPanel>,
    shortcut_rows: Vec<SettingsShortcutRow>,
    mcp_rows: Vec<SettingsMcpRow>,
}

impl SettingsState {
    fn baseline() -> Self {
        Self {
            active_section: shared("general"),
            general_rows: settings_general_rows(),
            agent_rows: settings_agent_rows(),
            open_in_rows: settings_open_in_rows(),
            git_action_panels: settings_git_action_panels(),
            shortcut_rows: settings_shortcut_rows(),
            mcp_rows: settings_mcp_rows(),
        }
    }

    fn apply_to(&self, app: &AppWindow) {
        app.set_settings_active_section(self.active_section.clone());
        app.set_settings_general_rows(model(self.general_rows.clone()));
        app.set_settings_agent_rows(model(self.agent_rows.clone()));
        app.set_settings_open_in_rows(model(self.open_in_rows.clone()));
        app.set_settings_git_action_panels(model(self.git_action_panels.clone()));
        app.set_settings_shortcut_rows(model(self.shortcut_rows.clone()));
        app.set_settings_mcp_rows(model(self.mcp_rows.clone()));
    }

    fn sync_from(&mut self, app: &AppWindow) {
        self.active_section = app.get_settings_active_section();
        self.general_rows = collect_model(app.get_settings_general_rows());
        self.agent_rows = collect_model(app.get_settings_agent_rows());
        self.open_in_rows = collect_model(app.get_settings_open_in_rows());
        self.git_action_panels = collect_model(app.get_settings_git_action_panels());
        self.shortcut_rows = collect_model(app.get_settings_shortcut_rows());
        self.mcp_rows = collect_model(app.get_settings_mcp_rows());
    }

    fn select_section(&mut self, section: &str) -> Option<SettingsFeedback> {
        if !is_known_settings_section(section) {
            self.active_section = shared("general");
            self.reset_transient_state();
            return Some(SettingsFeedback::warning(format!(
                "Unknown settings section {section}; showing General."
            )));
        }

        self.active_section = shared(section);
        self.reset_transient_state();
        None
    }

    fn reset_transient_state(&mut self) {
        for row in &mut self.shortcut_rows {
            row.capturing = false;
            row.status_detail = shortcut_status_detail(&row.default_binding, false);
        }
    }

    fn handle_action(&mut self, scope: &str, id: &str) -> SettingsFeedback {
        match scope {
            "general.primary" => self.handle_general_primary_action(id),
            "general.secondary" => self.handle_general_secondary_action(id),
            "agents.default" => self.set_default_agent(id),
            "git-actions.reset" => self.reset_git_action_script(id),
            "keybindings.capture" => self.capture_shortcut(id),
            "keybindings.reset" => self.reset_shortcut(id),
            "keybindings.clear" => self.clear_shortcut(id),
            "keybindings.reset-all" => self.reset_all_shortcuts(),
            "mcp" => self.toggle_mcp_row(id),
            _ => {
                SettingsFeedback::warning(format!("No Slint settings handler for {scope} / {id}."))
            }
        }
    }

    fn handle_toggle(&mut self, scope: &str, id: &str, enabled: bool) -> SettingsFeedback {
        match scope {
            "general" => self.toggle_general_row(id, enabled),
            "agents" => self.toggle_agent(id, enabled),
            "open-in" => self.toggle_open_in_app(id, enabled),
            _ => SettingsFeedback::warning(format!("No Slint settings toggle for {scope} / {id}.")),
        }
    }

    fn request_for_action(&self, scope: &str, id: &str) -> Option<SettingsRequest> {
        match scope {
            "agents.default" => self
                .agent_rows
                .iter()
                .find(|row| row.id == id && row.enabled && !row.default_agent)
                .map(|_| SettingsRequest::SetDefaultAgent {
                    agent_id: id.to_string(),
                }),
            "git-actions.reset" if self.git_action_panels.iter().any(|panel| panel.id == id) => {
                Some(SettingsRequest::ResetGitActionScript {
                    script_id: id.to_string(),
                })
            }
            "keybindings.reset" if self.shortcut_rows.iter().any(|row| row.id == id) => {
                Some(SettingsRequest::ResetShortcut {
                    action_id: id.to_string(),
                })
            }
            "keybindings.clear" if self.shortcut_rows.iter().any(|row| row.id == id) => {
                Some(SettingsRequest::ClearShortcut {
                    action_id: id.to_string(),
                })
            }
            "keybindings.reset-all" => Some(SettingsRequest::ResetAllShortcuts),
            "mcp" => self
                .mcp_rows
                .iter()
                .find(|row| row.id == id && row.action_enabled)
                .map(|row| {
                    if row.installed {
                        SettingsRequest::McpRemove {
                            entry_id: id.to_string(),
                        }
                    } else {
                        SettingsRequest::McpAddFromCatalog {
                            catalog_id: id.to_string(),
                        }
                    }
                }),
            _ => None,
        }
    }

    fn request_for_toggle(&self, scope: &str, id: &str, enabled: bool) -> Option<SettingsRequest> {
        match scope {
            "agents" if self.agent_rows.iter().any(|row| row.id == id) => {
                Some(SettingsRequest::SetAgentEnabled {
                    agent_id: id.to_string(),
                    enabled,
                })
            }
            "open-in" if self.open_in_rows.iter().any(|row| row.id == id) => {
                Some(SettingsRequest::SetOpenInAppEnabled {
                    app_id: id.to_string(),
                    enabled,
                })
            }
            _ => None,
        }
    }

    fn handle_general_primary_action(&mut self, id: &str) -> SettingsFeedback {
        match id {
            "build" => SettingsFeedback::info(
                "Build identity copy needs app-level clipboard wiring in a later slice.",
            ),
            "updates" => {
                if let Some(row) = self.general_rows.iter_mut().find(|row| row.id == id) {
                    row.status = shared("Checking");
                    row.status_detail =
                        shared("Updater command queued; real updater state is not wired yet.");
                    row.action_enabled = false;
                }
                SettingsFeedback::info("Checking for updates from the Slint settings model.")
            }
            _ => SettingsFeedback::warning(format!("No General action for {id}.")),
        }
    }

    fn handle_general_secondary_action(&mut self, id: &str) -> SettingsFeedback {
        match id {
            "updates" => SettingsFeedback::warning("No downloaded update is ready to install."),
            _ => SettingsFeedback::warning(format!("No secondary General action for {id}.")),
        }
    }

    fn toggle_general_row(&mut self, id: &str, enabled: bool) -> SettingsFeedback {
        let Some(row) = self.general_rows.iter_mut().find(|row| row.id == id) else {
            return SettingsFeedback::warning(format!("No General toggle for {id}."));
        };

        row.enabled = enabled;
        row.status = shared(if enabled { "Enabled" } else { "Disabled" });
        row.status_detail = shared("Mirrors the GPUI ProjectStore preference in this Slint slice.");
        SettingsFeedback::success(format!(
            "{} {}.",
            row.title,
            if enabled { "enabled" } else { "disabled" }
        ))
    }

    fn set_default_agent(&mut self, id: &str) -> SettingsFeedback {
        let Some(index) = self.agent_rows.iter().position(|row| row.id == id) else {
            return SettingsFeedback::warning(format!("No agent row for {id}."));
        };

        if !self.agent_rows[index].enabled {
            return SettingsFeedback::error("Enable the agent before making it the default.");
        }

        for row in &mut self.agent_rows {
            row.default_agent = row.id == id;
        }

        SettingsFeedback::success(format!(
            "Default agent set to {}.",
            self.agent_rows[index].label
        ))
    }

    fn toggle_agent(&mut self, id: &str, enabled: bool) -> SettingsFeedback {
        let Some(index) = self.agent_rows.iter().position(|row| row.id == id) else {
            return SettingsFeedback::warning(format!("No agent toggle for {id}."));
        };

        self.agent_rows[index].enabled = enabled;
        if !enabled && self.agent_rows[index].default_agent {
            self.agent_rows[index].default_agent = false;
            self.reconcile_default_agent();
        } else if enabled && !self.agent_rows.iter().any(|row| row.default_agent) {
            self.reconcile_default_agent();
        }

        SettingsFeedback::success(format!(
            "{} {}.",
            self.agent_rows[index].label,
            if enabled { "enabled" } else { "disabled" }
        ))
    }

    fn reconcile_default_agent(&mut self) {
        let default_index = self
            .agent_rows
            .iter()
            .position(|row| row.id == DEFAULT_AGENT_ID && row.enabled)
            .or_else(|| self.agent_rows.iter().position(|row| row.enabled));

        for (index, row) in self.agent_rows.iter_mut().enumerate() {
            row.default_agent = default_index == Some(index);
        }
    }

    fn toggle_open_in_app(&mut self, id: &str, enabled: bool) -> SettingsFeedback {
        let Some(row) = self.open_in_rows.iter_mut().find(|row| row.id == id) else {
            return SettingsFeedback::warning(format!("No Open In toggle for {id}."));
        };

        row.enabled = enabled;
        row.status = shared(if enabled { "Enabled" } else { "Hidden" });
        SettingsFeedback::success(format!(
            "{} {} in the Open In menu.",
            row.label,
            if enabled { "shown" } else { "hidden" }
        ))
    }

    fn reset_git_action_script(&mut self, id: &str) -> SettingsFeedback {
        let Some(panel) = self
            .git_action_panels
            .iter_mut()
            .find(|panel| panel.id == id)
        else {
            return SettingsFeedback::warning(format!("No Git Actions panel for {id}."));
        };

        panel.custom = false;
        panel.action_enabled = true;
        panel.status = shared("Default");
        panel.detail = shared("Currently using the default built-in template.");
        panel.script_preview = shared(match id {
            "commit" => DEFAULT_COMMIT_SCRIPT,
            "pull-request" => DEFAULT_PR_SCRIPT,
            _ => "",
        });

        SettingsFeedback::success(format!("Reset {}.", panel.title))
    }

    fn capture_shortcut(&mut self, id: &str) -> SettingsFeedback {
        let mut found = false;
        for row in &mut self.shortcut_rows {
            row.capturing = row.id == id;
            if row.capturing {
                found = true;
                row.status_detail = shared("Esc cancels. Delete clears.");
            } else {
                row.status_detail = shortcut_status_detail(&row.default_binding, false);
            }
        }

        if found {
            SettingsFeedback::info("Listening for a shortcut in the Slint settings model.")
        } else {
            SettingsFeedback::warning(format!("No shortcut row for {id}."))
        }
    }

    fn reset_shortcut(&mut self, id: &str) -> SettingsFeedback {
        let Some(row) = self.shortcut_rows.iter_mut().find(|row| row.id == id) else {
            return SettingsFeedback::warning(format!("No shortcut reset for {id}."));
        };

        row.binding = row.default_binding.clone();
        row.capturing = false;
        row.clear_enabled = true;
        row.reset_enabled = true;
        row.status_detail = shortcut_status_detail(&row.default_binding, false);
        SettingsFeedback::success(format!("Reset {}.", row.label))
    }

    fn clear_shortcut(&mut self, id: &str) -> SettingsFeedback {
        let Some(row) = self.shortcut_rows.iter_mut().find(|row| row.id == id) else {
            return SettingsFeedback::warning(format!("No shortcut clear for {id}."));
        };

        row.binding = shared("");
        row.capturing = false;
        row.clear_enabled = false;
        row.reset_enabled = true;
        row.status_detail = shared("Cleared; edit to capture a new shortcut or reset to default.");
        SettingsFeedback::success(format!("Cleared {}.", row.label))
    }

    fn reset_all_shortcuts(&mut self) -> SettingsFeedback {
        for row in &mut self.shortcut_rows {
            row.binding = row.default_binding.clone();
            row.capturing = false;
            row.clear_enabled = true;
            row.reset_enabled = true;
            row.status_detail = shortcut_status_detail(&row.default_binding, false);
        }
        SettingsFeedback::success("Reset all shortcuts.")
    }

    fn toggle_mcp_row(&mut self, id: &str) -> SettingsFeedback {
        let Some(row) = self.mcp_rows.iter_mut().find(|row| row.id == id) else {
            return SettingsFeedback::warning(format!("No MCP row for {id}."));
        };

        if !row.action_enabled {
            return SettingsFeedback::warning(format!(
                "{} is managed outside this control.",
                row.label
            ));
        }

        if row.installed {
            row.installed = false;
            row.action_label = shared("Add");
            row.status = shared("Catalog prompt");
            row.provider_summary = shared("Add before syncing");
            SettingsFeedback::success(format!(
                "Removed {} from the MCP registry model.",
                row.label
            ))
        } else {
            row.installed = true;
            row.action_label = shared("Remove");
            row.status = shared("Registry");
            row.provider_summary = shared("Claude Cursor Codex");
            SettingsFeedback::success(format!("Added {} to the MCP registry model.", row.label))
        }
    }
}

struct SettingsFeedback {
    kind: &'static str,
    message: SharedString,
}

impl SettingsFeedback {
    fn success(message: impl Into<SharedString>) -> Self {
        Self {
            kind: "success",
            message: message.into(),
        }
    }

    fn info(message: impl Into<SharedString>) -> Self {
        Self {
            kind: "info",
            message: message.into(),
        }
    }

    fn warning(message: impl Into<SharedString>) -> Self {
        Self {
            kind: "warning",
            message: message.into(),
        }
    }

    fn error(message: impl Into<SharedString>) -> Self {
        Self {
            kind: "error",
            message: message.into(),
        }
    }

    fn show(self, app: &AppWindow) {
        show_settings_toast(app, self.kind, self.message);
    }
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

fn collect_model<T: Clone + 'static>(items: ModelRc<T>) -> Vec<T> {
    (0..items.row_count())
        .filter_map(|index| items.row_data(index))
        .collect()
}

pub(crate) fn apply_agent_settings(
    app_weak: &slint::Weak<AppWindow>,
    view: frame::AgentSettingsViewWire,
) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_settings_agent_rows(model(agent_rows_from_daemon(&view)));
        }
    });
}

pub(crate) fn apply_open_in_settings(
    app_weak: &slint::Weak<AppWindow>,
    view: frame::OpenInSettingsViewWire,
) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_settings_open_in_rows(model(open_in_rows_from_daemon(&view)));
        }
    });
}

pub(crate) fn apply_git_action_scripts(
    app_weak: &slint::Weak<AppWindow>,
    view: frame::GitActionScriptsView,
) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_settings_git_action_panels(model(git_action_panels_from_daemon(&view)));
        }
    });
}

pub(crate) fn apply_shortcut_settings(
    app_weak: &slint::Weak<AppWindow>,
    view: frame::ShortcutSettingsView,
) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_settings_shortcut_rows(model(shortcut_rows_from_daemon(&view)));
        }
    });
}

pub(crate) fn apply_mcp_settings(app_weak: &slint::Weak<AppWindow>, view: frame::McpSettingsView) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_settings_mcp_rows(model(mcp_rows_from_daemon(&view)));
        }
    });
}

fn is_known_settings_section(section: &str) -> bool {
    SETTINGS_SECTION_IDS.contains(&section)
}

fn settings_nav_entries() -> Vec<SettingsNavEntry> {
    SETTINGS_SECTION_IDS
        .into_iter()
        .zip(SETTINGS_SECTION_LABELS)
        .map(|(id, label)| SettingsNavEntry {
            id: shared(id),
            label: shared(label),
        })
        .collect()
}

fn agent_rows_from_daemon(view: &frame::AgentSettingsViewWire) -> Vec<SettingsAgentRow> {
    view.agents
        .iter()
        .map(|row| {
            let args_label = if row.launch_args.is_empty() {
                "No extra args".to_string()
            } else {
                row.launch_args.join(" ")
            };
            SettingsAgentRow {
                id: shared(row.id.clone()),
                label: shared(row.label.clone()),
                detail: shared(format!(
                    "Extra argv tokens passed to {} on every launch and resume.",
                    row.label
                )),
                args_label: shared(args_label),
                enabled: row.enabled,
                default_agent: row.is_default,
                validation: shared("Arg tokens reject empty values and whitespace."),
                action_label: shared("Make default"),
                action_enabled: row.enabled && !row.is_default,
            }
        })
        .collect()
}

fn open_in_rows_from_daemon(view: &frame::OpenInSettingsViewWire) -> Vec<SettingsOpenInRow> {
    view.available_apps
        .iter()
        .map(|row| SettingsOpenInRow {
            id: shared(row.id.clone()),
            label: shared(row.label.clone()),
            detail: shared(row.description.clone()),
            enabled: row.enabled,
            status: shared(if row.enabled { "Enabled" } else { "Hidden" }),
        })
        .collect()
}

fn git_action_panels_from_daemon(
    view: &frame::GitActionScriptsView,
) -> Vec<SettingsGitActionPanel> {
    vec![
        git_action_panel_from_daemon(
            "commit",
            "Commit message instructions",
            "Paste commit generation instructions here.",
            &view.commit_script,
            view.commit_using_default,
        ),
        git_action_panel_from_daemon(
            "pull-request",
            "PR title/body instructions",
            "Paste PR title/body instructions here.",
            &view.pr_script,
            view.pr_using_default,
        ),
    ]
}

fn git_action_panel_from_daemon(
    id: &str,
    title: &str,
    placeholder: &str,
    script: &str,
    using_default: bool,
) -> SettingsGitActionPanel {
    SettingsGitActionPanel {
        id: shared(id),
        title: shared(title),
        detail: shared(if using_default {
            "Currently using the default built-in template."
        } else {
            "Currently using a custom persisted template."
        }),
        placeholder: shared(placeholder),
        script_preview: shared(script),
        custom: !using_default,
        action_enabled: !using_default,
        status: shared(if using_default { "Default" } else { "Custom" }),
    }
}

fn shortcut_rows_from_daemon(view: &frame::ShortcutSettingsView) -> Vec<SettingsShortcutRow> {
    view.actions
        .iter()
        .map(|row| {
            let binding = row.current_binding.trim();
            let default_binding = row.default_binding.trim();
            SettingsShortcutRow {
                id: shared(row.id.clone()),
                label: shared(row.label.clone()),
                binding: shared(binding),
                default_binding: shared(default_binding),
                capturing: false,
                status_detail: shortcut_status_detail(default_binding, false),
                reset_enabled: binding != default_binding,
                clear_enabled: !binding.is_empty(),
            }
        })
        .collect()
}

fn mcp_rows_from_daemon(view: &frame::McpSettingsView) -> Vec<SettingsMcpRow> {
    let catalog_ids = view
        .catalog_entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<HashSet<_>>();
    let mut rows = Vec::new();

    for entry in &view.catalog_entries {
        if let Some(server) = view
            .registry_entries
            .iter()
            .find(|server| server.id == entry.id)
        {
            rows.push(mcp_registry_row_from_daemon(
                server,
                &view.sync_error_provider_ids,
            ));
        } else {
            rows.push(SettingsMcpRow {
                id: shared(entry.id.clone()),
                label: shared(entry.label.clone()),
                detail: shared(entry.description.clone()),
                source: shared("catalog"),
                provider_summary: shared("Add before syncing"),
                installed: false,
                action_label: shared("Add"),
                action_enabled: true,
                status: shared("Catalog prompt"),
            });
        }
    }

    rows.extend(
        view.registry_entries
            .iter()
            .filter(|entry| !catalog_ids.contains(entry.id.as_str()))
            .map(|entry| mcp_registry_row_from_daemon(entry, &view.sync_error_provider_ids)),
    );

    rows
}

fn mcp_registry_row_from_daemon(
    row: &frame::McpServerDto,
    sync_error_provider_ids: &[String],
) -> SettingsMcpRow {
    let can_remove = !matches!(row.source, frame::McpSourceDto::BuiltInDaemon);
    let mut provider_summary = if row.enabled_for.is_empty() {
        "Not enabled".to_string()
    } else {
        row.enabled_for.join(" ")
    };
    if !sync_error_provider_ids.is_empty() {
        provider_summary.push_str(" · sync errors: ");
        provider_summary.push_str(&sync_error_provider_ids.join(" "));
    }

    SettingsMcpRow {
        id: shared(row.id.clone()),
        label: shared(row.label.clone()),
        detail: shared(format!("{}  ·  {}", mcp_source_label(row.source), row.id)),
        source: shared(mcp_source_label(row.source)),
        provider_summary: shared(provider_summary),
        installed: true,
        action_label: shared(if can_remove { "Remove" } else { "Built-in" }),
        action_enabled: can_remove,
        status: shared(
            if matches!(row.source, frame::McpSourceDto::BuiltInDaemon) {
                "Built-in daemon"
            } else {
                "Registry"
            },
        ),
    }
}

fn mcp_source_label(source: frame::McpSourceDto) -> &'static str {
    match source {
        frame::McpSourceDto::Catalog => "catalog",
        frame::McpSourceDto::Custom => "custom",
        frame::McpSourceDto::BuiltInDaemon => "daemon",
    }
}

fn settings_general_rows() -> Vec<SettingsGeneralRow> {
    vec![
        SettingsGeneralRow {
            id: shared("sidebar-git-metadata"),
            title: shared("Sidebar git metadata"),
            detail: shared("Show relative commit time and +/- line counts in task rows."),
            status: shared("Enabled"),
            status_detail: shared("Persists through ProjectStore in the GPUI baseline."),
            enabled: true,
            action_label: shared(""),
            action_enabled: false,
            secondary_action_label: shared(""),
            secondary_action_enabled: false,
            toggle_row: true,
        },
        SettingsGeneralRow {
            id: shared("build"),
            title: shared("Build"),
            detail: shared(format!(
                "Slint POC package v{}; GPUI also exposes short SHA, profile, cargo version, and copyable full SHA.",
                env!("CARGO_PKG_VERSION")
            )),
            status: shared("Static"),
            status_detail: shared("Updater identity projection remains app-level wiring."),
            enabled: false,
            action_label: shared("Copy SHA"),
            action_enabled: true,
            secondary_action_label: shared(""),
            secondary_action_enabled: false,
            toggle_row: false,
        },
        SettingsGeneralRow {
            id: shared("updates"),
            title: shared("Updates"),
            detail: shared("Check for updates and install a downloaded update when one is ready."),
            status: shared("Not checked"),
            status_detail: shared("Idle until updater state is projected into Slint."),
            enabled: false,
            action_label: shared("Check"),
            action_enabled: true,
            secondary_action_label: shared("Install"),
            secondary_action_enabled: false,
            toggle_row: false,
        },
    ]
}

fn settings_agent_rows() -> Vec<SettingsAgentRow> {
    vec![
        agent_row("claude-code", "Claude Code", "No extra args", true, false),
        agent_row("codex", "Codex", "No extra args", true, false),
        agent_row("cursor", "Cursor Agent", "No extra args", true, false),
        agent_row("gemini", "Gemini", "No extra args", true, false),
        agent_row("pi", "Pi", "No extra args", true, true),
        agent_row("opencode", "OpenCode", "No extra args", true, false),
        agent_row("amp", "Amp", "No extra args", true, false),
        agent_row("rovo-dev", "Rovo Dev", "No extra args", true, false),
        agent_row("forge", "Forge", "No extra args", true, false),
    ]
}

fn settings_open_in_rows() -> Vec<SettingsOpenInRow> {
    vec![
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
                "Open the project directory in your system file manager."
            },
        ),
    ]
}

fn settings_git_action_panels() -> Vec<SettingsGitActionPanel> {
    vec![
        SettingsGitActionPanel {
            id: shared("commit"),
            title: shared("Commit message instructions"),
            detail: shared("Currently using the default built-in template."),
            placeholder: shared("Paste commit generation instructions here."),
            script_preview: shared(DEFAULT_COMMIT_SCRIPT),
            custom: false,
            action_enabled: true,
            status: shared("Default"),
        },
        SettingsGitActionPanel {
            id: shared("pull-request"),
            title: shared("PR title/body instructions"),
            detail: shared("Currently using the default built-in template."),
            placeholder: shared("Paste PR title/body instructions here."),
            script_preview: shared(DEFAULT_PR_SCRIPT),
            custom: false,
            action_enabled: true,
            status: shared("Default"),
        },
    ]
}

fn settings_shortcut_rows() -> Vec<SettingsShortcutRow> {
    vec![
        shortcut_row(SETTINGS_SHORTCUT_IDS[0], "Cycle Projects", "Cmd-O"),
        shortcut_row(SETTINGS_SHORTCUT_IDS[1], "New Tab in Current Task", "Cmd-N"),
        shortcut_row(SETTINGS_SHORTCUT_IDS[2], "New Task", "Cmd-T"),
        shortcut_row(
            SETTINGS_SHORTCUT_IDS[3],
            "Close Current Tab",
            if cfg!(target_os = "macos") {
                "Cmd-W"
            } else {
                "Ctrl-W"
            },
        ),
        shortcut_row(SETTINGS_SHORTCUT_IDS[4], "Next Tab", "Cmd-Shift-]"),
        shortcut_row(SETTINGS_SHORTCUT_IDS[5], "Previous Tab", "Cmd-Shift-["),
        shortcut_row(SETTINGS_SHORTCUT_IDS[6], "Next Task", "Cmd-Alt-Down"),
        shortcut_row(SETTINGS_SHORTCUT_IDS[7], "Previous Task", "Cmd-Alt-Up"),
    ]
}

fn settings_mcp_rows() -> Vec<SettingsMcpRow> {
    vec![
        mcp_row(
            "context7",
            "Context7",
            "Catalog server for library docs and code examples.",
            "catalog",
            "Claude Cursor Codex",
            false,
            "Add",
            true,
            "Catalog prompt",
        ),
        mcp_row(
            "filesystem",
            "Filesystem",
            "Built-in daemon entry for local project file access.",
            "daemon",
            "Claude Cursor Codex Gemini",
            true,
            "Built-in",
            false,
            "Built-in daemon",
        ),
        mcp_row(
            "custom-json",
            "Custom entry (JSON)",
            "Custom transports, env, and headers are edited in ~/.config/another-one/mcp.json.",
            "custom",
            "Manual JSON",
            false,
            "Manual",
            false,
            "Config file",
        ),
    ]
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
        validation: shared("Arg tokens reject empty values and whitespace."),
        action_label: shared("Add"),
        action_enabled: false,
    }
}

fn open_in_row(id: &str, label: &str, detail: &str) -> SettingsOpenInRow {
    SettingsOpenInRow {
        id: shared(id),
        label: shared(label),
        detail: shared(detail),
        enabled: true,
        status: shared("Enabled"),
    }
}

fn shortcut_row(id: &str, label: &str, binding: &str) -> SettingsShortcutRow {
    SettingsShortcutRow {
        id: shared(id),
        label: shared(label),
        binding: shared(binding),
        default_binding: shared(binding),
        capturing: false,
        status_detail: shortcut_status_detail(binding, false),
        reset_enabled: true,
        clear_enabled: true,
    }
}

fn shortcut_status_detail(default_binding: &str, capturing: bool) -> SharedString {
    if capturing {
        return shared("Esc cancels. Delete clears.");
    }
    shared(format!("Default: {default_binding}"))
}

fn mcp_row(
    id: &str,
    label: &str,
    detail: &str,
    source: &str,
    provider_summary: &str,
    installed: bool,
    action_label: &str,
    action_enabled: bool,
    status: &str,
) -> SettingsMcpRow {
    SettingsMcpRow {
        id: shared(id),
        label: shared(label),
        detail: shared(detail),
        source: shared(source),
        provider_summary: shared(provider_summary),
        installed,
        action_label: shared(action_label),
        action_enabled,
        status: shared(status),
    }
}

fn show_settings_toast(app: &AppWindow, kind: &str, message: impl Into<SharedString>) {
    app.set_toast_kind(kind.into());
    app.set_toast_message(message.into());
    app.set_toast_detail("".into());
}

fn send_settings_request(
    settings_event_tx: &mpsc::UnboundedSender<SlintClientEvent>,
    request: Option<SettingsRequest>,
    app: &AppWindow,
) {
    let Some(request) = request else {
        return;
    };

    if settings_event_tx
        .send(SlintClientEvent::Settings(request))
        .is_err()
    {
        show_settings_toast(app, "warning", "Settings daemon channel is not available.");
    }
}

fn shared(value: impl Into<SharedString>) -> SharedString {
    value.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPUI_SETTINGS: &str = include_str!("../../desktop/src/settings_page.rs");
    const GPUI_MCP: &str = include_str!("../../desktop/src/mcp_page.rs");
    const CORE_AGENTS: &str = include_str!("../../core/src/agents.rs");
    const CORE_GIT_ACTIONS: &str = include_str!("../../core/src/git_actions.rs");
    const CORE_OPEN_IN: &str = include_str!("../../core/src/open_in.rs");
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
    fn settings_model_preserves_gpui_agent_labels() {
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
    }

    #[test]
    fn settings_model_preserves_gpui_shortcut_labels() {
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
    fn settings_model_preserves_open_in_inventory() {
        for id in ["cursor", "zed", "vscode", "file-manager"] {
            assert!(CORE_OPEN_IN.contains(&format!("\"{id}\"")));
            assert!(settings_open_in_rows()
                .iter()
                .any(|row| row.id.as_str() == id));
        }
    }

    #[test]
    fn git_action_defaults_match_core_source() {
        for fragment in [
            "Generate a git commit message for these staged changes.",
            "No markdown fences, no commentary, no quotes.",
            "Generate a GitHub pull request title and body for these branch changes.",
            "Keep the body skimmable and avoid filler.",
        ] {
            assert!(CORE_GIT_ACTIONS.contains(fragment));
        }
    }

    #[test]
    fn nav_selection_resets_shortcut_capture_state() {
        let mut state = SettingsState::baseline();

        state.handle_action("keybindings.capture", "new-task");
        state.select_section("agents");

        assert!(state.shortcut_rows.iter().all(|row| !row.capturing));
    }

    #[test]
    fn disabling_default_agent_reconciles_to_enabled_fallback() {
        let mut state = SettingsState::baseline();

        state.handle_toggle("agents", DEFAULT_AGENT_ID, false);

        assert!(state
            .agent_rows
            .iter()
            .any(|row| row.default_agent && row.enabled && row.id != DEFAULT_AGENT_ID));
    }

    #[test]
    fn keybinding_clear_and_reset_restore_default_binding() {
        let mut state = SettingsState::baseline();

        state.handle_action("keybindings.clear", "new-task");
        state.handle_action("keybindings.reset", "new-task");

        let row = state
            .shortcut_rows
            .iter()
            .find(|row| row.id == "new-task")
            .expect("new-task shortcut row");
        assert_eq!(row.binding, row.default_binding);
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

    #[test]
    fn mcp_builtin_daemon_row_is_not_removable() {
        let row = settings_mcp_rows()
            .into_iter()
            .find(|row| row.id == "filesystem")
            .expect("filesystem MCP row");

        assert!(!row.action_enabled);
    }

    #[test]
    fn daemon_agent_projection_marks_enabled_default_action_state() {
        let view = frame::AgentSettingsViewWire {
            agents: vec![
                frame::AgentSettingsRowWire {
                    id: "codex".to_string(),
                    label: "Codex".to_string(),
                    icon_path: "icons/codex.svg".to_string(),
                    provider: Some(frame::AgentProvider::Codex),
                    enabled: true,
                    is_default: false,
                    launch_args: vec!["--model".to_string(), "gpt".to_string()],
                },
                frame::AgentSettingsRowWire {
                    id: "pi".to_string(),
                    label: "Pi".to_string(),
                    icon_path: "icons/pi.svg".to_string(),
                    provider: Some(frame::AgentProvider::Pi),
                    enabled: true,
                    is_default: true,
                    launch_args: Vec::new(),
                },
            ],
            default_agent_id: Some("pi".to_string()),
        };

        let rows = agent_rows_from_daemon(&view);

        let codex = rows.iter().find(|row| row.id == "codex").unwrap();
        assert!(codex.action_enabled);
        assert_eq!(codex.args_label, "--model gpt");
        let pi = rows.iter().find(|row| row.id == "pi").unwrap();
        assert!(!pi.action_enabled);
        assert!(pi.default_agent);
    }

    #[test]
    fn daemon_mcp_projection_renders_catalog_before_custom_registry() {
        let view = frame::McpSettingsView {
            catalog_entries: vec![frame::McpCatalogEntryDto {
                id: "context7".to_string(),
                label: "Context7".to_string(),
                description: "Catalog docs server.".to_string(),
                docs_url: "https://example.test".to_string(),
            }],
            registry_entries: vec![frame::McpServerDto {
                id: "filesystem".to_string(),
                label: "Filesystem".to_string(),
                source: frame::McpSourceDto::BuiltInDaemon,
                transport_kind: frame::McpTransportKindDto::Stdio,
                enabled_for: vec!["codex".to_string()],
            }],
            sync_error_provider_ids: Vec::new(),
        };

        let rows = mcp_rows_from_daemon(&view);

        assert_eq!(rows[0].id, "context7");
        assert_eq!(rows[0].action_label, "Add");
        assert_eq!(rows[1].id, "filesystem");
        assert!(!rows[1].action_enabled);
    }
}
