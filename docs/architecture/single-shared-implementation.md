# Single Shared Implementation, Thin Platform Bootstraps

**Status:** locked architectural requirement.
**Decision tracked in beads as:** `another-one-arc` (this doc is the long form).

## The principle

> Every feature must work on every targeted platform.
> Mobile is a thin bootstrap mask over a single shared implementation,
> not a parallel codebase with its own gaps.
> Wherever a variant is genuinely needed, the seam is a **trait**, not
> a runtime `if-platform-then-fast-path` branch.

This applies across the Rust core, the bridge, and the Flutter UI.
The platform-specific code on each target is constrained to:

* Boot.
* Wire the terminal engine into the platform's display surface.
* Hand a `DaemonConnection` to the rest of the app.

Everything else — every screen, every provider, every state machine,
every verb — is shared.

## Why this matters

We came into this migration with a two-transport design (FFI for the
local embedded daemon, iroh for remote daemons). That decision
guaranteed parallel Dart-side implementations of every verb and a
permanent maintenance tax. The desktop got a fast path; the mobile
client got a stubbed one. Mobile pairing worked but `addProject`,
`renameTask`, `findPullRequestStatus`, and 17 other verbs all threw
`UnimplementedError` at runtime. The remediation was an "iroh wire
parity" epic to chase the FFI surface down. **That framing is the
wrong target.** The right target is to delete the FFI surface and let
both desktop and mobile speak the same wire.

## What that means concretely

### One transport, dialled differently per bootstrap

```
core/  +  another-one-bridge/   ← one daemon, listens on iroh.

Desktop bootstrap:
  1. Boot the embedded daemon (already exists). Listen on iroh,
     secret key pinned to a stable per-host loopback NodeId.
  2. Open a `DaemonConnection` against `iroh://<own-NodeId>`.
  3. Use it. Same code path mobile takes against a remote NodeId.

Mobile / remote-desktop bootstrap:
  1. Skip the daemon boot.
  2. Open a `DaemonConnection` against the paired NodeId.
  3. Use it. Same code path.
```

The Dart-side `DaemonConnection` interface stays. The two
implementations (`LocalSession`, `IrohSession`) collapse into one. The
only branch left is "do I boot a local daemon or not?"

### Self-pair semantics

The TOFU pairing handshake is built for unknown peers. When desktop
dials its own NodeId, friction must be zero. Pre-allowlist the
loopback NodeId in the daemon's `paired_peers` set at boot, or
short-circuit the handshake entirely when `peer.node_id ==
own.node_id`. The bootstrap should not surface a pair UI to the user
when desktop is talking to itself.

### Variants live behind traits

This is the core invariant. If two things need to differ — by
platform, by performance, by environment — the seam is a trait, and
the bootstrap picks the impl. Not a runtime `if cfg!(target_os =
"…")` inside otherwise-shared code. Not a feature flag carved into
one struct.

Concrete trait seams already in the tree or scoped:

| Trait | Owner | Variants |
|-------|-------|----------|
| `HeadlessPlatform` | `core::platform` | `LinuxPlatform`, `MacosPlatform`, `WindowsPlatform`, `IosPlatform`, `AndroidPlatform` |
| `TerminalEngine` | `core::terminal_engine` | `AlacrittyEngine` (default), `XtermDartShimEngine` (fallback for any target where `alacritty_terminal` won't link) |
| `DaemonConnection` (Dart) | `another-one/lib/src/connection.dart` | future single iroh-backed impl, plus mock impls for tests |

Concrete trait seams to add as the iroh-only path lands:

| Trait | Owner | Why a trait, not a branch |
|-------|-------|---------------------------|
| `PtyByteStream` | `core::transport` (new) | PTY byte streaming is the only verb where loopback throughput might justify a shared-memory ring-buffer instead of QUIC frames. If that need ever materialises, it lands as an alternate impl behind the trait — selected by the bootstrap that knows the connection is loopback — not as an `if loopback then fast_path` branch inside the iroh transport. The default impl (QUIC frames) ships first; the ring-buffer impl ships only if benchmarks demand it. |

The general rule when the next variant question comes up: **trait
first, branch never.**

## Migration path (gradual, in order)

The single-shared-impl target is the destination. The current branch
ships LocalSession + a stubbed IrohSession. Going from here to there
without breaking in-flight work:

1. **Build the iroh wire surface to feature-complete.** That's the
   existing `another-one-ojm` epic — every Control / WorkerReply
   variant that LocalSession serves over FFI today, served over iroh
   too. Foundation task (`ojm.1`) lands ALPN bump + request_id
   correlation + DaemonRegistry trait surgery first; domain children
   (`ojm.2..8`) follow in any order.
2. **Swap the desktop bootstrap to loopback iroh.** Once IrohSession
   is at parity, the desktop boots the daemon, opens a self-targeted
   iroh connection, and uses it. No widget code changes.
3. **Throughput verification.** Measure sustained PTY throughput on
   the new path. Burst at 10 MB/s for 30 minutes; assert no drops, no
   visible jank, CPU within 2× of the FFI baseline.
4. **Delete LocalSession.** The `another-one-bridge::api::local_session`
   module, the FRB-bound class on the Dart side, the boot path that
   wires the in-process registry. All gone in a single cleanup PR.
5. **Iterate on `PtyByteStream` only if step 3 forces it.** YAGNI
   otherwise.

Steps 1 and 4 are the bookends. Steps 2 and 3 are the actual
transition. None of it requires the LocalSession surface to be
half-deleted while in flight.

## Out of scope for this requirement

* Web target. Not on the platform list. If it ever lands, it joins
  the same trait-selected story (`DaemonConnection` over WebRTC or
  similar; `TerminalEngine` whichever-shim-can-link).
* Multi-daemon UX surface beyond "the architecture supports it." The
  migration plan already calls this out as a follow-up after parity
  is verified.
* Caching layers. Verbs are remote-able directly today; a cache fits
  inside a `DaemonConnection` decorator if it ever needs to.

## Acceptance — how we know we're there

* `another-one-bridge::api::local_session` is deleted.
* `connection.dart` exposes one `DaemonConnection` impl.
* `flutter run -d linux` and `flutter run -d <android-pixel>` boot
  against the same Dart codebase, hit the same daemon code, and
  exercise every screen with no `UnimplementedError` thrown.
* Adding a new feature requires touching one set of files (the
  shared core + bridge + Dart UI). No "and now the mobile equivalent"
  PR follows.
* Any future variant lands as a trait impl, not as a branch inside
  shared code.
