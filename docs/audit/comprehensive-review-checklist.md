# Comprehensive Review Checklist

Reference baseline:
- Original GPUI desktop source: commit `f656826fa9967a4ec99e2fe2e74f803ab5d7fe88` (`d481d0c^`, immediately before Phase 6 deleted `desktop/`)
- Original source root: `desktop/src/`
- Port/runtime roots: `another-one/lib/src/`, `another-one-bridge/src/`, `core/src/`, `daemon-sandbox/src/`
- Audit umbrella in beads: `another-one-6gc`
- Additional daemon-host parity bucket in beads: `another-one-6gc.23`

Current surfaced audit branches in beads:
- `another-one-6gc.1` app state machine
- `another-one-6gc.2` titlebar
- `another-one-6gc.3` left sidebar
- `another-one-6gc.4` right sidebar
- `another-one-6gc.5` terminal runtime
- `another-one-6gc.6` project page
- `another-one-6gc.7` settings page
- `another-one-6gc.8` new task modal
- `another-one-6gc.9` create branch modal
- `another-one-6gc.10` add agent modal
- `another-one-6gc.11` custom actions modal
- `another-one-6gc.12` MCP page
- `another-one-6gc.13` pair mobile
- `another-one-6gc.14` resource indicator
- `another-one-6gc.15` shortcuts / keybindings
- `another-one-6gc.16` open-in launcher
- `another-one-6gc.17` panels / multi-pane layout
- `another-one-6gc.18` security review
- `another-one-6gc.19` efficiency / performance
- `another-one-6gc.20` best practices
- `another-one-6gc.21` code quality
- `another-one-6gc.22` DRY principle
- `another-one-6gc.23` embedded daemon host
- `another-one-6gc.24` app bootstrap and entrypoint
- `another-one-6gc.25` MCP orchestrator
- `another-one-6gc.26` build info
- `another-one-6gc.27` layout primitives
- `another-one-6gc.28` theme contract
- `another-one-6gc.29` design tokens
- `another-one-6gc.30` bundled assets
- `another-one-6gc.31` agent icons
- `another-one-6gc.32` leakscope
- `another-one-6gc.33` platform abstraction
- `another-one-6gc.34` Linux platform integration
- `another-one-6gc.35` macOS platform integration
- `another-one-6gc.36` Windows platform assumptions
- `another-one-6gc.37` resource usage sampler

Every component audit must explicitly cover:
- [x] feature parity
- [x] security
- [x] efficiency
- [x] best practices
- [x] code quality
- [x] DRY
- [x] beads entry created for every actionable finding

## Component Audit Matrix

