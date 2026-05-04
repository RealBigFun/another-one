# GUI / Control / MCP unification

> Every action a desktop user can take is also reachable over MCP.
> The GUI doesn't have privileged shortcuts; a button click and a
> `tools/call` go through the same dispatcher.

#pattern · #rust · #mcp · #gui

## The three buckets

A discovery sweep (`another-one-9jw`) audited every GUI action and
classified it into one of three categories. Pick the right bucket
when adding a new action:

### 1. Daemon-bound state mutation

Anything that touches `ProjectStore`, registry tabs, git state,
agent settings, etc. **Lives as a `daemon_proto::Control` variant.**
Both the GUI and MCP construct the same variant and dispatch it via
`daemon::dispatch::dispatch_call(ctrl, registry, peer_id) ->
WorkerReply`. One execution path; transport doesn't matter.

Examples (already in `Control`):
`AddProject`, `RemoveProject`, `SubmitNewTask`, `StageChangedFile`,
`UnstageChangedFile`, `DiscardChangedFile`, `ListProjects`,
`ListProjectActions`, `SaveProjectAction`, `ReadActiveGitState`, etc.

**MCP tool surface for these is auto-derivable** (tracked under
`another-one-vkq`): one tool per `Control` variant, schema from
`schemars`, handler deserializes args and dispatches. New verb =
new tool, no manual sync.

### 2. Desktop-only ephemera

Anything that flips transient GUI state with no remote analogue —
overlays, focus moves, zoom, panel toggles, modal open/close, scroll
position. **Lives as a `another_one_core::mcp::orchestrator::UiAction`
variant.** The GUI's existing GPUI Action handlers construct the
matching `UiAction` and call `AnotherOneApp::dispatch_ui_action`.
MCP routes through the `dispatch_ui_action` MCP tool to the same
method via the `pending_ui_actions` queue + render-tick drain.

Examples:
`OpenPairMobile`, `ClosePairMobile` (today). Future:
`ZoomIn`/`ZoomOut`/`ZoomReset`, `NextTab`/`PreviousTab`,
`OpenTerminalSearch`, `CloseTerminalSearch`, `OpenSettings`,
`ToggleSidebar`, etc.

### 3. Pure UI plumbing

Anything that's strictly internal to one render cycle — hover
state, drag state, momentary visual feedback, animation tick. These
**don't get exposed**. They're not user-meaningful actions; they're
implementation details of how the GPUI tree updates between frames.

## How a new action lands

```text
                  ┌─────────────────────┐
                  │ "user can do this" │
                  └──────────┬──────────┘
                             │
       ┌─────────────────────┼─────────────────────┐
       │                     │                     │
 ┌──────────────┐    ┌───────────────┐    ┌────────────────┐
 │ touches      │    │ flips         │    │ pure UI        │
 │ persistent / │    │ desktop-only  │    │ plumbing       │
 │ daemon state │    │ ephemera      │    │ (hover, scroll)│
 └──────┬───────┘    └───────┬───────┘    └────────┬───────┘
        ▼                    ▼                     ▼
 daemon_proto::    another_one_core::      no abstraction
 Control variant   mcp::orchestrator::     needed
        +          UiAction variant
 dispatch_call         +
 arm in            dispatch_ui_action
 daemon::dispatch  arm in app::AnotherOneApp
        +                +
 MCP tool entry    MCP tool dispatch via
 (auto today;      `dispatch_ui_action` tool
 hand-written     (single tool that takes a
 entries override  UiAction-shaped args object)
 the auto-default)
```

### Bucket 1 (daemon verb) — checklist

1. Add a variant to `daemon_proto::Control`. Use `serde(rename_all =
   "snake_case")` field naming — that's what the auto-derived MCP
   tool will reflect.
2. Add a match arm in `daemon::dispatch::dispatch_call`. Returns a
   `WorkerReply` (success variant or `Err { kind, message }`).
3. Refactor the GUI handler to construct the `Control` and call
   into the in-process equivalent of `dispatch_call`. After
   `another-one-vkq` lands, MCP exposure is automatic.
4. Test via the in-memory transport pair (`daemon-transport::in_memory::pair`)
   so the verb has end-to-end coverage without iroh.

### Bucket 2 (UI ephemera) — checklist

1. Add a variant to `another_one_core::mcp::orchestrator::UiAction`.
   Use `serde(tag = "kind", rename_all = "snake_case")` so the wire
   shape is `{"kind": "open_pair_mobile"}`.
2. Add a match arm in `AnotherOneApp::dispatch_ui_action`.
3. Refactor any GPUI Action handler that does the same thing today
   to construct the `UiAction` and call `dispatch_ui_action`. The
   MCP tool (`dispatch_ui_action` in `core/src/mcp/tools.rs`)
   automatically routes here via the orchestrator trait method.
4. Document the new variant in this doc's "Examples" section so
   future contributors don't reinvent it.

## Why two buckets, not one

Squeezing UI ephemera into `Control` would force every overlay
toggle through the daemon — adding wire traffic, schema noise, and
a round-trip the desktop doesn't need. Squeezing daemon verbs into
`UiAction` would lose the MCP/mobile/in-memory transport story
entirely. The split is honest about which actions need to be visible
to remote clients (state mutation) vs which are local to one
session's view (ephemera).

## Anti-patterns

- **Don't** add a "shortcut" path that lets the GUI bypass the
  dispatcher and call registry methods directly. That's how the
  pre-unification desktop diverged from remote clients (see
  `another-one-9jw` discovery report — 10 GUI handlers were
  bypassing `Control` at the time it was filed).
- **Don't** add a `UiAction` variant for state that has a meaningful
  remote shape (anything a phone client would also need to know
  about). Use `Control`.
- **Don't** wire each `UiAction` variant as its own MCP tool. The
  single `dispatch_ui_action` tool taking a tagged-enum `action`
  argument keeps the tool catalog small and matches how `Control`
  variants will surface once auto-derivation lands.

## Roadmap

This doc is an intentionally short reference. The unification work
has its own beads tree under `another-one-2w5`:

- `another-one-o9y` — `UiAction` enum + dispatcher (this PR's
  starting point; foundation for bucket 2).
- `another-one-vkq` — auto-derive MCP tools from `Control` variants
  (closes the 18 "verb exists, tool missing" findings from
  `9jw` in one move).
- `another-one-afa` — migrate the 10 Control-bypassing GUI handlers
  to dispatch through `Control`. Lands cleaner after `vkq` so the
  resulting MCP exposure is automatic.
- `another-one-g9j` — this doc.
