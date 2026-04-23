# 2026-04-23 · Relay path validated end-to-end on cellular

> After the LAN-direct path was working on the Pixel 7 Pro, we needed to
> prove the dev relay could carry the connection off-LAN. Turned wifi off
> on the phone, pushed a build whose default endpoint URL had **only** a
> relay entry (no direct addrs), and tapped Connect. It worked.

#postmortem #mobile #iroh #relay #cellular

## What this test proves

- Direct addrs in the ticket are LAN-only (192.168.x) plus a home public
  IP (107.204.x). Neither is reachable from the phone on cellular
  without hole-punching through the home NAT.
- The build used for this test had `direct=[]` passed through
  `iroh_connect`. Zero direct addrs. If the connection came up, it
  could only be the relay path.
- The phone emitted `iroh::socket::transports::relay::actor: home is now
  relay https://usw1-1.relay.n0.iroh-canary.iroh.link./, was None` —
  phone registered with the dev relay.
- Daemon logged `iroh client connected remote=dddb2dd8…`, resize frames
  flowed on keyboard open/close, `echo test` round-tripped in under a
  second of perceived latency.

## What changed to enable it

Two commits' worth of diff:

**Daemon** (`daemon-sandbox/src/transport_iroh.rs`) — ticket now includes
`relay=<url>` lines alongside `addr=<ip:port>` lines. Previously the
ticket file only carried direct addrs, so off-LAN clients had no way to
learn the relay URL.

**Mobile-core** (`mobile-core/src/api/iroh_client.rs`) — `iroh_connect`
now takes `relay_urls: Vec<String>` in addition to `direct_addrs`.
Relay mode flips from `Disabled` to `Default` whenever at least one
relay URL is supplied (no relay URL → stay on the LAN-only direct
path). Both are attached to the `EndpointAddr` via `with_ip_addr` and
`with_relay_url`, and at least one of the two must be non-empty or we
refuse the call.

**Client plumbing** — `mobile/lib/src/transport_iroh.dart` grew a
`relayUrls` parameter; `mobile/lib/main.dart`'s `iroh://…` URL parser
accepts `&relay=<url>[,<url>…]` with comma separation mirroring the
existing `&direct=…` syntax.

## Caveat worth remembering

The relay we used is `usw1-1.relay.n0.iroh-canary.iroh.link` — part of
N0's **development/testing** mesh. Rate-limited and explicitly not for
production per their docs. We've validated the path works on that
infrastructure; real rollout needs a self-hosted `iroh-relay` or a paid
Iroh Services tier. See [[peer-to-peer-nodes]] and the main plan's
"Phase 4: production infrastructure" for the ops path.

## What still isn't tested

- **Latency under real load** — `echo test` is tiny. A streaming agent
  session (`claude` producing bursts of output) on cellular through
  the relay is a separate measurement. Acceptable interactive feel is
  still assumed, not proven.
- **Reconnect / resume** — the phone hasn't been backgrounded, put to
  sleep, or switched networks mid-session. Session survival across
  those is the next thing to exercise.
- **Hole-punching** — since this build passed zero direct addrs, we
  didn't exercise the direct path from cellular. Some CGNAT setups
  allow hole-punching; separate test.

## Related

- [[2026-04-23-iroh-android-hang]] — four fixes that were needed to
  get Iroh binding at all on Android before any path worked.
- [[2026-04-23-android-gso-quinn-2399]] — the GSO patch that had to
  land before a real phone (as opposed to the emulator) could send
  any QUIC packet.
