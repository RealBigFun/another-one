# Terminal Production Readiness

This document tracks the Rust daemon and Slint client terminal contract. The
GPUI terminal remains the behavioral baseline, but active implementation is the
Rust daemon plus Slint renderer/client path.

## Scope

- Daemon transport owns PTY lifecycle, attach/detach state, resize routing,
  input routing, and stale-output rejection.
- Slint client owns terminal parsing/rendering, user input capture, visual
  fidelity, and renderer performance.
- Raw `TY_DATA` daemon-to-client output remains the PTY byte stream.
- Slint client-to-daemon input should use typed control frames rather than raw
  `TY_DATA` input frames.

## Input And Reply Contract

- `Control::TabInput` carries `TerminalInputEvent`.
- `TerminalInputEvent` is owned by `another_one_core::terminal_types` so build
  targets and UI shells share the same event model.
- Keyboard, text, paste, focus, mouse protocol bytes, and parser-generated PTY
  replies are represented explicitly.
- Resize remains `Control::TabResize`, because it changes PTY geometry rather
  than stdin bytes.
- Legacy inbound `TY_DATA` input is accepted by the daemon for compatibility.

Evidence:

- `cargo test -p another-one-core terminal_input_event`
- `cargo test -p daemon-sandbox tab_input`
- `cargo test -p daemon-sandbox terminal_input`
- `cargo check -p slint-poc`

## Lifecycle Contract

Each Iroh control stream has at most one attached terminal tab. Attachment state
is `(section_id, tab_id, forwarder)`.

Attach:

- Attaching a different target increments `data_generation`.
- Reattaching the same target keeps the existing generation.
- Replacing a live attachment calls `note_tab_output_observed` and aborts the old
  forwarder.
- Attaching a different target clears the viewer's prior resize/focus claims via
  `viewer_disconnected`.
- If the runtime is not live yet, the daemon records a pending attachment with no
  forwarder so resize/input/launch intent still targets the requested tab.

Detach:

- Detach increments `data_generation`.
- Detach aborts any live forwarder.
- Detach observes buffered output for the previous live tab.
- Detach clears the viewer's resize/focus claims with `viewer_disconnected`.
- Input while detached is dropped.

Stale output:

- Outbound `TY_DATA` frames are tagged with the attachment generation active when
  the forwarder queued them.
- The writer drops `TY_DATA` frames whose generation no longer matches the
  current connection generation.
- This generation gate is the successor to the older 200 ms stale-byte ignore
  window; it is deterministic and does not depend on wall-clock timing.

Evidence:

- `cargo test -p daemon-sandbox handle_control`
- `cargo test -p daemon-sandbox pending_attach`

Covered cases:

- live attach then detach cleans state and advances generation;
- same-target reattach preserves generation;
- retarget attach advances generation and clears prior viewer state;
- pending attach keeps resize and launch routing available;
- typed input reaches the attached tab;
- input without attachment is dropped.

## Slint Renderer Evidence

The Slint renderer now consumes Alacritty cells into batched text,
background, and cursor spans.

Implemented coverage:

- ANSI foreground colors resolve into Slint text runs.
- Combining marks stay attached to the leading text run.
- CJK and emoji wide cells preserve terminal cell occupancy.
- ZWJ emoji continuations compact into the leading wide cell so following text is
  not shifted by the internal Alacritty continuation cells.
- Styled run boundaries split correctly after wide cells.
- Beam and underline cursors render as cursor spans; hidden cursors emit no
  cursor span; hollow-block cursor mapping is represented for the Slint layer.
- OSC8 hyperlinks render as terminal link spans; primary-click fallback resolves
  the clicked cell to a URI and opens through the platform seam when terminal
  mouse reporting is disabled.
- Terminal selection is represented as batched cell spans in the Slint layer.
  Pointer drags select only when terminal mouse reporting is disabled; selected
  text is extracted from the Alacritty grid so wide cells and combining marks
  copy as text rather than as lossy codepoints.
- Ctrl+C copies an active terminal selection through a narrow platform clipboard
  seam; without a selection it remains terminal input and can still reach the
  PTY as interrupt input.
- The Slint key path encodes cursor keys after reading active Alacritty modes,
  so application-cursor mode switches arrow/home/end sequences from CSI to SS3.
- The Slint pointer/focus path reads Alacritty terminal modes before sending
  input: focus reports are only sent after `?1004`, mouse clicks/motion only
  after mouse reporting modes, SGR mouse is preferred after `?1006`, and legacy
  X10 encoding remains the fallback.
- The render loop no longer uses an always-on 33 ms ticker. PTY output schedules
  a dirty-only coalesced flush; idle panes do not wake from a frame interval.

Evidence:

- `cargo test -p slint-poc`
- `cargo check -p slint-poc`
- Live debug hot-reload window: `AnotherOne` /
  `com.anotherone.Slint` on Hyprland workspace 1.
- Provisional idle debug sample after dirty-only scheduling:
  `top -b -n 3 -d 1 -p 3015109` reported `0.0%`, `0.0%`, then `1.0%` CPU with
  approximately `175480 KiB` RSS. This is a debug/hot-reload sample, not the
  final release performance gate.

## Remaining Gates

- Slint visual proof for grapheme/wide-cell rendering, ANSI/indexed/truecolor
  colors, cursor states, selection, and restored/failed tab states.
- Slint renderer throughput proof under sustained PTY output.
- Idle CPU and memory measurements for the Slint terminal pane.
