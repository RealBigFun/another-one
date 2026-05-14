# Daemon-canonical terminal state

## Status

Accepted

## Decision Summary

Move per-tab `alacritty_terminal::Term` ownership and ANSI/VT parsing into
the daemon, with viewers (desktop GPUI, mobile) consuming versioned
`TerminalFrame` snapshots over the existing `Session` transport. The wire
gains a pull-paced subscription verb so each viewer chooses its own frame
rate; inactive tabs cost zero on every viewer because the daemon emits no
frames for unsubscribed `(viewer, tab)` pairs. The tradeoff is one Term
per PTY (good: scales with viewers, fixes desktop GPUI lockups under busy
harnesses) at the cost of refining the existing terminal-wrapping
principle to make daemon-side VT parsing explicitly allowed.

## Problem Statement

When a single agent harness (Claude Code, Codex, Cursor, etc.) emits
sustained PTY output — for example a streaming agent response or a
verbose tool-call log — the desktop GPUI thread becomes unresponsive
until the harness calms down. Concretely:

- Per-tab `LiveTerminalRuntime` lives on the GPUI render thread
  (`app/src/terminal_runtime.rs`).
- `drain_terminal_launch_replies` (`app/src/app.rs:6059`) and
  `drain_session_events` (`app/src/app.rs:5910`) both feed PTY bytes
  through `LiveTerminalRuntime::apply_output` (`terminal_runtime.rs:283`)
  during the GPUI render tick. That call runs
  `alacritty_terminal::vte::ansi::Processor::advance(&mut term, bytes)`
  synchronously on the render thread.
- A 64 KiB-per-tick byte cap (`DRAIN_OUTPUT_BYTE_CAP`, `app/src/app.rs:150`)
  exists specifically to keep parsing under one frame; the comment
  records that 256 KiB "blew the frame budget."
