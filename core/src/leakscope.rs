//! Shared atomic counters for PTY-side memory-leak investigations.
//!
//! PTY reader threads call [`record_pty_read`] after every successful
//! `read()`. Pairing this with the desktop-side drain counters lets an
//! observer compute how many bytes are *in flight* between the PTY
//! reader and the UI drain — the delta that was missing when the
//! AppImage silently ballooned to 37 GiB RSS before being OOM-killed.
//!
//! **Always on** in both debug and release builds: each call is one
//! relaxed `fetch_add` on a `AtomicU64` — nanoseconds relative to the
//! ~8 KiB `memcpy` that produced the chunk. Keeping these free means
//! future perf-debugging never has to patch in instrumentation from
//! scratch.
//!
//! The `leakscope` cargo feature is still defined, but now it only
//! forces the desktop-side 1 Hz sampler on in release builds (debug
//! builds auto-start the sampler via `debug_assertions`). The
//! counters themselves no longer depend on it.

use std::sync::atomic::{AtomicU64, Ordering};

static BYTES_READ_TOTAL: AtomicU64 = AtomicU64::new(0);
static CHUNKS_READ_TOTAL: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn record_pty_read(nbytes: usize) {
    BYTES_READ_TOTAL.fetch_add(nbytes as u64, Ordering::Relaxed);
    CHUNKS_READ_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn bytes_read_total() -> u64 {
    BYTES_READ_TOTAL.load(Ordering::Relaxed)
}

#[inline]
pub fn chunks_read_total() -> u64 {
    CHUNKS_READ_TOTAL.load(Ordering::Relaxed)
}

/// True when this build should auto-emit sampler logs.
///
/// * `debug_assertions` — any `cargo run` or dev profile build, so
///   future perf issues surface without having to rebuild with a flag.
/// * `feature = "leakscope"` — opt-in for release builds (CI, packaged
///   binaries) when investigating a specific incident.
#[inline(always)]
pub const fn sampler_auto_enabled() -> bool {
    cfg!(any(debug_assertions, feature = "leakscope"))
}
