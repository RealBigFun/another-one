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

## Load-bearing consequences

- [[../apps/daemon-sandbox]] exposes raw PTY bytes, not structured
  messages. Control (resize) is a small JSON control channel, not
  tool-aware.
- [[../apps/mobile]]'s `TerminalTransport` interface is byte-in, byte-out.
  The terminal renderer parses ANSI; nothing above it tries to "understand"
  agent output.
- Client UIs do not template agent prompts, extract commands from agent
  output, or render agent-specific elements. A bash session and a Claude
  Code session are the same protocol; only the cell grid differs.

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

- [[../postmortems/2026-04-23-iroh-android-hang]] — the mobile transport
  work that keeps this principle intact.
- [[transport-abstraction]] — how the byte-in/byte-out shape propagates
  through the code.
