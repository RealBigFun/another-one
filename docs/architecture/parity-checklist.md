# GPUI → Flutter Parity Checklist

Per-screen parity tracker for the Phase 5 verification gate.
Each row captures one named surface from the original GPUI
desktop and the Flutter port that replaced it. Use the boxes
to track sign-off — set `[x]` once a reviewer has eyeballed
the live Flutter app and confirmed the listed behaviour matches
GPUI's contract.

> The other Phase 5 layers (golden screenshots, scripted
> clickthrough, PTY byte-replay, latency benchmarks, side-by-
> side staging build) all required the GPUI binary to build for
> comparison. Phase 6 deleted `desktop/`, so those layers need
> a Flutter-only redesign before they're useful — captured in
> [[#Deferred verification machinery]] below.

## Top-level chrome

### Titlebar
File: `another-one/lib/src/screens/desktop_titlebar/desktop_titlebar.dart`
- [ ] sidebar toggle (left edge, 28×28, layout-split icon)
- [ ] build-identity chip (dev+dirty=red, dev+clean=amber, release=subtle), tooltip surfaces profile/branch/sha/dirty
- [ ] Custom Actions split-button (148w × 28h, primary half runs last-used or opens modal, chevron toggles dropdown)
- [ ] Custom Actions dropdown (260w, per-row globe glyph for global, settings cog, divider, "Add action" footer)
- [ ] Open In split-button (114w, primary → preferred app, chevron → enabled-apps dropdown)
- [ ] active-project GitHub button (gates on `origin = github.com`)
- [ ] pull-request status pill (state hue: open green / closed grey / merged purple)
- [ ] git-actions split-button (156w, primary auto-picks Commit / Push / Pull / Fetch by state, chevron → menu)
- [ ] git-actions dropdown rows: Commit, Commit & Push, Push, Force Push, Pull, Fetch, Undo Last Commit, Create PR, Create Draft PR
- [ ] active-action label flips during run (`Committing…`, `Force Pushing…`)
- [ ] danger tint on Force Push / Undo
- [ ] pair-mobile QR button (28×28)
- [ ] resource indicator (CPU% | Mem MiB, em-dashes until first sample)
- [ ] right-sidebar toggle

### Left sidebar
File: `another-one/lib/src/screens/desktop_sidebar/desktop_sidebar.dart` + `project_row.dart` + `task_row.dart`
- [ ] PROJECTS small-caps header
- [ ] project rows with avatar, name, chevron, ellipsis menu, github link, plus button
- [ ] expand/collapse animation per project
- [ ] task rows: name + subtitle (last-commit-relative) + diff stats (+N -N)
- [ ] pinned tasks render with pin glyph
- [ ] worktree marker (always shown for worktree tasks)
- [ ] inline rename (double-click)
- [ ] right-click context menu (pin/unpin, rename, delete)
- [ ] delete confirmation dialog
- [ ] footer: settings gear + add-project (folder-plus)

### Right sidebar
File: `another-one/lib/src/screens/desktop_right_sidebar/desktop_right_sidebar.dart`
- [ ] tab strip with Changes / Commits / Checks / Compare (Compare gated on target branch presence)
- [ ] Changes pane: Staged / Uncommitted sections, per-file stage / unstage / discard, per-section actions
- [ ] discard-confirm dialog (320w, Esc cancels, Enter confirms)
- [ ] Commits pane: header subtitle, expandable rows showing per-commit file list, "Load more" pill
- [ ] Checks pane: summary badges (passed/failed/pending/skipped) + sorted rows
- [ ] Compare pane: header copy "Comparing {current} against {target}" + read-only file rows
- [ ] first-commit Undo button reads activeGitActionProvider

### Tab strip
File: `another-one/lib/src/screens/desktop_terminal/desktop_tab_strip.dart`
- [ ] every tab in the active section rendered (not just selected)
- [ ] pinned tabs sort to head
- [ ] active tab = `terminalBg`, inactive = `cardBg`, hover = `#2F3136`
- [ ] per-tab pin glyph + provider icon + title (with " 2"/" 3" suffix when multi-tab)
- [ ] close button (18×18, hover white@0.08)
- [ ] right-click → Pin/Unpin context menu
- [ ] pinned-tab close confirmation (364w)
- [ ] add-agent "+" button at strip end
- [ ] horizontal overflow scroll

### Terminal pane
File: `another-one/lib/src/screens/desktop_terminal/desktop_terminal_pane.dart`
- [ ] xterm.dart receives PTY bytes from `attachTab` broadcast
- [ ] 200ms ignore window after attach
- [ ] 400ms attach retry
- [ ] 1.5s spinner failsafe
- [ ] error banner on `TransportStatus.error` / `unpaired`
- [ ] CR/LF normalisation: IME `\n` → `\r`

### Project page
File: `another-one/lib/src/screens/desktop_project_page/`
- [ ] header + Configuration collapsible (default branch + target branch dropdowns)
- [ ] Open PRs section (filter tabs, search, PR rows with #N + CI icon + Draft/Review chip)
- [ ] Review button per PR row → `createReviewTask`

## Modals

### New Task
File: `another-one/lib/src/screens/new_task/new_task_modal.dart`
- [ ] header "New task" + project subtitle, X close
- [ ] source-branch picker: dropdown trigger + filter input + branch list
- [ ] New / Existing branch toggle (only in Worktree mode)
- [ ] task-name field with generated-name placeholder (cosmic-river-zooms generator)
- [ ] agent multi-select chips + Terminal sentinel (clears all)
- [ ] Workspace toggle Worktree | Direct
- [ ] danger banner under Direct: "Direct uses the branch already checked out…"
- [ ] Advanced options collapsible (GitHub + Jira issue placeholder pickers)
- [ ] footer Cancel + Create
- [ ] Esc dismisses, sanitises validation copy ("Source branch is required" etc.)

### Add Agent
File: `another-one/lib/src/screens/add_agent_modal/add_agent_modal.dart`
- [ ] header "Add Agent to Task" + subtitle
- [ ] single-select agent picker w/ branded glyphs + radio circle
- [ ] Terminal sentinel option (CLI Only)
- [ ] help text flips between Terminal / agent-specific copy
- [ ] footer Cancel (border) + Create (white button)
- [ ] Esc dismisses, Enter submits

### Custom Action editor
File: `another-one/lib/src/screens/custom_action_modal/custom_action_modal.dart`
- [ ] header dynamic title ("Add Action" / "Edit Action") + subtitle
- [ ] kind selector pill (Shell | Agent)
- [ ] name field (required, "Custom actions need a name." toast)
- [ ] icon picker (7 buttons: play, test, lint, configure, build, debug, agent)
- [ ] Shell branch: command field + validation
- [ ] Agent branch: provider dropdown (branded), prompt textarea (190h), model + traits + mode + access dropdowns
- [ ] toggles: Run on worktree create, Global action
- [ ] footer: Delete (red, only on Edit) + Cancel + Save
- [ ] Esc dismisses, Cmd/Ctrl-Enter submits

### Create Branch
File: `another-one/lib/src/screens/create_branch/create_branch_modal.dart`
- [ ] branch-name field with live slug preview
- [ ] "Use current task" toggle (forces "Migrate changes" on)
- [ ] "Migrate changes" toggle
- [ ] Esc cancels, Enter submits

### Pinned Tab Close confirm
File: `another-one/lib/src/screens/desktop_terminal/desktop_tab_strip.dart` (`_showPinnedTabCloseConfirm`)
- [ ] 364w card with "Close pinned tab \"{title}\"?" message
- [ ] Cancel + Close (danger-red) buttons

### Pair Mobile
File: `another-one/lib/src/screens/pair_mobile/pair_mobile_modal.dart`
- [ ] QR code rendered from bridge `pairing_url`
- [ ] regenerate button

## Settings page

File: `another-one/lib/src/screens/settings_page/settings_page.dart` (shell) + `sections/*.dart`

### Shell
- [ ] 180w left rail with Back-to-app chevron + 5 nav items
- [ ] active item highlighted `#2E5DC2`, inactive hovers white@0.06

### Agents
- [ ] header + paragraph + Availability panel ("N enabled")
- [ ] per-agent rows: branded icon, label, description
- [ ] launch-args input (180×34, mono font, "--flag value" hint) + Add button
- [ ] arg pills (mono font, x-button per pill) or "No extra args"
- [ ] Make-default / Default radio pill (filled blue when default)
- [ ] Enabled / Disabled checkbox pill (filled blue when enabled)

### Open In
- [ ] header + Detected-apps panel ("N enabled")
- [ ] per-app rows (clickable to toggle): icon + label + description + Enabled/Disabled label + checkbox
- [ ] empty state: "Install Cursor, Zed, VS Code, or use your system file manager…"

### Git Actions
- [ ] two stacked panels: Commit message + PR title/body
- [ ] header per panel: title + "Currently using…" subtitle + Reset to Default pill
- [ ] body: 280-480px scrolling code editor with mono font
- [ ] debounced save (500ms after last keystroke)

### Keybindings
- [ ] header + paragraph
- [ ] per-action rows: label + binding pills (with ⌘ ⌃ ⌥ ⇧ glyphs)
- [ ] click to start capture: "Press a key…" with blue accent
- [ ] Esc cancels capture
- [ ] × clear button (only when bound)
- [ ] ↺ reset button (dimmed when at default)

### MCP
- [ ] header + paragraph (with instructions copy)
- [ ] catalog rows: prompt with "Add" button (busy spinner)
- [ ] registry rows: label + "{source} · {id}" subtitle + 6 provider toggles
- [ ] Codex toggle gated off + tooltip for HTTP entries
- [ ] Remove button (hidden for BuiltInDaemon source)
- [ ] footer note about ~/.config/another-one/mcp.json

## Cross-cutting

- [ ] toast surface for errors / info / success (ScaffoldMessenger pattern)
- [ ] active-section persistence across launches (selectedTabProvider via SharedPreferences)
- [ ] left/right sidebar open state persists
- [ ] light/dark theme — currently dark only (no GPUI light theme either)

## Deferred verification machinery

The other five Phase 5 layers required GPUI to build. With
`desktop/` deleted in Phase 6, they need a Flutter-only
redesign:

1. **Golden-screenshot regression** — needs a Flutter
   `integration_test` harness that boots against a deterministic
   seeded `RegistryState` and dumps PNGs. The "diff against
   GPUI" half is now historical — file the GPUI screenshots
   from the last working commit if a baseline is needed.
2. **Scripted clickthrough** — same shape as #1 but stepping
   through user flows. Was supposed to capture a transcript
   from GPUI; now needs hand-written assertions.
3. **PTY byte-replay** — record bytes once, replay through
   xterm.dart, assert final visible cells match a hand-checked
   expected snapshot. The "match alacritty" half is gone.
4. **Latency benchmarks** — Flutter has its own profiling
   tools (DevTools, `flutter run --profile`). Define keystroke-
   to-render and project-tree-refresh harnesses against those.
5. **Side-by-side staging build** — non-applicable post Phase 6
   (no GPUI to render alongside). Replaced by the parity
   checklist above.
