# AGENTS

This is a greenfield app, and has no users.
This app is built for mac and linux - any changes must keep this in mind.

## Rust formatting

- Do not run broad `cargo fmt` or `rustfmt` on module entry files unless the task is explicitly formatting-related.
- Prefer `cargo fmt --check` first.
- If formatting is required, only keep formatting changes in files that are part of the current task.

## UI Rules

- This applies to icon-only controls and text-based actions alike unless the element is purely decorative or intentionally non-interactive.
- Any user-facing errors or notifications should go through the app toast function unless explicitly specified otherwise.

## Tracking work

We use GitHub Issues to capture work. Do not let drive-by findings expand the current branch's scope — file them and move on.

**When to file an issue instead of fixing in place**
- You notice a bug/smell/idea that is not part of the current task.
- The user says "idea:" / "later:" / "while you're here" / anything not on the current branch's stated goal.
- Work is blocked and needs external input.

**How to file — always confirm first**
- Never open an issue silently. Propose title + label + one-line body to the user and wait for a yes. Applies to both user idea-dumps and agent-spotted drive-by findings.
- Search first: `gh issue list --search "<keywords>"` — no duplicates.
- After approval: `gh issue create --title "<imperative, short>" --label <label> --body "..."`.
- Body: one line on what you were doing when you noticed it, plus file/line if relevant.

**Labels**
- `bug` — something is broken
- `enhancement` — new capability or improvement to existing behavior
- `idea` — raw brain-dump, not yet triaged
- `question` — needs clarification before work can start

**Finding the next piece of work**
- `gh issue list --state open` to browse, or filter by label (`--label enhancement`, `--label bug`, etc.).
- If picking one up, reference it in the PR (`Closes #123`).

**Never**
- Open an issue without first getting the user's OK on title + label.
- Close issues you did not fix — let the user triage `idea` / `question`.
- Open an issue and then immediately fix it on the current branch. If it's small enough to fix right now, it was part of the current task; if it wasn't, it doesn't belong on this branch.

this app should have a dark and light mode.

## Cross-thread event routing

There are two patterns for moving data to the render thread. They coexist by design and are not expected to merge.

**Push events — AppEvent bus**

Asynchronous results from background tasks (daemon acks, file diffs, search replies, lookup completions) go through the single `tokio::sync::mpsc::UnboundedSender<AppEvent>` on `AnotherOneApp`. Producers clone `app_event_tx` before the thread boundary and call `let _ = tx.send(AppEvent::YourVariant { … });` inside the closure. The render timer calls `drain_app_events` once per tick, which `try_recv`-loops until empty and ORs the `bool` return of each per-variant handler. Handlers must **not** call `cx.notify()` — the render timer calls it at most once per tick after collecting all dirty bits.

**Polled-state drains — stay polled**

Drains that pull from `Vec<T>` fields in `RegistryState` or from global statics (`drain_pending_tab_launches`, `drain_pending_spawn_terminals`, `drain_pending_close_tabs`, `drain_pending_select_focus`, `drain_pending_ui_actions`, `drain_pending_tab_resizes`, `drain_git_refresh`, `drain_git_actions`, `drain_session_events`, `drain_terminal_launch_replies`, and similar) remain as explicit drain methods called from the render timer. Do not migrate them to the AppEvent bus.

**How to add a new AppEvent variant**

1. Add a variant to `AppEvent` in `app/src/app_event.rs`. All payload fields must be `Send + 'static`. Add a doc comment naming the producer and explaining when it fires.
2. In the producer, clone `self.app_event_tx` before the spawn boundary and call `tx.send(AppEvent::YourVariant { … })` inside the closure.
3. Add a match arm to `drain_app_events` in `app/src/app.rs` that calls a new `handle_*` method and ORs its `bool` return into `dirty`.
4. Implement `fn handle_your_variant(&mut self, …, cx: &mut Context<Self>) -> bool` — return `true` if render-side state changed, `false` otherwise.

## ControlRegistry

`AnotherOneApp` holds a `RefCell<ControlRegistry>` (see `app/src/control.rs`). Every interactive element that registers itself here gets a stable `ControlId` and a `ControlEntry` for the current frame. The registry is the hook point for the `test-harness` feature, which will eventually expose `simulate_click` / `simulate_toggle` on `AnotherOneApp` for interaction tests without a real window.

**Render lifecycle**

At the very top of `AnotherOneApp::render`, before any element construction:
```rust
self.control_registry.borrow_mut().clear();
```
During element construction each builder calls `registry.borrow_mut().register(ControlEntry { … })`. After `render` returns the registry holds exactly the controls visible in that frame.

**RefCell pattern**

The registry is stored as `RefCell<ControlRegistry>` so `&self` render helpers can register without `&mut self` cascading through the entire element tree. Builder functions accept `registry: &std::cell::RefCell<crate::control::ControlRegistry>` and call `registry.borrow_mut().register(…)` inline.

**ControlId variants**

- `ControlId::Static(&'static str)` — for buttons with a compile-time id string (settings buttons, one-off controls). Use a unique literal per button, e.g. `"theme-light"`.
- `ControlId::Task { task_id: SharedString, kind: TaskControl }` — for per-row controls in lists where the same builder is called N times with different data. Add a new `TaskControl` discriminant if the kind is not already covered.

**How to add a new control**

1. Pick or add a `ControlId` variant.
2. In the builder, before constructing the GPUI element:
   ```rust
   registry.borrow_mut().register(crate::control::ControlEntry {
       id: crate::control::ControlId::Static("your-id"),
       label: "Human label".into(),
       kind: crate::control::ControlKind::Button,
       enabled: true,
       handler: None,   // stays None until test-harness step 4
   });
   ```
3. Leave `handler: None` until `simulate_click` / `simulate_toggle` land (step 4 in the migration checklist in `control.rs`). Do not skip registration just because the handler is not yet wired.

## Debug state seeding

In `#[cfg(debug_assertions)]` builds the project store opens a per-worktree SQLite database at `<binary location>/.another-one/state.sqlite` (i.e. `target/debug/.another-one/state.sqlite`) rather than the production path from `app_config_dir()`. This directory is already covered by the `target/` gitignore entry and is removed by `cargo clean` or worktree deletion.

On first run, if the production database exists at `~/.config/another-one/state.sqlite`, `dev_state_db_path()` seeds the debug copy via `VACUUM INTO` (a consistent read-only snapshot that captures any uncommitted WAL data). Subsequent debug runs reuse the copy, so project mutations persist between sessions. Release builds are unaffected.

Do not add workarounds for "empty project list" in debug builds or hard-code config paths — the seeding handles it automatically.
