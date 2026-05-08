//! Shared atomic counters for PTY-side memory-leak investigations.
//!
//! PTY reader threads call [`record_pty_read`] after every successful
//! `read()`. Pairing this with the desktop-side drain counters lets an
//! observer compute how many bytes are *in flight* between the PTY
//! reader and the UI drain — the delta that was missing when the
//! AppImage silently ballooned to 37 GiB RSS before being OOM-killed.
//!
//! They also call [`record_pty_send_block_ns`] after each
//! `SyncSender::send` into the bounded launch-reply channel. The
//! GUI-lockup investigation (issue #125) needs to know when the
//! reader thread was backpressured by a full channel — a long
//! `send()` means the GPUI drain is falling behind (hypothesis A:
//! drain starvation) or is deadlocked against it (hypothesis B:
//! MCP sync-channel reentrancy). Counters here stay platform-neutral
//! and the desktop sampler prints them.
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

// Reader-thread SyncSender::send block-time histogram. See
// `terminal_launch::launch_terminal` for the call sites. All times
// are nanoseconds. We keep a coarse histogram rather than a full
// distribution because the sampler prints them at 1 Hz and the
// interesting signal is just "is backpressure rare or chronic, and
// how deep does it get?".
static SEND_BLOCK_TOTAL: AtomicU64 = AtomicU64::new(0);
static SEND_BLOCK_MAX_NS: AtomicU64 = AtomicU64::new(0);
static SEND_BLOCK_SUM_NS: AtomicU64 = AtomicU64::new(0);
static SEND_BLOCKS_OVER_1MS: AtomicU64 = AtomicU64::new(0);
static SEND_BLOCKS_OVER_10MS: AtomicU64 = AtomicU64::new(0);
static SEND_BLOCKS_OVER_100MS: AtomicU64 = AtomicU64::new(0);
static SEND_BLOCKS_OVER_1S: AtomicU64 = AtomicU64::new(0);

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

/// Record one `SyncSender::send` duration observed on a PTY reader
/// thread. Separating this from `record_pty_read` lets the sampler
/// tell apart "kernel is delivering bytes quickly" from "reader was
/// blocked on the GPUI drain". Only failures that matter are *long*
/// sends; the accumulators below make those visible without forcing
/// the caller to own any state.
#[inline]
pub fn record_pty_send_block_ns(ns: u64) {
    SEND_BLOCK_TOTAL.fetch_add(1, Ordering::Relaxed);
    SEND_BLOCK_SUM_NS.fetch_add(ns, Ordering::Relaxed);
    // `fetch_max` is stable since Rust 1.45. Relaxed is fine — we
    // don't care about interleaving with other counters, just that
    // the maximum monotonically tracks the worst case we've seen.
    SEND_BLOCK_MAX_NS.fetch_max(ns, Ordering::Relaxed);
    if ns >= 1_000_000 {
        SEND_BLOCKS_OVER_1MS.fetch_add(1, Ordering::Relaxed);
    }
    if ns >= 10_000_000 {
        SEND_BLOCKS_OVER_10MS.fetch_add(1, Ordering::Relaxed);
    }
    if ns >= 100_000_000 {
        SEND_BLOCKS_OVER_100MS.fetch_add(1, Ordering::Relaxed);
    }
    if ns >= 1_000_000_000 {
        SEND_BLOCKS_OVER_1S.fetch_add(1, Ordering::Relaxed);
    }
}

/// Snapshot of the send-block counters at a point in time. Exposed
/// as a struct so the sampler can pull them atomically-enough in one
/// pass (still racy against reader threads, but that's fine — the
/// numbers are monotonic and we only read them for logging).
#[derive(Clone, Copy, Debug, Default)]
pub struct SendBlockStats {
    pub total: u64,
    pub sum_ns: u64,
    pub max_ns: u64,
    pub over_1ms: u64,
    pub over_10ms: u64,
    pub over_100ms: u64,
    pub over_1s: u64,
}

pub fn send_block_stats() -> SendBlockStats {
    SendBlockStats {
        total: SEND_BLOCK_TOTAL.load(Ordering::Relaxed),
        sum_ns: SEND_BLOCK_SUM_NS.load(Ordering::Relaxed),
        max_ns: SEND_BLOCK_MAX_NS.load(Ordering::Relaxed),
        over_1ms: SEND_BLOCKS_OVER_1MS.load(Ordering::Relaxed),
        over_10ms: SEND_BLOCKS_OVER_10MS.load(Ordering::Relaxed),
        over_100ms: SEND_BLOCKS_OVER_100MS.load(Ordering::Relaxed),
        over_1s: SEND_BLOCKS_OVER_1S.load(Ordering::Relaxed),
    }
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
