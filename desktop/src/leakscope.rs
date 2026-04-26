//! Desktop-side memory-leak instrumentation.
//!
//! [`start_sampler`] spawns a 1 Hz OS thread that logs a `LEAKSCOPE`
//! line to stderr containing:
//!
//! * `rss` — current VmRSS read from `/proc/self/status` (Linux only;
//!   `?` on other platforms). This is the number we care about — it's
//!   what the OOM killer sees.
//! * `pty_bytes` / `pty_chunks` — totals from `core::leakscope`,
//!   incremented by PTY reader threads on every successful `read()`.
//! * `drained_bytes` / `drained_chunks` / `drains` — what the GPUI
//!   render thread has pulled out of the launch channels.
//! * `in_flight` — derived: `pty_bytes − drained_bytes`. This is the
//!   signal that pins the leak on channel-depth vs. downstream
//!   retention: if RSS grows but `in_flight` stays flat, the bytes
//!   have been drained and are leaking somewhere else.
//! * `live_tabs` / `snapshots` — pushed from the drain so we can tell
//!   whether tabs (or their snapshots) are accumulating.
//!
//! The sampler runs independently of the GPUI render loop, so we keep
//! getting data even if the UI thread stalls (which is exactly when
//! the 37 GiB incident was most informative).
//!
//! Auto-enabled in `debug_assertions` builds (any `cargo run`) and in
//! release builds compiled with `--features leakscope`. Release
//! builds without the feature compile the counter call sites (one
//! relaxed atomic each, effectively free) but skip the sampler
//! thread.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static DRAINED_OUTPUT_BYTES: AtomicU64 = AtomicU64::new(0);
static DRAINED_OUTPUT_CHUNKS: AtomicU64 = AtomicU64::new(0);
static DRAIN_CALLS: AtomicU64 = AtomicU64::new(0);
static LIVE_TABS: AtomicUsize = AtomicUsize::new(0);
static LIVE_SNAPSHOTS: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn note_drain_output(nbytes: usize) {
    DRAINED_OUTPUT_BYTES.fetch_add(nbytes as u64, Ordering::Relaxed);
    DRAINED_OUTPUT_CHUNKS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn note_drain_call() {
    DRAIN_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn set_live_counts(tabs: usize, snapshots: usize) {
    LIVE_TABS.store(tabs, Ordering::Relaxed);
    LIVE_SNAPSHOTS.store(snapshots, Ordering::Relaxed);
}

/// Spawn the 1 Hz sampler. No-op when neither `debug_assertions` nor
/// the `leakscope` feature is active, and idempotent on repeat calls.
pub fn start_sampler() {
    if !another_one_core::leakscope::sampler_auto_enabled() {
        return;
    }
    use std::sync::atomic::AtomicBool;
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::Builder::new()
        .name("leakscope-sampler".into())
        .spawn(run_sampler_loop)
        .expect("spawn leakscope sampler");
    eprintln!("LEAKSCOPE sampler started");
}

fn run_sampler_loop() {
    let started = Instant::now();
    loop {
        thread::sleep(Duration::from_secs(1));
        let t = started.elapsed().as_secs();
        let rss = read_rss_bytes();
        let pty_bytes = another_one_core::leakscope::bytes_read_total();
        let pty_chunks = another_one_core::leakscope::chunks_read_total();
        let drained_bytes = DRAINED_OUTPUT_BYTES.load(Ordering::Relaxed);
        let drained_chunks = DRAINED_OUTPUT_CHUNKS.load(Ordering::Relaxed);
        let drains = DRAIN_CALLS.load(Ordering::Relaxed);
        let in_flight = pty_bytes.saturating_sub(drained_bytes);
        let tabs = LIVE_TABS.load(Ordering::Relaxed);
        let snaps = LIVE_SNAPSHOTS.load(Ordering::Relaxed);
        eprintln!(
            "LEAKSCOPE t={t}s rss={rss} pty_bytes={pty} pty_chunks={pc} \
             drained_bytes={db} drained_chunks={dc} drains={dr} \
             in_flight={inf} live_tabs={tabs} snapshots={snaps}",
            rss = rss.map(format_bytes).unwrap_or_else(|| "?".to_string()),
            pty = format_bytes(pty_bytes),
            pc = pty_chunks,
            db = format_bytes(drained_bytes),
            dc = drained_chunks,
            dr = drains,
            inf = format_bytes(in_flight),
        );
    }
}

fn read_rss_bytes() -> Option<u64> {
    // Linux: /proc/self/status → VmRSS (kB). On other platforms this
    // returns None and the log prints `?`. macOS can use
    // `libc::task_info` with `TASK_BASIC_INFO` when we need it.
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb = rest.trim().split_whitespace().next()?.parse::<u64>().ok()?;
                return Some(kb * 1024);
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn format_bytes(n: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if n >= GIB {
        format!("{:.2}GiB", n as f64 / GIB as f64)
    } else if n >= MIB {
        format!("{:.2}MiB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{:.1}KiB", n as f64 / KIB as f64)
    } else {
        format!("{n}B")
    }
}
