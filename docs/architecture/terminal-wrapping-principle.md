# Terminal-wrapping principle

> AnotherOne wraps raw PTY sessions rather than talking to each agent
> tool's API/protocol. The app inherits new agent behavior for free;
> reject any design that re-introduces per-tool scaffolding.

#core-principle

## Why

The agent-CLI ecosystem changes constantly — new tools ship, existing
ones change their output, features are added. A design that speaks each
tool's protocol (or parses each tool's output) means continuous
maintenance to stay current with every vendor.

Wrapping a PTY makes the app transport-agnostic and vendor-agnostic: if
Anthropic ships a new Claude Code release, or a new agent-CLI appears,
AnotherOne supports it without a code change. The byte stream is the
interface.

## What this principle is and is not about

The principle forbids **per-tool / per-agent semantic parsing** — the
app must not understand "what Claude said" or "what Codex returned."
It does **not** forbid parsing the universal terminal protocol
(ANSI/VT escape sequences). Every terminal renderer parses ANSI; the
question is *where* in the system it happens.

Decided in [[../designs/01-daemon-canonical-terminal]] (#158): the
VT/ANSI parser may live daemon-side. Per-tab `alacritty_terminal::Term`
state moves into the daemon's tokio runtime so:

- one `Term` per PTY, regardless of how many viewers (desktop +
  mobile + future) attach;
- viewers consume daemon-emitted grid frames
  (`daemon_proto::TerminalFrame`) instead of re-parsing the byte
  stream locally;
- inactive tabs cost zero on every viewer (no parse, no allocation,
  no traffic).

This is a refinement, not a violation: the daemon still doesn't
understand agent semantics. It runs the same VT state machine every
client ran before; the only thing that changed is the side of the
wire it runs on.

## Load-bearing consequences

- [[../apps/daemon-sandbox]] exposes raw PTY bytes **and** grid
  frames. Bytes remain available in-process for tools that
  legitimately wrap a PTY byte stream (in-process MCP `tab_output`).
  Snapshot frames are the default subscription for renderers
  (desktop, mobile).
- Control (resize, input, search, scrollback fetch) is a typed
  control channel, not tool-aware. New verbs land in
  `daemon_proto::Control`; daemon-side handlers operate on the
  canonical `Term`, not on agent output.
- [[../apps/mobile]]'s renderer consumes `TerminalFrame` snapshots
  through the same `Session` interface desktop uses. Nothing above
  the renderer tries to "understand" agent output.
- Client UIs do not template agent prompts, extract commands from
  agent output, or render agent-specific elements. A bash session
  and a Claude Code session are the same protocol; only the cell
  grid differs.

## Where the principle gets tested

- **Permission approvals** (e.g. Claude Code's "allow this edit?" prompts)
  are the strongest temptation to violate this. Our current stance: if
  approvals become important enough to intercept, reach for the agent's
  *supported* control surface (MCP hooks, structured stdout modes) rather
  than scraping the PTY. Document any exception carefully; once we cross
  this line for one tool, the maintenance debt is permanent.
- **Session restore across agent restarts.** See `docs/codex-hooks.md` for
  Codex; the implementation reads session log files rather than parsing
  TTY output.

## See also

- [[../designs/01-daemon-canonical-terminal]] — daemon-canonical Term;
  the refinement above lives there with full rationale, rejected
  alternatives, and implementation phases.
- [[../postmortems/2026-04-23-iroh-android-hang]] — the mobile transport
  work that keeps this principle intact.
- [[transport-abstraction]] — how the byte-in/byte-out shape (and now
  the frame-out shape) propagates through the code.
