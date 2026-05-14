# Phase 5 cutover — legacy alacritty-on-GPUI deletion checklist

Step 1 of the terminal-hardening plan ([RC branch](../../README.md):
`rc/terminal-hardening`). Inventory of every legacy surface that the
daemon-canonical cutover (design 01 / #158, Phase 5) replaces. The
later steps in the plan (#3 unify builders, #5 viewer scrollback, #6
viewer-only input, #7 retire dual-write) cite this list as their
deletion targets.

This file is not architecture; it is a working checklist. Delete it
when Phase 5 closes.

> Convention: `path:line` references are pinned to the RC base
> commit. Re-grep before deleting if the file has churned.

## A. The renderer-side `Term`/`Processor`/event-queue/local-PTY surface

The whole point of design 01: the renderer stops parsing VT bytes
and stops owning the PTY. The following must all leave
`app/src/terminal_runtime.rs`:

- [ ] `RuntimeEventProxy` struct + `EventListener` impl — `terminal_runtime.rs:174–185`.
- [ ] `LocalPty` struct — `terminal_runtime.rs:192–199`.
- [ ] `LiveTerminalRuntime::term` field + `parser` field +
      `event_queue` field + `local_pty: Option<LocalPty>` field —
      `terminal_runtime.rs:200–211`.
- [ ] `LiveTerminalRuntime::from_prepared` (constructs the local
      `Term`/`Processor`) — `terminal_runtime.rs:215–233`.
- [ ] `LiveTerminalRuntime::from_remote` (already viewer-only;
      collapses into the new single constructor) —
      `terminal_runtime.rs:240–259`.
- [ ] `LiveTerminalRuntime::output_broadcast` —
      `terminal_runtime.rs:268`.
- [ ] `LiveTerminalRuntime::has_local_pty` —
      `terminal_runtime.rs:274`.
- [ ] `LiveTerminalRuntime::apply_output` (already
      `#[deprecated]`) — `terminal_runtime.rs:304–340`.
- [ ] `LiveTerminalRuntime::write_input` — replaced by viewer-only
      input task (#6) — `terminal_runtime.rs:343–358`.
- [ ] `LiveTerminalRuntime::paste_text` — moves into the same
      viewer-only path — `terminal_runtime.rs:360–367`.
- [ ] `LiveTerminalRuntime::request_soft_redraw` — same — `terminal_runtime.rs:418`.
- [ ] `LiveTerminalRuntime::display_offset` (reads local grid;
      replaced by viewer scroll-state in #5) —
      `terminal_runtime.rs:371`.
- [ ] `LiveTerminalRuntime::screen_lines` (reads local grid; the
      proto snapshot already carries `rows`) —
      `terminal_runtime.rs:381`.
- [ ] `LiveTerminalRuntime::is_alternate_screen` and
      `LiveTerminalRuntime::mouse_protocol` — replace with reads
      against `GridSnapshot::mode` — `terminal_runtime.rs:385,392`.
- [ ] `LiveTerminalRuntime::resize` body that calls
      `self.term.resize(size)` — keep the size field, drop the
      local-grid mutation — `terminal_runtime.rs:284–294`.
- [ ] `LiveTerminalRuntime::snapshot` rebuild branch
      `if self.local_pty.is_some() { self.cached_snapshot =
      build_surface_snapshot(&self.term, self.size) }` —
      `terminal_runtime.rs:464–471` (this is the branch that
      currently *clobbers* the proto cache on desktop).
- [ ] `LiveTerminalRuntime::writer_handle` —
      `terminal_runtime.rs:565–567`.
- [ ] `LiveTerminalRuntime::kill` (uses local PTY) — `terminal_runtime.rs:553–562`.

`TerminalRuntimeUpdate` is the wrapper `apply_output` returned;
delete with it (`terminal_runtime.rs:165–171`). Re-export from
`app.rs:73` and the test reconstructions at `app.rs:15035–15071`
go away with it.

## B. The renderer-side surface-builder duplicate

The proto-side builder (`build_surface_snapshot_from_proto`) becomes
the only builder. Everything below is the alacritty-grid sibling we
delete in #3:

- [ ] `build_surface_snapshot::<T: EventListener>` —
      `terminal_runtime.rs:1080–1314`.
- [ ] `cell_display_text` — `terminal_runtime.rs:1335`.
- [ ] `cell_is_trimmable_blank` — `terminal_runtime.rs:1352`.
- [ ] `cell_copy_text` — `terminal_runtime.rs:1356`.
- [ ] `cell_is_render_blank` — `terminal_runtime.rs:1371`.
- [ ] `terminal_cell_width(cell: &alacritty::Cell)` —
      `terminal_runtime.rs:1390` (do not confuse with
      `panels::terminal_cell_width(window, font_size) -> Pixels`,
      which stays).
- [ ] `effective_background_color(&Cell)` —
      `terminal_runtime.rs:1423`.
- [ ] `resolve_cell_style(&Cell, &Colors)` —
      `terminal_runtime.rs:1456`.
- [ ] `underline_style(&Cell, &Colors, fg)` —
      `terminal_runtime.rs:1522`.
- [ ] `resolve_color(Color, Flags, is_fg, &Colors)` —
      `terminal_runtime.rs:1568`.
- [ ] `resolve_named_color(NamedColor, &Colors)` —
      `terminal_runtime.rs:1595`.
- [ ] `resolve_indexed_color(u8, &Colors)` —
      `terminal_runtime.rs:1599`.
- [ ] `resolve_color_request` + `default_color_request` (only
      reachable from the renderer-side `Event::ColorRequest`
      handler in `apply_output`) — `terminal_runtime.rs:1691,1695`.
- [ ] `window_size_from_grid` (only reachable from
      `Event::TextAreaSizeRequest` in `apply_output`) —
      `terminal_runtime.rs:1674`.

The proto siblings stay (`proto_cell_display_text`, … —
`terminal_runtime.rs:888–982`) and lose the `proto_` prefix as part
of #3, since they become the only versions.

`default_named_color` and `default_indexed_color`
(`terminal_runtime.rs:1603,1638`) are still load-bearing for the
proto path (`resolve_grid_color` in `terminal_runtime.rs:1019` calls
`default_indexed_color`); they move to the theme module in #4 but
do not get deleted here.

## C. The renderer-side scrollback/search surface

Replaced by `Control::TerminalReadScrollback` /
`Control::TerminalSearch` (already implemented daemon-side in
`daemon/src/terminal/frame.rs::{read_scrollback,search}`):

- [ ] `LiveTerminalRuntime::scroll_display` (calls
      `self.term.scroll_display(Scroll::Delta(...))`, no-ops on
      viewer-only runtimes) — `terminal_runtime.rs:535–548`.
- [ ] `LiveTerminalRuntime::scroll_to_match` —
      `terminal_runtime.rs:509–533`.
- [ ] `LiveTerminalRuntime::search_scrollback` (already
      `#[allow(dead_code)]`) — `terminal_runtime.rs:496–504`.
- [ ] `search_scrollback_in_term::<T: EventListener>` (reference
      impl) — `terminal_runtime.rs:609–679`.
- [ ] Renderer call sites that are now wrong on viewer-only:
  - [ ] `app.rs:8838` — `runtime.scroll_to_match(&target)` →
        `App::scroll_terminal_to_match` (#5 lands the helper).
  - [ ] `app.rs:9491` — drag-autoscroll
        `runtime.scroll_display(velocity)` → viewer scroll API.
  - [ ] `app.rs:9575` — wheel scroll → viewer scroll API.

## D. The renderer-side launch fulfillment + dual-write echo

Phase 5d / Phase 4 of design 01 finishes the move. On this RC
branch we delete the `apply_output` hook on the renderer side
(#7); the actual PTY-spawn-in-daemon move lives in a later
unstacked PR.

- [ ] `DRAIN_OUTPUT_BYTE_CAP` constant — `app.rs:151`.
- [ ] `App::drain_terminal_launch_replies` `Output` arm's call to
      `apply_output` (already replaced by
      `try_send_bytes_to_term_task`; the legacy parse just goes
      away with `apply_output` itself) —
      `app.rs:6275–6464` (the whole function survives until daemon
      spawn lands; only the byte-cap + `apply_output` references
      retire here).
- [ ] `App::drain_warm_terminal_launch_replies` same shape —
      `app.rs:6467–6700`. `runtime.apply_output(&bytes)` at
      `app.rs:6571` is the literal call to delete.
- [ ] `App::apply_pty_title_update` and the GPUI `Event::Title /
      ResetTitle / Bell` plumbing in `drain_session_events`'s
      `PtyBytes` arm —
      `app.rs:5227–5246` (helper) and `app.rs:6063–6080` (caller).
      Replaced by `apply_proto_terminal_title` /
      `apply_proto_terminal_bell` already present at
      `app.rs:6776–6810`.
- [ ] `apply_terminal_title_update(tab, &TerminalRuntimeUpdate)` —
      `app.rs:482`. Drop with `TerminalRuntimeUpdate`.
- [ ] `App::drain_session_events` `PtyBytes` arm: stops calling
      `runtime.apply_output(&bytes)` at `app.rs:6063`. Bytes still
      need to reach MCP subscribers (`maybe_emit_tab_output`) and
      the recent-output ring (`append_terminal_recent_output`);
      both of those stay.

## E. Imports + test scaffolding that drag the deletions out

After A–D land, these clean up automatically:

- [ ] `use alacritty_terminal::*` imports in
      `app/src/terminal_runtime.rs:5–11` shrink to whatever the
      proto-only builder still needs (likely just `vte::ansi::Rgb`
      while the palette table lives there; zero imports after #4).
- [ ] `app/Cargo.toml:50` `alacritty_terminal = "0.26.0"` becomes
      unused on the renderer side. Verify with
      `cargo tree -p another-one-app -e normal | rg alacritty`
      after the cutover; if zero, drop the dep.
- [ ] Tests in `terminal_runtime.rs::tests` that drive a local
      `Term<VoidListener>` (`term_from_ansi`,
      `ingest_full_frame_matches_apply_output_text`,
      `search_scrollback_*`, `ayu_*_palette_matches_zed`) need to
      either move to the daemon side (where the Term lives) or be
      rewritten to drive the proto path end-to-end. The parity
      test from #3 supersedes
      `ingest_full_frame_matches_apply_output_text`.

## F. Things that look related but are *not* on the deletion list

Documenting these so a later pass doesn't accidentally rip them:

- `panels::terminal_cell_width(window, font_size) -> Pixels` —
  unrelated to the alacritty `Cell::terminal_cell_width`. Stays.
- `daemon/src/pty.rs::write_input` — daemon-side PTY writer the
  Term task uses. Stays; this is the destination viewer input
  routes to in #6.
- `core/src/terminal_launch.rs` — moves wholesale into the daemon
  in a follow-on PR (Phase 4 of design 01). Out of scope for the
  RC branch.
- `daemon/src/terminal/{task,frame,pacer,launch,mod}.rs` — the
  daemon-canonical implementation. This is what we're cutting
  *over to*; nothing here is on the deletion list.
- `LiveTerminalRuntime::ingest_frame` and the proto cell helpers
  (`proto_*`) — these are the keep-side of #3. They lose the
  `proto_` prefix and absorb new wire fields (#2), but the code
  itself stays.

## Verification gates

A cutover PR is "done" when:

1. `cargo build --workspace` and `cargo test --workspace` clean.
2. `rg 'apply_output|RuntimeEventProxy|LocalPty|local_pty|TerminalRuntimeUpdate|DRAIN_OUTPUT_BYTE_CAP|search_scrollback_in_term|build_surface_snapshot\b|cell_display_text|resolve_cell_style|fn resolve_color\b' app/src` returns zero hits outside this checklist.
3. `cargo tree -p another-one-app -e normal | rg alacritty_terminal` returns zero hits.
4. The parity test from plan step #3 (proto path vs. captured
   golden) passes for every byte sequence currently exercised by
   `ingest_full_frame_matches_apply_output_text`.
