# Slint Settings Port Review

Source of truth: `desktop/src/settings_page.rs`, `desktop/src/mcp_page.rs`, `desktop/src/app.rs`, `core/src/agents.rs`, `core/src/open_in.rs`, `core/src/shortcuts.rs`, `core/src/git_actions.rs`, and `slint-poc/ui/components.slint`.

## Source Inventory

- GPUI rendering owner is `AnotherOneApp::render_settings_page` in `desktop/src/settings_page.rs`.
- Settings navigation is `SettingsSection` with six labels in this order: `General`, `Agents`, `Open In`, `Git Actions`, `Keybindings`, `MCP`.
- GPUI page state lives in `AnotherOneApp`: `settings_open`, `settings_section`, `shortcut_capture_action`, `settings_agent_input`, git-action script drafts/layouts, `available_open_in_apps`, `mcp_registry`, and `mcp_last_sync_errors`.
- General settings read updater identity/state, sidebar metadata visibility, and updater commands from `desktop/src/app.rs` / `desktop/src/updater.rs`.
- Agent settings render the canonical `AGENTS` list from `core/src/agents.rs` and mutate per-agent enabled/default/argv settings through project-store helpers.
- Open In settings render detected `OpenInAppKind` rows from `core/src/open_in.rs` in the desktop-detected availability set.
- Keybindings render `ALL_SHORTCUT_ACTIONS` from `core/src/shortcuts.rs`; capture remains GPUI-event-coupled in `desktop/src/shortcuts.rs`.
- MCP settings render catalog/registry rows from `desktop/src/mcp_page.rs` and sync provider toggles through the MCP registry.

## Section Relationships

- The settings page is a full-window replacement surface with a fixed left sidebar and one active content section.
- Sidebar activation resets transient section state: shortcut capture, focused agent input, git-action focused editors, and drag anchors.
- `General` is app/build/update scoped and includes sidebar metadata visibility.
- `Agents` is global agent-launch configuration: availability, default agent, and per-agent argv tokens. Disabled agents remain visible in settings but are hidden from New Task/Add Agent pickers.
- `Open In` is platform availability scoped: only detected apps render, and the titlebar Open In menu consumes the enabled subset.
- `Git Actions` owns two sibling script editors, one for commit generation and one for PR title/body generation.
- `Keybindings` is command scoped and captures one shortcut row at a time.
- `MCP` is registry scoped and rows can be catalog prompts, registry entries, or custom entries.

## Data Ownership

- GPUI persists settings through `ProjectStore` for UI preferences, shortcuts, open-in selections, agent settings, and git-action scripts.
- MCP is not project-store owned; `McpRegistry` is the canonical source and `sync_all()` pushes enabled server state to each provider's native config.
- Slint first slice adds typed view models in `slint-poc/ui/settings.slint` and seeds them from `slint-poc/src/settings.rs`.
- Current Slint data is intentionally model-backed/static because daemon settings controls are not yet projected into the Slint client protocol.
- User-facing setting action/toggle callbacks in the Slint slice route through `AoToast` by setting the app toast properties from Rust.

## Behavior States

- Sidebar nav states: normal, hover, active, and keyboard activation through a focus scope.
- General row states: static status, enabled action, disabled action.
- Agent row states: enabled/disabled, default/not-default, argv-token summary.
- Open In row states: enabled/disabled and detected-app summary.
- Git Actions panel states: default/custom template and reset action.
- Keybinding row states: normal binding display, listening/capture display, edit/reset/clear actions.
- MCP row states: installed/add prompt, provider summary, remove/add action.
- Deferred behavior: real mutation persistence, text editing/capture logic, MCP provider-column error state, exact scroll virtualization, and visual-diff captures.

## Slint Mapping

- `SettingsView` in `slint-poc/ui/settings.slint` maps the GPUI full-window settings surface to a Slint sidebar plus mutually-exclusive content body.
- Typed models are `SettingsNavEntry`, `SettingsGeneralRow`, `SettingsAgentRow`, `SettingsOpenInRow`, `SettingsGitActionPanel`, `SettingsShortcutRow`, and `SettingsMcpRow`.
- `SettingsView` composes existing Slint base controls: `AoButton`, `AoCheckbox`, `AoSectionLabel`, and `AoStatusPill`.
- `AppWindow` owns `settings_open`, `settings_active_section`, and the typed settings row models.
- The footer settings icon opens the Slint settings surface; `Back to app` closes it without touching daemon or GPUI state.
- `settings::seed_settings_model` preserves GPUI labels and section relationships for the first production slice.

## Verification

- Source-contract assertions in `slint-poc/src/settings.rs` compare Slint settings labels against GPUI/core sources.
- Required commands for this slice:
  - `cargo fmt -p slint-poc`
  - `cargo check -p slint-poc`
  - `cargo test -p slint-poc --lib`
- Visual-fidelity captures remain required before parity closure: `settings/general.png`, `settings/agents.png`, `settings/open-in.png`, `settings/git-actions.png`, `settings/keybindings.png`, and `settings/mcp.png`.