- [x] `desktop/src/main.rs` -> `another-one/lib/main.dart`, `scripts/dev-watch.sh`, packaging boot flow
- [x] `desktop/src/titlebar.rs` -> `another-one/lib/src/screens/desktop_titlebar/*`
- [x] `desktop/src/left_sidebar.rs` -> `another-one/lib/src/screens/desktop_sidebar/*`, `another-one/lib/src/projects_drawer_page.dart`
- [x] `desktop/src/right_sidebar.rs` -> `another-one/lib/src/screens/desktop_right_sidebar/desktop_right_sidebar.dart`, `another-one/lib/src/widgets/git_toolbar_button.dart`, `another-one/lib/src/widgets/toolbar_spinner.dart`
- [x] `desktop/src/panels.rs` -> `another-one/lib/src/screens/desktop_terminal/desktop_tab_strip.dart`, `another-one/lib/src/task_page.dart`
- [x] `desktop/src/new_task_modal.rs` -> `another-one/lib/src/screens/new_task/new_task_modal.dart`
- [x] `desktop/src/add_agent_modal.rs` -> `another-one/lib/src/screens/add_agent_modal/add_agent_modal.dart`
- [x] `desktop/src/custom_actions_modal.rs` -> `another-one/lib/src/screens/custom_action_modal/custom_action_modal.dart`
- [x] `desktop/src/create_branch_modal.rs` -> `another-one/lib/src/screens/create_branch/create_branch_modal.dart`
- [x] `desktop/src/pair_mobile.rs` -> `another-one/lib/src/screens/pair_mobile/pair_mobile_modal.dart`, `another-one-bridge/src/local_pair.rs`
- [x] `desktop/src/project_page.rs` -> `another-one/lib/src/screens/desktop_project_page/*`
- [x] `desktop/src/settings_page.rs` -> `another-one/lib/src/screens/settings_page/*`
- [x] `desktop/src/mcp_page.rs` -> `another-one/lib/src/screens/settings_page/sections/settings_mcp_section.dart`
- [x] `desktop/src/resource_indicator.rs` -> `another-one/lib/src/screens/desktop_titlebar/desktop_titlebar.dart`, `another-one/lib/src/state/resource_sample_provider.dart`
- [x] `desktop/src/resource_usage.rs` -> `core/src/resource_usage.rs`, resource-usage consumers in Flutter UI
- [x] `desktop/src/open_in.rs` -> `core/src/open_in.rs`, `another-one/lib/src/screens/desktop_titlebar/open_in_button.dart`, `another-one/lib/src/screens/settings_page/sections/settings_open_in_section.dart`
- [x] `desktop/src/shortcuts.rs` -> `core/src/shortcuts.rs`, `another-one/lib/src/screens/settings_page/sections/settings_keybindings_section.dart`
- [x] `desktop/src/terminal_runtime.rs` -> `core/src/terminal_manager.rs`, `core/src/terminal_engine/*`, `another-one/lib/src/screens/desktop_terminal/desktop_terminal_pane.dart`
- [x] `desktop/src/daemon_host.rs` -> `core/src/daemon_embed.rs`, `another-one-bridge/src/embedded_daemon.rs`, `another-one-bridge/src/pty_drain.rs`, `daemon-sandbox/src/registry.rs`
- [x] `desktop/src/mcp_orchestrator.rs` -> `core/src/mcp/orchestrator.rs`, `core/src/mcp/*`, `daemon-sandbox/src/transport_mcp.rs`
- [x] `desktop/src/build_info.rs` -> `another-one-bridge/src/api/build_info.rs`, `another-one/lib/src/state/build_info_provider.dart`
- [x] `desktop/src/layout.rs` -> `another-one/lib/src/tokens.dart`, `another-one/lib/src/layout/breakpoints.dart`, shell sizing/layout code
- [x] `desktop/src/theme.rs` -> `another-one/lib/src/theme.dart`, `another-one/lib/src/tokens.dart`
- [x] `desktop/src/tokens.rs` -> `another-one/lib/src/tokens.dart`
- [x] `desktop/src/assets.rs` -> `another-one/assets/**`, `another-one/lib/src/widgets/app_icon.dart`, `another-one/pubspec.yaml`
- [x] `desktop/src/agent_icons.rs` -> `another-one/lib/src/widgets/agent_provider_icon.dart`, `another-one/assets/agent-icons/**`
- [x] `desktop/src/app.rs` -> `another-one/lib/src/screens/desktop_shell.dart`, `another-one/lib/src/surface_router.dart`, `another-one/lib/src/state/*`, `core/src/section.rs`, `core/src/project_service.rs`, `core/src/git_service.rs`
- [x] `desktop/src/leakscope.rs` -> `core/src/leakscope.rs`
- [x] `desktop/src/platform/mod.rs` -> `core/src/platform/mod.rs`
- [x] `desktop/src/platform/linux.rs` -> `core/src/platform/linux.rs`, `another-one/linux/**`, `scripts/package-linux.sh`
- [x] `desktop/src/platform/macos.rs` -> `core/src/platform/macos.rs`, `another-one/macos/**`, `scripts/package-macos.sh`
- [x] `desktop/src/platform/windows.rs` -> `core/src/platform/windows.rs`, `another-one/windows/**` (historical/non-targeted; verify it does not break macOS/Linux assumptions)

## User-Facing Parity Checklist

### Titlebar
- [ ] sidebar toggle (left edge, 28x28, layout-split icon)
- [ ] build-identity chip (dev+dirty red, dev+clean amber, release subtle)
- [ ] build-identity tooltip surfaces profile, branch, sha, dirty state
- [ ] Custom Actions split-button primary action
- [ ] Custom Actions dropdown with global indicator, settings entry, divider, add-action footer
- [ ] Open In split-button primary action
- [ ] Open In dropdown with enabled apps in canonical order
- [ ] active-project GitHub button gated on `github.com` origin
- [ ] pull-request status pill with open/closed/merged states
- [ ] git-actions split-button with state-driven primary action
- [ ] git-actions dropdown rows: Commit, Commit & Push, Push, Force Push, Pull, Fetch, Undo Last Commit, Create PR, Create Draft PR
- [ ] active-action label flips while mutation runs
- [ ] danger tint on Force Push and Undo
- [ ] pair-mobile QR button
- [ ] resource indicator surfaces CPU and memory
- [ ] right-sidebar toggle

