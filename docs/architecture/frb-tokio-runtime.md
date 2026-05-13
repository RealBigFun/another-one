# Dedicated tokio runtime for embedded network clients

> Tokio-based networking must run on a real tokio runtime, regardless of the UI framework or host event loop embedding it.

#pattern #rust #async

## The problem

Desktop GPUI and Android host code do not guarantee that a tokio runtime is current on the thread that starts client networking. Crates like iroh use `tokio::net::UdpSocket`, timers, and internal actor tasks via `tokio::spawn`. Without a tokio runtime in context, async work can stall or panic even though the crate compiles.

## The pattern

Spin up a dedicated tokio runtime as a static singleton in the networking crate, then delegate all tokio-dependent work onto it:

```rust
use std::sync::OnceLock;
use tokio::runtime::Runtime;

fn tokio_rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("daemon-client")
            .build()
            .expect("build tokio runtime")
    })
}

pub fn connect(url: String) {
    tokio_rt().spawn(async move {
        connect_inner(url).await;
    });
}
```

All further tokio work inside `connect_inner` lands on this same runtime via `Handle::current()`.

## Current implementation

`daemon-client/src/session.rs` owns the dedicated runtime used by the Iroh client. The GPUI app and Android library build can call into `daemon-client` from non-tokio contexts while iroh's UDP sockets, timers, and frame pumps remain driven.

The embedded desktop daemon has a separate runtime in `app/src/daemon_host.rs` because it runs server-side daemon tasks, PTY forwarding, MCP/UDS listeners, and Iroh endpoint accept loops.

## When to use

- Any library entry point that starts iroh, reqwest, hyper, tokio-tungstenite, sqlx, or other tokio-based async crates from a non-tokio host.
- Long-lived background networking clients that need timers and spawned tasks to keep running independently of the UI frame loop.

## When not to use

- Pure synchronous APIs (crypto, data transforms, file I/O via `std::fs`).
- Code already running inside the daemon runtime or a clearly-owned tokio runtime; use the current handle instead of nesting runtimes unnecessarily.

## Gotchas

- Do not hold locks across runtime boundary calls; clone handles or channels first.
- Make shutdown explicit where needed. Static runtimes live for the process lifetime.
- Keep UI callbacks out of tokio tasks; send events over channels and let the UI drain them on its own thread.

## Observability note

Install a tracing subscriber explicitly for each host. Android and desktop have different logging sinks, and libraries using `tracing` will be invisible unless a subscriber is configured.

## See also

- [[../postmortems/2026-04-23-iroh-android-hang]] — historical debugging path that exposed this runtime requirement.
- [[transport-abstraction]]
- `daemon-client/src/session.rs`
- `app/src/daemon_host.rs`