- An RAII `drain_tick_guard` watchdog (`app/src/app.rs:6060`) and a
  `output_sender.send` timing instrument (issue #125) exist to
  distinguish a starved drain from a true deadlock.

In other words: the GPUI thread is on the parse path, the parse cost
scales with harness verbosity, and the existing mitigations are tuned
just well enough to limp under typical load. Mobile has the same shape
and the same lurking problem on the same drain path.

A second concern surfaces at scale. Each viewer that attaches to a tab
today maintains its own `Term` instance (~19 MB of grid + scrollback at
default sizes) and re-parses the full byte stream. With 100 background
agents and two viewers (desktop + mobile) the memory and CPU costs are
both 2× what a single canonical Term would cost.

## Goals

- The GPUI render thread never runs `Processor::advance` or any other
  parse-shaped work, regardless of how loud a harness is.
- "Inactive" tabs (no viewer focused on them) cost zero on every viewer:
  no parsing, no allocation, no wire traffic.
- A single Term per PTY is the source of truth, regardless of how many
  viewers (desktop + mobile + future) are attached.
- Refocusing a tab is bounded by one snapshot send + render — no
  multi-second replay storms.
- The wire protocol designed today supports diffs, per-viewer frame-rate
  pacing, and on-demand scrollback access without future wire breakage.

## Non-Goals

- This design does not change agent semantics. Per-agent / per-CLI
  output parsing remains forbidden by the (refined) terminal-wrapping
  principle.
- This design does not introduce structured agent-message types on the
  wire. The wire becomes "grid frames + bytes (opt-in for tools)," not
  "agent events."
- This design does not deliver `Diff` frame emission, per-viewer
  `max_fps` honoring, or scrollback diff compression in v1. Those are
  future implementations of an already-stable wire (see Implementation
  Plan).
- This design does not address cross-process MCP raw-byte subscription.
  In-process MCP keeps working through the existing
  `output_broadcast: broadcast::Sender<Vec<u8>>` registry handle.
  Cross-process MCP can land `Control::SubscribeRawBytes` later without
  re-opening this design.

## Design Decisions

### 1. Term and Processor live in the daemon, one task per PTY

For each live PTY, the daemon owns one tokio task that holds the
`Term` + `Processor` + PTY writer + master, with no Mutex around the
Term. The task's inputs are an `mpsc::Receiver<TerminalCommand>`:

```rust
enum TerminalCommand {
    Bytes(Vec<u8>),
    Input(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    RequestFullFrame { reply: oneshot::Sender<Arc<TerminalFrame>> },
    Search { request: TerminalSearchRequest, reply: oneshot::Sender<TerminalSearchReply> },
    ReadScrollback { request: ScrollbackRequest, reply: oneshot::Sender<ScrollbackReply> },
    Shutdown,
}
```

The task's outputs are:

- A `tokio::sync::watch::Sender<Arc<TerminalFrame>>` driven on every
  state-mutating command (advance, resize, etc.). Per-viewer pacer
  tasks subscribe to this watch.
- The existing per-tab `broadcast::Sender<Vec<u8>>` for raw bytes,
  retained for in-process MCP `tab_output` consumers.
- Dedicated `WorkerReply::Push` events for low-rate side channels:
  title change, bell.

The PTY reader thread (currently `core/src/terminal_launch.rs:~225`)
stays a `std::thread` and uses `mpsc::blocking_send` to push
`TerminalCommand::Bytes` into the Term task. Keeping the reader as a
blocking thread avoids an `AsyncFd` wrapper around `portable_pty`'s
master and matches the existing transport seam pattern.

This single-owner-no-Mutex shape rules out the lock-contention failure
mode of placing Term behind `Arc<Mutex<Term>>` and the head-of-line
blocking failure mode of a single shared VT-worker task processing
every Term in turn.

### 2. PTY spawn moves to the daemon

Today `Control::LaunchTab` queues `pending_tab_launches` for the GPUI
render tick to drain (`app/src/daemon_host.rs:1090`); GPUI runs
`core::terminal_launch::spawn_terminal_launch` and registers the
resulting broadcast/writer back into `RegistryState` via
`register_tab_with_registry`. That detour exists because the daemon
used to be a pure bus.

With the Term canonical in the daemon, the PTY's birth must coincide
with the Term task's birth. `Control::LaunchTab` becomes the real
spawn path: the daemon creates the PTY (via `portable_pty`), spawns
the Term task, starts the reader thread, and emits a
`WorkerReply::TabLaunched { section_id, tab_id, process_id, ... }`
projection. `pending_tab_launches`, the GPUI-side fulfillment, and
`DesktopTerminalRegistry::launch_tab`'s queueing logic go away.

Warm launches and Claude session restore (`spawn_warm_terminal_launch`,
`claude_session_watch_loop` in `core/src/terminal_launch.rs`) move
along with the spawn path. Their failure semantics need a fresh pass
during implementation; their public shape (a tab eventually exists or
a launch failure is reported) is preserved.

### 3. Wire shape: `TerminalFrame { Full | Diff }` with monotonic `seq`

The wire gains:

```rust
SessionEvent::TerminalFrame {
    section_id: String,
    tab_id: String,
    frame: TerminalFrame,
}

enum TerminalFrame {
    Full {
        seq: u64,
        snapshot: Arc<GridSnapshot>,
    },
    Diff {
        seq: u64,                    // = previous_seq + 1; gap → request Full
        rows_changed: Vec<RowDelta>,
        cursor: Option<CursorState>,
        mode: Option<ModeFlags>,
        scroll_offset: Option<usize>,
        bell: bool,
    },
}
```

`GridSnapshot` carries the visible viewport rows + a small backbuffer
(2× viewport, sized for momentum scroll) — not the full ~10k-line
scrollback. The full history stays on the daemon's Term and is
fetched on demand.

Always sending `Full` on attach and on `seq` gap means viewers never
need a "is my state consistent?" reconciliation algorithm. Diffs are
the steady-state hot path once shipped; before then, the daemon emits
only `Full` and the wire variant exists but is never produced.

`Arc<GridSnapshot>` is zero-copy on the in-memory transport and
serialized once per frame on iroh.

### 4. Pull-paced subscription, per viewer, per tab

```rust
Control::TerminalSubscribe {
    section_id, tab_id,
    max_fps: u8,
    since_seq: Option<u64>,
}

Control::TerminalUnsubscribe { section_id, tab_id }
```

Per `(viewer, tab)` subscription, the daemon spawns a small pacer task
that watches the Term task's `frame_watch`, debounces to `max_fps`,
and emits frames into the viewer's session event sink. No subscription
→ no pacer → no frame work for that tab on that viewer. A subscription
with `since_seq: None` (or with a stale seq the daemon no longer holds
diff history for) gets an immediate `Full`.

This is push-on-the-wire but pull-paced semantically: each viewer
declares its own frame rate. Desktop runs at 60; mobile-on-cellular at
10; a backgrounded mobile app at 0. The daemon does not guess.

Refocus on the desktop becomes: send `TerminalUnsubscribe` for the old
tab, `TerminalSubscribe { since_seq: None }` for the new one. The new
subscription's first frame *is* the refocus replay.

### 5. Scrollback and search are RPCs, not pushes

```rust
Control::TerminalReadScrollback { section_id, tab_id, range: ScrollbackRange }
    → WorkerReply::TerminalScrollback { rows: Vec<GridRow>, range_actual }

Control::TerminalSearch { section_id, tab_id, pattern, regex, case }
    → WorkerReply::TerminalSearch { matches: Vec<GridMatch> }
```

The daemon's Term keeps the full scrollback (~10k lines by default, same
as today's per-viewer Term default); a grid walk over it for either
read or search is microseconds. Frames stay small; viewers fetch
historical rows lazily as the user scrolls past the snapshot's
backbuffer. Search runs at "interactive" latency, not "frame" latency,
so an RPC is fine.

### 6. Bell, title, and OSC events ride dedicated `WorkerReply::Push` variants

Bell flashes and title updates already use `WorkerReply` plumbing; new
variants (`TerminalTitle { tab_id, title }`, `TerminalBell { tab_id }`,
`TerminalResetTitle { tab_id }`) keep them out of the frame stream so
viewers can react to them independently of frame cadence.

### 7. Multi-viewer resize: existing min-clamp policy preserved

`RegistryState::active_viewers: HashMap<TerminalRuntimeKey, HashMap<viewer_id, (cols, rows)>>`
and `effective_sizes` (`app/src/daemon_host.rs:97-103`) already pick
the min of all viewers' size claims. The new model keeps this. Effective
size changes route as `TerminalCommand::Resize` to the Term task; the
task resizes both `Term` and `MasterPty`. The GPUI-side
`TabResizeRequest` mpsc (`app/src/daemon_host.rs:434`) is removed.

### 8. Inactive tabs parse, don't render

The Term task always runs `Processor::advance` on incoming bytes —
skipping it would corrupt VT state for the next refocus. What it does
*not* do, when no viewer is subscribed, is bump the `frame_watch`
beyond an internal "dirty" flag (no `Arc<TerminalFrame>` allocation,
no serialization). Subscriber pacers don't run. The cost of an
inactive tab is the parse plus an `AtomicBool::store(true)`.

This satisfies "work continues in the daemon; viewers don't render
when not focused" while keeping VT state correct so refocus is a
single snapshot send rather than a replay-and-reparse.

### 9. Refining the terminal-wrapping principle

`docs/architecture/terminal-wrapping-principle.md` currently states
that the mobile `TerminalTransport` interface is byte-in, byte-out and
that "the terminal renderer parses ANSI." The principle's purpose is
to forbid per-agent / per-CLI semantic parsing — not to forbid VT/ANSI
parsing in general.

This design refines the doc with:

- ANSI/VT parsing may live in the daemon. The terminal-wrapping rule
  is about agent semantics, not the universal terminal protocol.
- The wire carries grid frames; raw PTY bytes remain available
  in-process for tools that legitimately wrap the byte stream
  (in-process MCP `tab_output`).
- Per-agent prompt templating, command extraction, and agent-specific
  rendering remain forbidden in client UIs.

The doc update lands alongside this design's first phase.

## Edge Cases & Failure Modes

- **Term task panic.** The task's `JoinError` is logged and the tab
  surfaces as failed via `WorkerReply::TabExited`. Viewers see the
  tab move to a failed state; no implicit restart. (Same surface as
  PTY exit today.)
- **Frame seq gap on a viewer.** The viewer's pacer task notices a
  non-contiguous seq (only relevant once `Diff` ships), emits a
  `RequestFullFrame` to the Term task, and resumes. A viewer that
  reconnects with a stale `since_seq` gets a `Full` regardless.
- **Lagged broadcast (legacy `output_broadcast`).** In-process MCP
  consumers continue to use the existing per-tab broadcast and the
  existing lag-handling. No wire change.
- **Multi-viewer resize flap.** Mobile attaches at 30×80, then
  detaches; effective size flips back to desktop's 60×200. The Term
  task resizes the grid twice in quick succession — alacritty handles
  this; same behavior as today, just routed through the Term task.
- **PTY reader outpaces Term task.** The reader thread's
  `blocking_send` into the Term mpsc backpressures naturally.
  Equivalent to today's bounded `mpsc::SyncSender<TerminalLaunchReply>`
  but isolated per tab, so a hot harness only backpressures itself.
- **Frame allocator pressure on busy `Full` emission.** A focused tab
  on a verbose harness allocates one `Arc<GridSnapshot>` per coalesce
  tick. Likely fine in v1; if profiling shows GC-style pressure, this
  is the trigger to ship `Diff` emission.
- **Refocus while previous tab is mid-launch.** Subscribing to a tab
  whose Term task hasn't started yet records the subscription intent;
  the Term task on spawn checks for outstanding subscriptions and
  emits an initial `Full` immediately.

## Rejected Alternatives

### Client-side off-thread parsing (per-viewer worker)

Move `Term` + `Processor` to a per-viewer worker thread (one per
client process), keep `PtyBytes` on the wire, every viewer parses
locally. Smaller refactor; fixes the lockup symptom on desktop.

Rejected because: it scales linearly with viewer count
(desktop + mobile = 2× memory, 2× parse), provides no benefit when
a future viewer attaches, and leaves mobile with the same lurking
lockup until it ships its own off-thread parser. The work to move
state into the daemon is paid once; the work to ship N off-thread
parsers compounds.

### Client-side off-thread + daemon raw-byte ring buffer for replay

Same as above, plus a daemon-side ring buffer that replays raw bytes
on attach so unfocused tabs can be unsubscribed entirely on the
client side. Cheapest steady-state cost for many idle background
tabs.

Rejected because: ring sizing is unsolvable for chatty TUIs. A
streaming agent or an animated progress bar can blow a megabyte-sized
ring in seconds, leaving the post-replay state partial and visually
wrong. A daemon Term solves the same "no client cost while inactive"
goal without a sizing knob nobody can pick correctly.

### Single shared VT-worker task in the daemon

One tokio task with `HashMap<Key, Term>`, dispatching commands.
Smaller per-tab task overhead.

Rejected because: a CPU-bound `Processor::advance` for one busy
harness blocks the worker from advancing every other tab's Term.
Tokio task overhead is negligible (<1 KB metadata per task) and one
task per Term gives natural per-tab CPU isolation.

### `Mutex<Term>` for direct synchronous reads

Per-tab task owns the Term, but wraps it in a Mutex so non-task code
paths (search, scrollback peek) can read it directly without going
through the inbox.

Rejected because: it reintroduces lock contention in the exact place
we're trying to remove it from, and any code path that bypasses the
inbox to read the Term can interleave with an in-progress
`Processor::advance` and observe a half-mutated grid. Search and
scrollback as RPCs are simpler.

### Snapshots-only wire (drop `PtyBytes` entirely)

Remove `SessionEvent::PtyBytes` from the wire and require all
consumers (including MCP) to consume snapshots.

Rejected because: in-process MCP's `tab_output` legitimately wraps a
PTY byte stream and exposing the grid to it would break the
abstraction in the wrong direction. Keeping `output_broadcast`
internal-to-daemon and removing only the *wire* `PtyBytes` (which is
covered separately by removing the default subscription) is the right
split.

### Push-paced (daemon picks the cadence)

Daemon emits frames at a fixed cadence per active tab, regardless of
viewer.

Rejected because: a single fixed cadence is wrong for both
mobile-on-cellular (too fast) and desktop (potentially too slow on a
high-refresh display). Pull pacing puts the rate decision where the
information is.

### `TabAttachment.replay: Vec<Vec<u8>>` for refocus replay

Use the existing `TabAttachment.replay` slot to deliver the initial
state on attach.

Rejected because: the field is a remnant of an earlier design, has
been unconditionally empty, and would force the initial-attach code
path into a different shape from steady-state frame delivery. Pushing
the initial `Full` through the regular event stream removes a special
case.

## Integration Points

- **`daemon_proto`** — gains `TerminalFrame`, `GridSnapshot`,
  `RowDelta`, `CursorState`, `ModeFlags`, `Control::TerminalSubscribe`,
  `Control::TerminalUnsubscribe`, `Control::TerminalReadScrollback`,
  `Control::TerminalSearch`, and matching `WorkerReply` variants.
- **`daemon-transport`** — gains a `SessionEvent::TerminalFrame`
  variant. The `in_memory` and `iroh` and `uds` impls all gain
  matching ServerFrame plumbing; serialization for iroh is bincode as
  with existing variants.
- **`daemon`** — adds a per-tab Term task module
  (`daemon/src/terminal/task.rs` proposed) plus per-viewer pacer
  module. `dispatch.rs` gains arms for the new Control variants;
  `handle_attach` is replaced (or repointed) by `TerminalSubscribe`
  handling. PTY spawn moves from
  `core::terminal_launch::spawn_terminal_launch` into the daemon
  (likely a new `daemon/src/terminal/launch.rs`).
- **`core/src/terminal_launch.rs`** — shrinks. The spawn helpers,
  warm-launch glue, and Claude session watch all migrate to the
  daemon side. Pure-data types (`TerminalLaunchConfig`, etc.) likely
  stay in `core` as shared.
- **`app/src/terminal_runtime.rs`** — `LiveTerminalRuntime` collapses
  to a snapshot cache + input helper. `apply_output`, the Term, the
  Processor, the local PTY handle, the writer, and the event-queue
  drain all leave the GPUI side.
- **`app/src/app.rs`** — `drain_terminal_launch_replies` is removed
  outright; `drain_session_events` keeps only `Push` and the new
  `TerminalFrame` arms; the byte-cap constants go away. The PTY
  reader-side watchdog and `DRAIN_OUTPUT_BYTE_CAP` mitigations become
  unnecessary; remove with prejudice.
- **`app/src/daemon_host.rs`** — the spawn-fulfillment path
  (`pending_tab_launches`, `register_tab_with_registry`,
  `TabResizeRequest` queue) is removed. `RegistryState` keeps
  `active_viewers` and `viewer_focus`; `broadcasts` and `writers`
  move into the Term tasks.
- **MCP (`core/src/mcp/...`)** — unchanged on the wire. In-process
  MCP keeps subscribing to the per-tab `output_broadcast` directly
  through the registry. `ClientEvent::Output` routing stays.
- **Mobile** — once daemon emits frames, mobile's local
  `apply_output` path is replaced with a snapshot renderer. Until
  that lands, the daemon emits nothing for unsubscribed legacy mobile
  attaches; once mobile sends `TerminalSubscribe`, it gets frames.
- **`docs/architecture/terminal-wrapping-principle.md`** — refined
  alongside Phase 1 of the implementation plan.

## Implementation Plan

The protocol shape (§3–§6) is the long-term commitment. The daemon's
implementation lands in committable phases; each phase leaves the
repository in a stable, reviewable state. Frame `Diff` emission,
per-viewer `max_fps` honoring, and `Full` allocation profiling are
explicitly deferred to later phases on the same wire.

Per repo conventions (`AGENTS.md`):

- All phases run `cargo build` and `cargo test --workspace` clean.
- Use `cargo fmt --check`; do not run broad `cargo fmt` on module
  entry files unless the change is formatting-only.
- Targeted `cargo clippy --workspace --all-targets -- -D warnings` on
  modified crates.
- Each phase reviewable as one PR; sub-phases are commit-sized within
  a PR unless explicitly noted as their own PR.

### Phase 1 — Wire types and principle clarification (one PR)

Baseline scaffolding. No behavior change; new verbs return
`WorkerReply::NotYetImplemented` (or the existing equivalent) and the
new event variant is never produced.

- [ ] **1a.** Add types to `daemon-proto`.
  - Files: `daemon-proto/src/lib.rs`.
  - Work: define `TerminalFrame`, `GridSnapshot`, `GridRow`,
    `GridCell`, `RowDelta`, `CursorState`, `ModeFlags`,
    `ScrollbackRange`, `ScrollbackRequest`, `ScrollbackReply`,
    `TerminalSearchRequest`, `TerminalSearchReply`, `GridMatch`. Add
    `Control::TerminalSubscribe { section_id, tab_id, max_fps,
    since_seq }`, `TerminalUnsubscribe`, `TerminalReadScrollback`,
    `TerminalSearch`, `TerminalInput`. Add matching
    `WorkerReply::TerminalSubscribeAck`,
    `TerminalUnsubscribeAck`, `TerminalScrollback`, `TerminalSearch`,
    `TerminalInputAck`. Add push variants `TerminalTitle`,
    `TerminalBell`, `TerminalResetTitle`.
  - Validation: `cargo test -p daemon-proto`; serde round-trip tests
    for every new struct/enum, including `Arc<GridSnapshot>` (Arc is
    transparent to bincode).
- [ ] **1b.** Add `SessionEvent::TerminalFrame` and serialize across
  every transport.
  - Files: `daemon-transport/src/lib.rs`,
    `daemon-transport/src/in_memory.rs`, `daemon-transport/src/uds.rs`,
    `daemon-client/src/iroh_transport.rs`,
    `daemon/src/transport_iroh.rs`, `daemon/src/transport_ws.rs`,
    `daemon/src/transport_mcp.rs`.
  - Work: add the variant; add the matching `ServerFrame` arm; route
    serialization for iroh, uds, in-memory, and ws/mcp pumps. No
    producer yet.
  - Validation: `cargo test --workspace`; new round-trip test in
    `daemon-transport/src/in_memory.rs` that injects a synthetic
    `TerminalFrame::Full` event and asserts the client receives it
    intact. Lint clean per crate.
- [ ] **1c.** Stub dispatch arms for the new `Control` verbs.
  - Files: `daemon/src/dispatch.rs`.
  - Work: add arms for `TerminalSubscribe`/`Unsubscribe`/`Search`/
    `ReadScrollback`/`Input`. Each returns a not-implemented reply
    (matching the project's existing pattern for unimplemented verbs;
    confirm pattern when starting). No registry calls yet.
  - Validation: `cargo test -p daemon`; existing dispatch tests stay
    green.
- [ ] **1d.** Refine `terminal-wrapping-principle.md`.
  - Files: `docs/architecture/terminal-wrapping-principle.md`.
  - Work: add the clarifications from §9 of this design — ANSI/VT
    parsing may live daemon-side; per-agent semantic parsing remains
    forbidden. Add a backlink to this design doc.
  - Validation: doc renders; backlinks resolve.

### Phase 2 — Daemon-side Term task (one PR)

Isolated module exercised by unit tests; no production code path
uses it yet.

- [ ] **2a.** Module skeleton + `TerminalCommand` enum + bytes loop.
  - Files: `daemon/src/terminal/mod.rs` (new),
    `daemon/src/terminal/task.rs` (new).
  - Work: define `TerminalCommand`, `TerminalTask` struct,
    `EventProxy` for the alacritty `Term`, the `select!` loop,
    handle `Bytes` via `processor.advance`, drain Term events,
    bump an internal `dirty` flag. No frame emission yet.
  - Validation: unit test feeding a recorded byte stream and
    asserting `term.grid()` reaches the expected state.
- [ ] **2b.** Frame serialization + `RequestFullFrame`.
  - Files: `daemon/src/terminal/frame.rs` (new),
    `daemon/src/terminal/task.rs`.
  - Work: implement `serialize_full_frame(&Term, seq) -> Arc<GridSnapshot>`;
    walk visible viewport + 2× backbuffer; populate cursor + mode
    flags. Wire `RequestFullFrame` reply.
  - Validation: unit test asserting snapshot rows match expected
    grid contents after parsing a curated stream (cursor moves, alt
    screen, color changes).
- [ ] **2c.** Resize, search, read-scrollback handlers.
  - Files: `daemon/src/terminal/task.rs`.
  - Work: route `Resize { cols, rows }` to both Term and (later)
    PTY master — master integration is a no-op stub here, real wiring
    in Phase 4. Implement `Search` and `ReadScrollback` against the
    Term grid (regex via `regex` crate; mirror the existing
    `terminal_runtime.rs:415, 523` walks).
  - Validation: unit tests for resize→frame change; search→match
    coordinates; scrollback read→row contents.
- [ ] **2d.** Side-channel: `Bell`, `Title`, `ResetTitle` outputs.
  - Files: `daemon/src/terminal/task.rs`.
  - Work: drain Term events into a `mpsc::Sender<TerminalSideEffect>`
    consumed by the dispatch layer (Phase 3). Stub consumer in tests.
  - Validation: unit test confirming a `\a` byte produces a `Bell`
    side-effect; OSC title sequence produces `Title`.
- [ ] **2e.** Soak test on a recorded heavy harness trace.
  - Files: `daemon/tests/terminal_task_soak.rs` (new),
    `daemon/tests/fixtures/heavy_harness.bin` (new — captured trace).
  - Work: capture ~10 MB of real Claude/Codex output (script in
    `scripts/` or an ad-hoc capture) and replay it through a Term
    task, asserting bounded memory and no panics.
  - Validation: test runs in <5s; peak RSS bounded.

### Phase 3 — Per-viewer pacer + dispatch wiring (one PR)

Bytes-in production still goes through legacy paths; the new pacer
is exercised end-to-end through tests using a synthetically-fed Term
task.

- [ ] **3a.** Pacer module.
  - Files: `daemon/src/terminal/pacer.rs` (new).
  - Work: per-`(viewer, tab)` tokio task, watches a
    `tokio::sync::watch::Receiver<Arc<TerminalFrame>>`, debounces at
    a single fixed 60 fps cap (no `max_fps` honoring), pushes
    frames into the viewer's session event sink. Honors
    `since_seq` on first poll: sends `Full` if missing or stale.
  - Validation: unit test with an in-process channel as the "sink"
    confirming pace cap.
- [ ] **3b.** Dispatch `TerminalSubscribe` / `TerminalUnsubscribe`.
  - Files: `daemon/src/dispatch.rs`,
    `daemon/src/terminal/mod.rs`.
  - Work: registry gains a `subscribe(viewer_id, section, tab) -> watch::Receiver`
    helper backed by a per-tab map of Term task handles.
    `TerminalSubscribe` spawns a pacer; `TerminalUnsubscribe` aborts
    it. Viewer-disconnect cleans up all pacers for that viewer.
  - Validation: integration test using `daemon-transport`'s
    `in_memory::pair` plus a synthetic Term task: two viewers
    subscribe, both receive frames; one unsubscribes, only the other
    keeps receiving; viewer disconnects, no orphan pacer.
- [ ] **3c.** Side-channel pushes wired through dispatch.
  - Files: `daemon/src/dispatch.rs`, `daemon/src/terminal/mod.rs`.
  - Work: drain `TerminalSideEffect` from active Term tasks and
    rebroadcast as `WorkerReply::Push(TerminalTitle | Bell |
    ResetTitle)` to all subscribers of the originating tab.
  - Validation: integration test: bell on the Term task is observed
    as a `Push(TerminalBell)` on every subscribed viewer.

### Phase 4 — PTY spawn moves to the daemon (one PR; behavior gated)

Legacy GPUI-side spawn path remains intact and the default; the new
daemon-side spawn path lands behind a runtime flag
(`ANOTHER_ONE_DAEMON_SPAWN=1`) so it can be exercised in tests and
dev without breaking shipped builds.

- [ ] **4a.** Move command building + environment helpers to the daemon.
  - Files: `daemon/src/terminal/launch.rs` (new),
    `core/src/terminal_launch.rs` (extract reusable helpers).
  - Work: move `build_command`, `apply_terminal_environment`,
    `HarnessEnv` consumers, and pure-data types
    (`TerminalLaunchConfig` stays in `core`; the spawn function
    moves). Pure-data types in `core` are re-exported for the daemon.
  - Validation: `cargo test --workspace`; existing GPUI spawn path
    keeps using the now-extracted helpers and behaves identically.
- [ ] **4b.** Daemon-side spawn function + reader thread + Term task
  start.
  - Files: `daemon/src/terminal/launch.rs`,
    `daemon/src/terminal/mod.rs`.
  - Work: implement `daemon_spawn_terminal` that opens a PTY,
    spawns the child, starts the reader `std::thread` blocking-sending
    into the new Term task's mpsc, registers the task in the
    daemon-side terminal map. Emits `WorkerReply::Push(TabLaunched)`
    matching the existing `TerminalLaunchReply::Launched` semantics
    (process_id, runtime metadata).
  - Validation: in-memory integration test: send
    `Control::LaunchTab` with the flag set; observe
    `WorkerReply::TabLaunched`; subscribe; observe a `Full` frame
    after parsing the synthetic harness output.
- [ ] **4c.** Flag wiring + dispatch integration.
  - Files: `daemon/src/dispatch.rs`,
    `daemon/src/registry.rs`.
  - Work: `Control::LaunchTab` consults the flag; if set, takes the
    new path; otherwise queues `pending_tab_launches` as today.
    Mark legacy paths `#[doc(hidden)]` (not `#[deprecated]` yet — we
    delete them in Phase 5d, before any external users see warnings).
  - Validation: existing GPUI launch flow still passes its tests
    with the flag unset; new flow passes with the flag set.

### Phase 5 — Desktop cutover (multiple PRs)

The biggest phase; explicitly multi-PR. Each sub-phase ships
standalone, leaves the repository runnable, and removes a small
amount of legacy scaffolding.

- [ ] **5a.** Snapshot ingestion path (PR; dual-write).
  - Files: `app/src/terminal_runtime.rs`, `app/src/app.rs`.
  - Work: introduce `LiveTerminalRuntime::ingest_frame(frame:
    &TerminalFrame)` alongside the existing `apply_output`. New
    method updates the snapshot cache from a `TerminalFrame::Full`
    (Diff path returns an explicit "unimplemented" error path —
    daemon doesn't emit Diff yet). Add `drain_terminal_frame_events`
    on the GPUI tick that fans `SessionEvent::TerminalFrame` into
    `ingest_frame`. No subscriber wiring yet — method exists, dead
    code.
  - Validation: unit test on `LiveTerminalRuntime` that builds a
    synthetic `TerminalFrame::Full`, calls `ingest_frame`, and
    asserts the rendered surface matches the same content rendered
    via `apply_output`.
- [ ] **5b.** Desktop subscribes via `Control::TerminalSubscribe` (PR; flag-gated).
  - Files: `app/src/app.rs`, `app/src/daemon_host.rs`,
    `app/src/session_host.rs`.
  - Work: when a tab becomes focused, fire
    `Control::TerminalSubscribe`; on unfocus,
    `TerminalUnsubscribe`. Both paths run alongside the legacy
    `apply_output` path. New `TerminalFrame` events feed
    `ingest_frame` per 5a. The visible rendered surface is whichever
    path runs first — by construction, both should converge to the
    same grid. Behind the same flag as Phase 4c.
  - Validation: with the flag set, manually verify a focused tab
    renders correctly under typed input and a noisy harness; no
    visual divergence from the unflagged build.
- [ ] **5c.** Switch the rendering source to snapshots (PR; flag flip).
  - Files: `app/src/terminal_runtime.rs`, `app/src/app.rs`.
  - Work: rendering reads only from the snapshot cache; the
    `apply_output` call on the GPUI thread becomes a no-op when the
    flag is set (still compiled). Default the flag to **on**.
  - Validation: full smoke test on a verbose harness (Claude
    streaming a long response, `yes | head -1000000`, `cargo build
    -v` of a large workspace). Watchdog instrumentation
    (`leakscope::drain_tick_guard`) reports zero starvation; GPUI
    frame time stays under 16 ms throughout. Manual: type, paste,
    resize, scroll, search, ctrl-c, claude-restore.
- [ ] **5d.** Remove legacy GPUI-side parse path (PR; deletion).
  - Files: `app/src/terminal_runtime.rs`, `app/src/app.rs`,
    `app/src/daemon_host.rs`, `app/src/background_ops.rs`.
  - Work: delete `LiveTerminalRuntime::apply_output` and its
    callers; delete `drain_terminal_launch_replies`; delete
    `DRAIN_OUTPUT_BYTE_CAP` and the byte-cap comments; delete
    `pending_tab_launches`, `TabResizeRequest`, the
    register-after-fulfill seam, and `RegistryState::broadcasts` /
    `writers` (those move into the Term task). Drop
    `terminal_launch_receiver` and friends from `app::AnotherOneApp`.
    The runtime flag from 4c becomes unconditional; remove the env
    check.
  - Validation: no `apply_output` references remain (`rg apply_output
    app core`); GPUI thread owns no `Term`/`Processor` types
    (`rg 'alacritty_terminal::(Term|ansi::Processor)' app`); manual
    smoke as in 5c.
- [ ] **5e.** Remove load-bearing watchdog instrumentation (PR; tidy).
  - Files: `app/src/leakscope.rs`, `app/src/app.rs`,
    `core/src/terminal_launch.rs`.
  - Work: remove `drain_tick_guard` and the `output_sender.send`
    timing instrument added for issue #125; the GPUI thread is no
    longer on the parse path so the watchdog has no signal to
    report.
  - Validation: `cargo test --workspace`; manual smoke; a brief
    note in commit message that #125's instrumentation is intentionally
    removed.
- [ ] **5f.** Search and scrollback go RPC (PR).
  - Files: `app/src/terminal_runtime.rs`, `app/src/app.rs`,
    `daemon/src/dispatch.rs`, `daemon/src/terminal/task.rs`.
  - Work: replace local grid walks (`terminal_runtime.rs:415, 523`)
    with `Control::TerminalSearch` /
    `Control::TerminalReadScrollback` round-trips through the
    session. Scrollback fetch is lazy (only when user scrolls past
    the snapshot's backbuffer).
  - Validation: search interactive in a long-scrolled buffer;
    scrollback paging visibly correct; latency under 50 ms on a
    10k-row Term.

### Phase 6 — Warm launches + Claude session restore (one PR)

- [ ] **6a.** Move warm-launch state machine.
  - Files: `daemon/src/terminal/launch.rs`,
    `core/src/terminal_launch.rs`.
  - Work: port `spawn_warm_terminal_launch` and the warm-launch
    bookkeeping (`warm_launch_hint`, the sender/receiver shape) to
    the daemon side. `OpenTaskRequest::warm_launch_hint` keeps the
    same `#[serde(skip)]` semantics.
  - Validation: warm-launch test (existing or new in
    `daemon/tests/`); roundtrip assertion in
    `clients::open_task_request_warm_launch_hint_skipped_in_serde`
    stays green.
- [ ] **6b.** Move Claude session restore.
  - Files: `daemon/src/terminal/claude_restore.rs` (new),
    `core/src/terminal_launch.rs`.
  - Work: port `claude_session_watch_loop` to a daemon-side task;
    surface session-discovery via `WorkerReply::Push(SessionDiscovered)`
    (already exists — reuse).
  - Validation: claude-restore round-trip integration test (manual
    if no automated coverage exists) confirming desktop receives
    the discovered session and resumes correctly.

### Phase 7 — Mobile viewer cutover (own PR; possibly own repo)

Mobile renderer location TBD; this design doc reflects the daemon
side. The mobile-side work mirrors Phase 5a–5d on the mobile crate.
If mobile lives in a separate repo/crate, this is a separate tracking
issue under that project.

- [ ] **7a.** Mobile snapshot ingestion path.
  - Validation: typing into a paired mobile session updates the
    daemon-hosted Term and reflects on both viewers; bandwidth
    budget on iroh is acceptable for a single focused tab.

### Phase 8 — Diff frame emission (deferred; own issue)

Trigger: real measurement showing iroh bandwidth pressure or `Full`
allocator pressure under realistic harness load.

### Phase 9 — Per-viewer `max_fps` honoring (deferred; own issue)

Trigger: mobile-on-cellular ships and needs throttling, or a
backgrounded mobile viewer needs to drop to zero frames.
