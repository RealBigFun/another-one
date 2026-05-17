# Technical Feature Inventory

This document lists the internal capabilities, platform features, and system behaviors implemented in `another-one` that support the user-facing product.

## 1. Application Architecture

### 1.1 Desktop application stack
- Native desktop application implemented in Rust.
- UI implemented with Zed's GPUI framework.
- Modular codebase split into focused files for app state, sidebars, modals, terminals, git integration, settings, and platform services.

### 1.2 Internal app structure
- Central app state object for workspace, project, terminal, and UI coordination.
- Dedicated modules for project pages, left sidebar, right sidebar, layout, titlebar, settings, and modals.
- Shared theme and asset loading system.

## 2. Persistence and Stored State

### 2.1 Local persistence
- Persistent local project store.
- Serialized project and task data using serde.
- Persisted branch settings.
- Persisted terminal tab state.
- Persisted shortcut settings.
- Persisted Open In configuration.
- Persisted repo default commit-action preferences.

### 2.2 State restoration
- Restore terminal tabs for sections/tasks.
- Restore task and section state on app reopen.
- Restore branch-related preferences and settings.

## 3. Git Integration

### 3.1 Repository awareness
- Detect and manage git repositories.
- Detect worktree relationships.
- Track repo common directory relationships for grouping.
- Identify default branches and current branches.

### 3.2 Branch metadata and comparisons
- Track ahead/behind counts.
- Track changed file state.
- Track recent commits.
- Track branch comparison state.
- Resolve branch settings for each project.

### 3.3 Git action execution
- Execute git actions for commit, push, pull, fetch, and force push flows.
- Execute staging and unstaging actions for files or groups.
- Support PR-related git flows such as branch-based PR creation.

## 4. GitHub Integration

### 4.1 GitHub metadata lookup
- Detect GitHub repository remotes and normalize GitHub links.
- Fetch GitHub repository link information for projects.
- Fetch pull request metadata for active branches.
- Fetch check run / CI status metadata.

### 4.2 Background GitHub caching
- Cache pull request lookups.
- Cache check-run lookups.
- Refresh cached PR/check state on intervals.

## 5. Agent Launch and Session Infrastructure

### 5.1 Multi-provider agent model
- Internal provider abstraction for multiple coding agents.
- Agent catalog with icons, labels, and provider identifiers.
- Provider-to-launch configuration mapping.

### 5.2 Launch configuration
- Raw shell launch mode.
- Agent launch mode.
- Terminal session reference support for resumable sessions.
- Session kind modeling for providers such as Claude, Cursor, Codex, and Pi.
- Optional home directory override per terminal launch.

### 5.3 Warm launch infrastructure
- Hidden terminal prewarming for modal-driven task creation.
- Warm launch reservation for Add Agent modal.
- Warm launch reservation for New Task modal.
- Cancelation of prewarmed launches when modal state changes.

## 6. Terminal Runtime

### 6.1 PTY and terminal emulation
- PTY owned and managed by the embedded daemon; raw byte streams delivered to the app over iroh QUIC.
- Terminal emulation via alacritty_terminal.
- Live terminal runtime abstraction.
- Terminal grid sizing and snapshot generation.

### 6.2 Terminal interaction support
- Output buffering and trimming.
- Terminal title updates.
- Terminal resize propagation.
- Selection state tracking.
- Clipboard integration.
- Input forwarding from UI to terminal runtime.

### 6.3 Session/process management
- Child process tracking for active terminal tabs.
- Restore-state tracking for terminal sessions.
- Runtime cleanup when tabs close.

## 7. Resource Usage Sampling

### 7.1 Resource monitoring
- CPU sampling for app and terminal processes.
- Memory sampling for app and terminal processes.
- Tracking of process trees related to active sessions.

### 7.2 Aggregation
- Aggregate resource usage by app shell.
- Aggregate resource usage by project.
- Aggregate resource usage by task.
- Aggregate resource usage by session.

## 8. Background Work and Async Coordination

### 8.1 Background job channels
- Channel-based background communication using mpsc.
- Background refresh workers for git state.
- Background PR lookups.
- Background check-run lookups.
- Background commit file-change lookups.
- Background terminal warm-launch work.

### 8.2 Refresh scheduling
- Active git status refresh intervals.
- Active metadata refresh intervals.
- Idle refresh intervals.
- Resource refresh intervals for open and closed states.
- Toast animation refresh intervals.

## 9. UI System Behaviors

### 9.1 Focus and interaction management
- Focus management across panes and modals.
- Dropdown state handling.
- Modal overlay lifecycle handling.
- Tooltip rendering helpers.

### 9.2 Notification infrastructure
- Toast stack with limits.
- Toast animation timing.
- Error toast lifetimes.
- Swipe-dismiss support for toasts.
- Clipboard copy feedback state.
- Temporary pasted-image preview state.

### 9.3 Layout and navigation
- Keyboard navigation actions for tabs, tasks, and projects.
- Global zoom bindings.
- Section-level state keyed by project/branch/task identities.

## 10. Platform Integration

### 10.1 Cross-platform abstraction
- Platform services abstraction layer.
- Platform-specific titlebar handling.
- Platform-specific window decoration handling.
- Platform-specific default keybinding behavior.

### 10.2 macOS-specific support
- Dock icon handling on macOS.
- Cocoa/Objective-C integration on macOS.

### 10.3 External app and URL integration
- External URL opening support.
- External editor/app launching support.
- System file manager launching support.

## 11. Assets and Presentation Infrastructure

### 11.1 Asset packaging
- Bundled project assets root.
- SVG and raster icon asset usage.
- Branded agent icon support.

### 11.2 Font support
- Bundled Lilex Nerd Font Mono loading at startup.
- Runtime registration of local font assets without system installation.

## 12. Tooling and Developer Workflow

### 12.1 Build and dev scripts
- Development watch scripts.
- Production build-and-open script.
- App icon rendering script.

### 12.2 Agent workflow hooks
- Codex session start hook support.
- Pi session start extension support.
- Example hook configuration docs.
- MCP server configuration per agent (add, toggle, remove servers from Settings).