### Left Sidebar
- [ ] PROJECTS header
- [ ] project rows with avatar, name, chevron, ellipsis menu, GitHub link, plus button
- [ ] expand/collapse animation for grouped projects
- [ ] task rows with title, subtitle, and diff stats
- [ ] pinned tasks render with pin glyph
- [ ] worktree marker remains visible
- [ ] inline rename by double click
- [ ] context menu with pin/unpin, rename, delete
- [ ] delete confirmation dialog
- [ ] footer controls: settings and add-project

### Terminal Tab Strip And Pane
- [ ] every tab in active section rendered
- [ ] pinned tabs sort to head
- [ ] per-tab pin glyph, provider icon, title, duplicate-title suffix
- [ ] close button and pinned-tab close confirmation
- [ ] right-click pin/unpin menu
- [ ] add-agent plus button
- [ ] horizontal overflow scroll
- [ ] terminal attaches to live PTY bytes
- [ ] attach ignore window
- [ ] attach retry
- [ ] spinner failsafe
- [ ] error banner on connection failure or unpaired state
- [ ] CR/LF normalization for IME newline input
- [ ] terminal copy/paste still works
- [ ] pasted-image preview still works

### Right Sidebar
- [ ] tab strip with Changes, Commits, Checks, Compare
- [ ] Changes pane staged/uncommitted sections
- [ ] per-file stage/unstage/discard actions
- [ ] per-section stage all / unstage all actions
- [ ] discard confirmation dialog
- [ ] Commits pane subtitle and expandable rows
- [ ] commit rows surface file list
- [ ] load-more affordance
- [ ] Checks pane summary badges and sorted rows
- [ ] Compare pane header and read-only file rows
- [ ] first-commit Undo wiring honors active git action state

### Project Page
- [ ] header and Configuration collapsible
- [ ] default branch selector
- [ ] target branch selector
- [ ] Open PRs section filter tabs
- [ ] PR search field
- [ ] PR rows with number, CI icon, draft/review chip, metadata
- [ ] Review button creates a review task

### New Task Modal
- [ ] header and close affordance
- [ ] source branch picker with filter input
- [ ] New / Existing branch toggle in Worktree mode
- [ ] task-name field with generated placeholder
- [ ] agent multi-select chips plus Terminal sentinel
- [ ] Workspace toggle: Worktree vs Direct
- [ ] Direct-mode danger banner
- [ ] Advanced options collapsible
- [ ] Cancel and Create footer actions
- [ ] Esc dismisses
- [ ] validation copy matches original intent

### Add Agent Modal
- [ ] header and subtitle
- [ ] single-select agent picker with branded glyphs
- [ ] Terminal sentinel option
- [ ] help text flips between Terminal and agent-specific copy
- [ ] Cancel and Create actions
- [ ] Esc dismisses and Enter submits

### Custom Actions Modal
- [ ] Add vs Edit title/subtitle copy
- [ ] kind selector pill (Shell vs Agent)
- [ ] name field validation through toast
- [ ] icon picker with play/test/lint/configure/build/debug/agent choices
- [ ] shell command field and validation
- [ ] agent provider dropdown
- [ ] prompt textarea
- [ ] model, traits, mode, and access selectors
- [ ] run-on-worktree-create toggle
- [ ] global-action toggle
- [ ] Delete button on edit only
- [ ] Esc dismisses
- [ ] Cmd/Ctrl-Enter submits

### Create Branch Modal
- [ ] branch-name field with live slug preview
- [ ] Use current task toggle
- [ ] Migrate changes toggle
- [ ] Esc cancels
- [ ] Enter submits

### Pair Mobile Modal
- [ ] QR code rendered from pairing URL
- [ ] regenerate action

### Settings Shell
- [ ] left navigation rail
- [ ] active nav styling
- [ ] inactive hover styling
- [ ] back-to-app affordance

### Settings Agents
- [ ] header and explanatory copy
- [ ] availability summary panel
- [ ] per-agent branded rows
- [ ] launch-args input and add button
- [ ] arg pills / empty state
- [ ] Make-default / Default affordance
- [ ] Enabled / Disabled affordance

### Settings Open In
- [ ] header and detected-apps summary
- [ ] per-app toggle rows
- [ ] empty state copy

### Settings Git Actions
- [ ] commit-message panel
- [ ] PR title/body panel
- [ ] currently-using subtitle
- [ ] reset-to-default affordance
- [ ] scrolling mono editor
- [ ] debounced save

### Settings Keybindings
- [ ] header and explanatory copy
- [ ] per-action rows with binding pills
- [ ] capture mode prompt
- [ ] Esc cancels capture
- [ ] clear action
- [ ] reset action

