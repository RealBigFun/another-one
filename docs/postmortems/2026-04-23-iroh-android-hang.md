# 2026-04-23 · Iroh on Android hung silently

> Historical/obsolete-context note: this postmortem documents the abandoned Flutter/Dart mobile experiment. Keep the Android networking/runtime lessons, but do not treat the Flutter/mobile-core references as current architecture.

> `Endpoint::bind().await` sat Pending forever on the Android emulator
> with no error, no timeout firing, no iroh log messages. Root cause
> was **four independent issues stacked**; fixing any one alone
> didn't help.

#postmortem #mobile #iroh #frb

## Symptom

Dart calls `irohConnect` → enters Rust → logs `iroh_connect: binding
local endpoint` → **silence for 30+ seconds**. No timeout error, no
panic, no stderr. `adb logcat` shows iroh creates one tracing span
("endpoint; id=…") that opens and closes in 150 ms, then nothing.

## Why it was hard to diagnose

- No error surfaced. Futures just stayed `Pending`.
- `tokio::time::timeout(..., bind())` also never fired, which is only
  possible if the task carrying the timeout isn't being polled.
- Our own `tracing::info!` calls from my code *did* reach logcat, so it
  looked like code was running — just iroh wasn't.

## Root causes (all four were needed)

### 1. FRB's executor isn't a tokio runtime

`flutter_rust_bridge` 2.x defaults to a thread-pool executor. iroh's
UDP sockets and spawned actor tasks need tokio. Calling iroh from an
FRB async fn means `tokio::spawn` succeeds but the spawned work is
never polled. Outer awaits hang with nothing scheduled.

**Fix**: `mobile-core` builds its own `tokio::runtime::Runtime`
(multi-thread, 2 workers) as a `OnceLock` static and delegates
`iroh_connect_inner` onto it with `tokio_rt().spawn(async move {
  ... }).await`. See [[../architecture/frb-tokio-runtime]].

### 2. `presets::N0` attaches a DNS lookup that hangs on Android

iroh-relay's `DnsResolver::default()` calls `with_system_defaults()`,
which tries to read `/etc/resolv.conf`. iroh's own source (iroh-relay
0.98.0, `src/dns.rs:254`) literally comments:

> We first try to read the system's resolver from `/etc/resolv.conf`.
> This does not work at least on some Androids, therefore we fallback
> to the default `ResolverConfig`…

The fallback exists but doesn't reliably kick in on the Pixel 7
emulator (API 36) — reading or timing out on that path stalled bind
before anything useful happened.

**Fix**: use `presets::Minimal` (crypto provider only), `RelayMode::Disabled`,
and inject an explicit `DnsResolver::with_nameserver("8.8.8.8:53")`.

### 3. No way to resolve EndpointId → addresses

With relay disabled *and* DNS address lookup disabled, iroh has no
way to turn an EndpointId into a network address. The sandbox has no
address-lookup service. `connect(EndpointAddr::new(id), ALPN)` fails
silently — the connection tries, finds nothing dial-able, and closes.

**Fix**: `iroh_connect` takes `direct_addrs: Vec<String>`. The Dart
URL format became `iroh://<endpoint_id>?direct=host:port[,host:port...]`.
Daemon prints its EndpointAddr (with LAN IPs) on startup; caller pastes
the useful one (`10.0.2.2:<port>` for the Android emulator).

### 4. Async race in `_tearDownTransport`

Independent of everything above: once the first three were fixed, the
connection succeeded, but mobile immediately called `session.close()`.
Stack trace showed `IrohTransport.close` called by
`_TerminalPageState._tearDownTransport`.

The problem: `_connect()` called `_tearDownTransport()` without
awaiting it, then synchronously created the new transport and assigned
`_transport = transport`. `_tearDownTransport` is async; its first
`await` suspended before capturing `_transport` into its local `t`
variable, and when it resumed it captured the **new** transport
instance and closed it.

**Fix**: snapshot all owned state synchronously at the top of
`_tearDownTransport` before any await; `_connect()` now awaits the
teardown before creating the replacement.

## Lessons

- **`async fn` parameter capture order matters.** If a state field can
  change between calls, capture it into a local before the first
  `await`, or the continuation will read the new value. See
  `mobile/lib/main.dart::_tearDownTransport` comment.
- **When no error surfaces and no timer fires, your task isn't being
  polled.** Either the runtime is wrong (our case #1) or it's been
  forgotten (detached task).
- **Check the library's doc comments.** iroh's DnsResolver comment
  said the Android case was known-broken; I found it only after
  reading the source. A single grep for "android" in the crate would
  have pointed here earlier.
- **Instrument from both sides.** `adb logcat` showed only my `tracing::info!`
  lines; FRB didn't wire a tracing subscriber, so iroh's events weren't
  visible until I added `tracing-android` explicitly in `init_app`.

## Verification

On Pixel 7 API 36 emulator, tap Connect with URL
`iroh://<EndpointId>?direct=10.0.2.2:<UDP-port>`:

- Status goes to `connected`
- bash prompt renders in TerminalView
- Typed input echoes, commands run (`ls`, `echo`, etc.)

Commit `a21aad3` lands the combined fix.

## Followups

- **xterm.dart compatibility with Claude Code** — separate issue, not
  caused by this set of fixes. Claude uses kitty keyboard protocol
  (`\x1b[<u`), synchronized output (`\x1b[?2026h`), focus reporting —
  xterm.dart's parser stalls. Manifests as blank TerminalView when
  daemon is running Claude. Workarounds: patch xterm.dart, pre-strip
  these sequences daemon-side, or swap renderer.
- **Iroh self-hosted relay**. Public Number Zero "canary" relay mesh
  is dev-only; production needs a self-hosted `iroh-relay` per Iroh's
  own recommendation.
- **iroh 0.98 is pre-1.0** — breaking changes every few minor
  versions. Pin aggressively; budget for periodic migration.
