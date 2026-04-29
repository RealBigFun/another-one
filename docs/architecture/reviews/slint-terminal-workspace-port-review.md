# Slint Terminal Workspace Port Review

Source of truth: `desktop/src/panels.rs`, `desktop/src/app.rs`, and `daemon-sandbox/src/frame.rs`.

## Section Relationships

- The workspace center is mutually exclusive with the project overview page. A project row opens `active_project_page`; a task row opens `active_section` and renders terminal workspace.
- Terminal workspace owns one tab bar and one active tab content panel. The tab bar is not part of the titlebar and must stay inside the center workspace region.
- Tab content has four GPUI modes: live terminal snapshot, launching card, lazy-restore card, failed-launch card, plus an empty-tabs card when the active section has no tabs.
- Terminal overlays are siblings above the shell: terminal tab menu, pinned-tab close confirmation, add-agent modal, and error toasts. They must not be folded into the terminal renderer.
- The terminal renderer owns grid painting, selection, links, cursor, keyboard focus, mouse protocol forwarding, and resize reporting.

## Data Ownership And Activation

- `WorkspacePane.section_states` owns per-section tabs, active tab index, pinned ordering, and cwd.
- `TerminalTab` owns title, provider launch config, pinned state, fixed title, and `TerminalRestoreStatus`.
- `AnotherOneApp` owns live runtimes, pending launches, terminal snapshots, recent output, launch errors, and process/resource tracking.
- The daemon wire projection is `ProjectSummary -> TaskSummary -> TabSummary`; `TabSummary` exposes `running`, `pinned`, `fixed_title`, `restore_status`, `failure_message`, and `failure_details`.
- Left-clicking a tab activates the tab. Right-clicking opens the terminal tab menu. The close icon requests close; pinned tabs require confirmation. The plus button opens Add Agent in GPUI.
- `Control::ActivateSectionTab`, `AddAgentToSection`, `CloseSectionTab`, and `ToggleSectionTabPinned` are the daemon-only control points Slint can use for tab state parity.

## Behavior And State Matrix

- Tab bar: normal, hover, active, pinned, provider icon, close-hover, right-click menu-open, overflow-scroll.
- Add tab: normal and hover; GPUI opens Add Agent modal with default-agent seeding.
- Active tab content: ready/live terminal, launching, lazy restore/not-started, failed with copy-details action, and no-tabs empty state.
- Terminal input: focus, key, paste, focus-reporting, mouse protocol, selection, link hover/open, scroll, and resize.
- Errors are user-facing and route through toast. Failed terminal content also renders a centered details card with copy action.

## Asset And Color Contract

- Tab provider icons use `desktop/assets/agent-icons/*` through GPUI `branded_icon`; plain shell uses `desktop/assets/icons/icons__terminal.svg`.
- Tab controls use `icons__pin-off.svg`, `icons__close.svg`, `icons__plus.svg`, and menu uses `icons__pin-off.svg`.
- Failed cards use copy affordance `icons__copy.svg`; empty/lazy/launching cards use `icons__terminal.svg`. Shell chrome controls use the matching GPUI SVG where one exists, including `icons__panel-left.svg`, `icons__panel-right.svg`, and `icons__settings.svg`.
- GPUI terminal workspace colors are feature-specific: tab bar `#27292e`, active tab/terminal `#1e1f22`, inactive tab/card `#2b2d31`, hover `#2f3136`, status card `#25282d`, border white `0.06` or `0.08`, active text `hsla(0,0,0.92,1)`, inactive tab/icon text `hsla(0,0,0.55,1)`.

## Slint Mapping

- `TerminalTabChip` must carry `provider`, `restore-status`, `failure-message`, and `failure-details` in addition to active/running/pinned.
- Slint should reuse GPUI SVG assets in `AoTabChip`, terminal status cards, and shell icon-only controls instead of text placeholders.
- Slint tab activation must keep using daemon `ActivateSectionTab` plus `LaunchTab`/`AttachTab`.
- Slint close/pin/add-tab actions should use daemon controls and refresh `ProjectList` from inline replies or follow-up list until a richer push model exists.
- Project overview placeholder must not be treated as terminal workspace.

## Deferred Gaps

- Full Add Agent modal parity belongs with modal/menu/popover flow work, but the terminal plus button must still have a deterministic behavior and not be decorative.
- Full right-click terminal tab menu positioning is modal/popover parity work; terminal tab pinning can be exposed through a direct affordance until the menu lands.
- Matched GPUI terminal-state captures are incomplete for some dynamic states; Slint fixture evidence is acceptable only when the gap is recorded.
