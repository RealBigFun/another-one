# Peer-to-peer nodes

> Every machine running AnotherOne is an Iroh endpoint. Every client —
> mobile, desktop UI, CLI, web — picks which endpoint to dial. The
> desktop app defaults to dialing itself (in-process), but is free to
> dial any other paired endpoint.

#architecture #north-star

## The model

```
 Desktop app (laptop)            Headless daemon (home server)
   core library                    core library
   + GPUI UI (client of core)      + no UI
   Iroh endpoint, id = E1          Iroh endpoint, id = E2
         \                          /
          \     user's peer mesh   /
           \___     (just IDs)    _/
                \                /
                 Mobile (paired with E1, E2, …)
                 Web / CLI (same story)
```

The daemon-that-hosts-sessions is a library, not a process. The desktop
process = `{ core library + GPUI client }`. A headless node = `{ core
library + no client }`. A phone = `{ client only }`.

## Consequences

- **The desktop UI is just a client.** It connects to its own embedded
  core through an `InProcessTransport` — logically equivalent to dialing
  its own EndpointId via Iroh, just optimized to skip the network. This
  means the desktop's terminal rendering goes through the same
  [[transport-abstraction]] as mobile's.

- **"My sessions" is a per-node thing.** The laptop hosts the sessions
  you started there; the home server hosts the ones you started there.
  Clients pick which node to connect to. (Cross-node session migration
  is interesting but not on the near-term roadmap.)

- **One protocol, one session-holding surface.** ALPN
  `anotherone/pty/1`, the `core` crate for sessions, same for every
  node. No special path for "local" vs "remote"; the only difference is
  which transport implementation the client picks.

- **Pairing is uniform.** User pairs `laptop` + `home-server` + `phone`
  + `ipad`. Each pairing stores `(peer_id, display_name, cap_token)`.
  Client picks from the roster at connect time.

- **Headless works.** You can run a node on a VPS/homelab with no
  display; mobile dials it the same way it dials your laptop. Keep your
  workstation asleep; your Claude-Code session continues on the server.

## What makes this real

1. **Phase 1 core extraction** from `desktop/src/app.rs`. Until the
   session-holding logic is a library consumable by multiple binaries,
   "daemon is a library" is aspirational.
2. **A headless-daemon binary** that wraps the same core. Today
   [[../apps/daemon-sandbox]] is a rough precursor; post-Phase-1 it
   becomes thin: `fn main { core::serve_iroh() }`.
3. **An `InProcessTransport` for desktop.** Same interface as
   `IrohTransport` but implemented as direct calls into `core`. This is
   what lets desktop default to itself.
4. **Pairing flow** that produces a roster and lets the client pick.

## Related design choices

- [[terminal-wrapping-principle]] applies unchanged to every node.
- [[transport-abstraction]] is what the client side of this picture uses.
- [[frb-tokio-runtime]] is how mobile's `IrohTransport` hangs together
  today.

## Open questions (worth noting)

- **Roster sync across a user's devices.** If you pair a new phone,
  should it automatically know about the laptop and home-server? Or does
  each client pair independently? Leaning toward independent for now,
  revisit when it becomes painful.
- **Capabilities per peer.** "This phone can drive the home-server but
  not the work laptop." Easy to imagine, out of scope for first release.
- **Discovering local nodes.** mDNS for LAN makes sense; off-LAN
  requires the user knows the EndpointId (from pairing).

## Not this

- **Not a centralized coordinator.** Nothing "knows" about all your
  nodes; it's purely peer-to-peer with Iroh's relay mesh as transport
  fallback. No project-operated infrastructure beyond relays and the
  optional push-notification relay.
- **Not a distributed filesystem.** Files stay on whichever node owns
  them. If you want to edit a file on another node, you dial that node
  and drive the agent there.
