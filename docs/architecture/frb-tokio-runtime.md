# Dedicated tokio runtime for flutter_rust_bridge

> `flutter_rust_bridge`'s default async executor is not a tokio runtime.
> Rust code called from Dart that uses tokio (iroh, reqwest, hyper, most
> networking crates) must bridge onto its own runtime, or async
> operations hang silently.

#pattern #rust #ffi

## The problem

FRB 2.x runs Rust `async fn` on a thread-pool executor (feature
`thread-pool`, set in `default`). That executor polls futures but
doesn't provide the tokio I/O driver, timer driver, or the tokio
`Handle::current()` that `tokio::spawn` relies on.

Crates like iroh use `tokio::net::UdpSocket` and spawn internal actor
tasks via `tokio::spawn`. Without a tokio runtime in context, those
spawns succeed (the crate compiles and runs) but the spawned tasks
never get polled. Outer `await`s sit on `Pending` forever with no
visible error — the task is simply never scheduled.

## The pattern

Spin up a dedicated tokio runtime as a static singleton, and delegate
all tokio-dependent work onto it:

```rust
use std::sync::OnceLock;
use tokio::runtime::Runtime;

fn tokio_rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("mobile_core-tokio")
            .build()
            .expect("build tokio runtime")
    })
}

pub async fn iroh_connect(endpoint_id: String) -> anyhow::Result<IrohSession> {
    // The inner future runs on OUR tokio runtime, polled by tokio's
    // scheduler. The outer await on JoinHandle is runtime-agnostic —
    // Dart's FRB executor can poll it fine.
    tokio_rt()
        .spawn(async move { iroh_connect_inner(endpoint_id).await })
        .await
        .map_err(|e| anyhow::anyhow!("connect task panicked: {e}"))?
}
```

All further tokio work inside `iroh_connect_inner` (including
additional `tokio::spawn` calls for per-connection send/recv tasks)
lands on this same runtime via `Handle::current()`.

Eagerly initialize the runtime in your `#[frb(init)]` to keep
first-call latency predictable:

```rust
#[frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
    let _ = tokio_rt(); // warm the lazy static
}
```

## When to use

- Any FRB API that calls into tokio-based crates (iroh, reqwest, hyper,
  tokio-tungstenite, sqlx, ...).

## When not to use

- Pure-sync APIs (crypto, data transforms, file I/O via `std::fs`)
  don't need it; FRB's default executor handles them fine.

## Gotchas

- Holding a `JoinHandle` or its parent task across the FRB boundary is
  fine; wakers are cross-runtime compatible for `JoinHandle`.
- Returning `async fn` values that contain unawaited tokio futures is
  not; make sure you `await` inside the `spawn`'d future, and only
  return plain values (or FRB-opaque handles whose bodies stash tokio
  state).
- The runtime lives for the lifetime of the process. Don't try to drop
  or reset it between calls.

## Observability note

Install a tracing subscriber explicitly (`tracing-android` layer on
Android, `tracing_subscriber::fmt` elsewhere). FRB's
`setup_default_user_utils()` wires `log` → `android_logger`, but it
does not wire `tracing` anywhere, so any library using `tracing`
(iroh, hyper, etc.) is invisible unless you set up your own subscriber.

## See also

- [[../postmortems/2026-04-23-iroh-android-hang]] — discovery path that
  led to this pattern.
- [[../apps/mobile-core]] — current implementation.
