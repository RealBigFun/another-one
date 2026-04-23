# AnotherOne Knowledge Vault

This folder is an Obsidian vault: `File → Open folder as vault…` on `docs/`.
Markdown files with `[[wiki-links]]` and tags; no plugins required.

## Layout

- [[apps/desktop]] · [[apps/daemon-sandbox]] · [[apps/mobile]] · [[apps/mobile-core]]
  — one doc per workspace member: what it is, how to run, key files.
- `architecture/` — reusable patterns that have proved out in the codebase.
  Currently: [[architecture/terminal-wrapping-principle]],
  [[architecture/transport-abstraction]],
  [[architecture/peer-to-peer-nodes]],
  [[architecture/frb-tokio-runtime]],
  [[architecture/git-mv-for-restructures]].
- `postmortems/` — real debugging sagas with root cause and fix.
  Currently:
  - [[postmortems/2026-04-23-iroh-android-hang]] — four stacked causes that
    made `irohConnect` hang silently on the emulator.
  - [[postmortems/2026-04-23-android-gso-quinn-2399]] — Android QUIC sends
    silently dropped by `noq-udp`; fixed via vendored fork at
    `vendor/noq-udp/`.

## When to add something

- **App doc** — when a new workspace member (crate or Flutter project) is
  added. Keep the template consistent: "what it is / entry points / run /
  known gaps."
- **Architecture pattern** — when a design decision is made that'll recur
  elsewhere, or when an abstraction/pattern has proven itself in at least
  two places in the codebase. Focus on the *why* and the tradeoff.
- **Postmortem** — when a bug takes longer than ~2h to find, or when the
  root cause is non-obvious enough that future-you would waste time
  rediscovering it. Include symptoms, investigation path, root cause, fix,
  and one-line "lesson" for the vault search.

## Conventions

- File names are kebab-case: `terminal-wrapping-principle.md`.
- Postmortems are prefixed with ISO date: `2026-04-23-iroh-android-hang.md`.
- Cross-link with `[[wiki-links]]` — Obsidian will create graph connections
  automatically.
- Put a one-line summary at the top of every doc so search previews are
  useful.

## Pre-existing

`codex-hooks.md` and `codex-hooks.json.example` predate the vault; kept
as historical references.
