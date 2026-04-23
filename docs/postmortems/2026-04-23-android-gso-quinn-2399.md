# 2026-04-23 · Android QUIC sends silently drop (quinn-rs/quinn#2399)

> After the four-fix [Iroh-on-Android hang](2026-04-23-iroh-android-hang.md)
> was fixed, the emulator worked but a real Pixel 7 Pro hit a new failure:
> TCP/QUIC handshake never completed. Packets left the daemon fine; packets
> left the phone but were dropped by the kernel before hitting the wire.
> Root cause: a known-open quinn bug with Linux UDP GSO on Android.

#postmortem #mobile #iroh #android #quinn #gso

## Symptom

Pixel 7 Pro on the same LAN as the daemon. Ticket parsed, explicit direct
addr passed. Phone got past `endpoint bound, dialing …` and then hung
forever with no error. Daemon never logged `iroh client connected`.

## What actually happens

`noq-udp` (quinn-udp's fork under iroh) enables Linux
[Generic Segmentation Offload][gso] at `setsockopt` time by setting the
`UDP_SEGMENT` socket option. On mainline Linux this kicks in on modern
NICs and is a big throughput win.

On Android kernels (Pixel 7 Pro, `android14-6.1`), `setsockopt` accepts
`UDP_SEGMENT` happily, but the first `sendmsg` call using it fails with
`EIO`. `noq-udp` catches the `EIO`, flips
`max_gso_segments` to 1, **and returns the error — dropping the packet
on the floor**. Because QUIC's handshake retry logic re-sends through
the same socket, every retry prepares another GSO transmit (quinn-proto
has already batched them) and hits the same fate.

End result: no Initial packet ever leaves the phone. Client times out.
Iroh's error path surfaces nothing because at the actor layer it just
looks like no ACK ever arrived.

This is [quinn-rs/quinn#2399][quinn-issue] — open since September 2025.
The same symptom bit Firefox, which carries a [private hotfix][firefox]
in their tree.

[gso]: https://www.kernel.org/doc/html/latest/networking/segmentation-offloads.html
[quinn-issue]: https://github.com/quinn-rs/quinn/issues/2399
[firefox]: https://bugzilla.mozilla.org/show_bug.cgi?id=1877291

## Why it bit us (and not most iroh users)

Most iroh traffic is server-to-server on amd64 Linux, where GSO Just
Works. The bug only surfaces on:
- Android kernels (all of them as of 2026, both arm64 and x86_64 emulators).
- Some older Linux NIC drivers and some virtio-net configurations.

The public `iroh-ffi` mobile bindings don't patch this either, so
anyone shipping iroh on Android directly hits the same wall.

## Fix

We vendored `noq-udp` 0.10.0 at `vendor/noq-udp/`, wired up a
`[patch.crates-io]` entry, and applied a ~30-line change in
`src/unix.rs::send`:

- Detect the `EIO`/`EINVAL` from `sendmsg` on Linux/Android.
- Flip `max_gso_segments` to 1 (this half was already there).
- **New**: if the failing transmit had `UDP_SEGMENT` set, split it
  in userspace (one `sendmsg` per `segment_size` chunk) and return
  `Ok(())` from the retry — instead of dropping the packet.

This mirrors the pattern already used for `EINVAL` + `IP_TOS` on the
same code path. After the patch:

- Pixel 7 Pro dials the daemon and reaches `iroh client connected`.
- Bidirectional stream opens. Resize control frame + terminal bytes
  flow both ways.
- Per-connection log still shows `halting segmentation offload` the
  first time GSO fails, which is expected — that's our signal that
  the fallback path ran.

## Scope of the patch

Only the `send()` function in `unix.rs` (the non-Apple path) is
modified. `send_via_sendmsg_x` (Apple fast path) and `send_gso_fallback`
(batched path, only compiled on Windows/some Linux paths) are
unchanged.

## Risks & unknowns

- The userspace split increases `sendmsg` syscalls proportionally to
  `contents.len() / segment_size` when GSO fails. Since we only hit
  this path once per connection (after which `max_gso_segments` is 1
  and quinn-proto stops batching), the cost is bounded.
- The patch assumes `effective_segment_size().is_some()` is a reliable
  signal that we were doing GSO. It is, per noq-udp's own invariants.

## Remove-when

When upstream quinn-rs/quinn#2399 lands and ships in a `noq`/`iroh`
release we consume, delete `vendor/noq-udp/` and the
`[patch.crates-io]` entry.

## Related

- [Iroh on Android hung silently](2026-04-23-iroh-android-hang.md) —
  the four earlier fixes that had to land before this one was even
  reachable.