### Settings MCP
- [ ] header and instructions copy
- [ ] catalog rows with Add action and spinner
- [ ] registry rows with provider toggles
- [ ] Codex toggle gated off for HTTP entries
- [ ] Remove action hidden for built-in daemon rows
- [ ] footer note about MCP config path

### Resource Usage And Feedback
- [ ] titlebar resource chip remains visible and accurate
- [ ] detailed resource usage surface exists or equivalent workflow is documented
- [ ] manual refresh exists where the original app exposed it
- [ ] resource tree groups by project
- [ ] resource tree groups by task
- [ ] resource tree groups by session
- [ ] tooltips remain on interactive controls
- [ ] toast notifications remain the canonical user-visible feedback channel

### Cross-Cutting Persistence
- [ ] active section persists across launches
- [ ] left sidebar open state persists
- [ ] right sidebar open state persists
- [ ] terminal tabs restore across launches
- [ ] branch settings restore correctly
- [ ] shortcut settings persist and restore
- [ ] Open In settings persist and restore

## Technical Capability Checklist

### App Architecture
- [ ] shared app state still coordinates workspace, projects, terminals, and chrome state coherently
- [ ] GPUI desktop responsibilities have clear Flutter or Rust analogs
- [ ] cross-platform boot remains thin and shared logic stays in shared layers

### Persistence
- [ ] local project store persists projects and tasks
- [ ] branch settings persist
- [ ] terminal tab state persists
- [ ] shortcut settings persist
- [ ] Open In configuration persists
- [ ] repo default commit action persists

### Git Integration
- [ ] repository detection and worktree grouping remain intact
- [ ] ahead/behind metadata remains available
- [ ] changed file state remains available
- [ ] recent commits remain available
- [ ] branch compare state remains available
- [ ] titlebar and sidebar git mutations route through a shared implementation

### GitHub Integration
- [ ] GitHub remote detection and URL normalization remain intact
- [ ] active-branch PR lookup remains intact
- [ ] CI/check-run lookup remains intact
- [ ] project-page PR listing remains intact
- [ ] metadata refresh and cache TTL logic remain sane

### Agent Launch And Session Infrastructure
- [ ] multi-provider catalog remains complete
- [ ] provider launch configuration mapping remains shared
- [ ] raw shell mode remains available
- [ ] terminal session restoration still works
- [ ] warm-launch infrastructure remains bounded and cancellable

### Terminal Runtime
- [ ] PTY-backed runtime remains stable
- [ ] output buffering and trimming remain bounded
- [ ] terminal title updates remain wired through
- [ ] resize propagation remains correct
- [ ] runtime cleanup happens when tabs close

### Resource Sampling
- [ ] CPU sampling remains accurate enough for UI use
- [ ] memory sampling remains accurate enough for UI use
- [ ] process-tree aggregation still groups by app, project, task, session

### Async And Background Work
- [ ] background refresh work is cancellable or intentionally detached
- [ ] polling intervals are still appropriate for active and idle states
- [ ] no UI-critical refresh path depends on dropped tasks or silent error swallowing

### Notifications And Feedback
- [ ] toast stack limits still prevent unbounded growth
- [ ] error lifetimes and dismissal affordances still behave predictably
- [ ] clipboard and pasted-image feedback still expire correctly

### Platform Integration
- [ ] macOS-specific behavior is preserved
- [ ] Linux-specific behavior is preserved
- [ ] Windows code paths do not regress shared macOS/Linux assumptions
- [ ] external URL opening remains routed through platform abstractions
- [ ] external editor/file-manager launching remains routed through platform abstractions

### MCP And Transport
- [ ] MCP orchestration remains shared and transport-agnostic
- [ ] built-in daemon vs registry-backed MCP rows behave consistently
- [ ] transport surfaces do not fork feature behavior between local and remote modes

### Observability And Safety
- [ ] build-info and debug markers remain trustworthy
- [ ] leak or runaway-session detection still has an analog where required
- [ ] PTY and daemon lifecycle code still cleans up on shutdown paths

## Findings Log

Use this section during the audit:
- [ ] Every actionable finding has a beads item
- [ ] Every beads item names the original component and the current analog
- [ ] Every parity gap is tagged distinctly from pure code-quality work
- [ ] Every security finding includes impact and exploit preconditions
- [ ] Every efficiency finding includes the hot path or boundedness concern
- [ ] Every best-practice / DRY finding includes the duplicated or inconsistent code path
