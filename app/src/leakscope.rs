//! Desktop-side memory-leak + lockup instrumentation.
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
//! * `drain_max_ms` / `drain_p_over_frame` — wall-time of the GPUI
//!   drain tick. If these grow while `in_flight` shrinks, the UI is
//!   eating output as fast as it arrives but spending the frame
//!   budget doing it (issue #125 hypothesis A).
//! * `send_block_*` — how long reader threads spent parked in
//!   `SyncSender::send` waiting for the GPUI drain to free a slot.
//!   High counts with short drain times point at a real deadlock
//!   (issue #125 hypothesis B) rather than starvation.
//!
//! [`start_watchdog`] spawns a second OS thread that monitors the
//! GPUI drain heartbeat. If the main thread hasn't drained for
//! `ANOTHER_ONE_WATCHDOG_MS` (default 2000 ms), the watchdog shells
//! out to the platform sampler (`sample` on macOS; `eu-stack` or
//! `gdb -batch` on Linux) to capture all-thread backtraces into
//! `~/.cache/another-one/lockup-<ts>.txt` and prints the path to
//! stderr. That file is the one artifact we need to tell the two
//! #125 hypotheses apart — it nails down which thread the main
//! loop is parked on, without any live-debugging gymnastics.
//!
//! The sampler + watchdog run independently of the GPUI render loop,
//! so we keep getting data even if the UI thread stalls (which is
//! exactly when the 37 GiB incident and the #125 CDP lockup were
//! most informative).
//!
//! Auto-enabled in `debug_assertions` builds (any `cargo run`) and in
//! release builds compiled with `--features leakscope`. Release
//! builds without the feature compile the counter call sites (one
//! relaxed atomic each, effectively free) but skip the sampler +
//! watchdog threads.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use another_one_core::leakscope::SendBlockStats;

static DRAINED_OUTPUT_BYTES: AtomicU64 = AtomicU64::new(0);
static DRAINED_OUTPUT_CHUNKS: AtomicU64 = AtomicU64::new(0);
static DRAIN_CALLS: AtomicU64 = AtomicU64::new(0);
static LIVE_TABS: AtomicUsize = AtomicUsize::new(0);
static LIVE_SNAPSHOTS: AtomicUsize = AtomicUsize::new(0);

// --- Drain-tick wall-time instrumentation ---
//
// Each drain ends by calling `record_drain_tick_ns`. That bumps a
// histogram of how long the GPUI main thread spent inside the drain
// (both the hot and warm paths — we pool them because the watchdog
// only cares about "is the main thread responsive" and the sampler
// already prints `drains` separately for overall tick count).
//
// 16 ms is the one-frame threshold at 60 Hz. 100 ms is "user-visible
// stutter". 1 s is "the app looks frozen". Histogram buckets are
// chosen to match the story we tell in the sampler output.
static DRAIN_MAX_NS: AtomicU64 = AtomicU64::new(0);
static DRAIN_SUM_NS: AtomicU64 = AtomicU64::new(0);
static DRAINS_OVER_16MS: AtomicU64 = AtomicU64::new(0);
static DRAINS_OVER_100MS: AtomicU64 = AtomicU64::new(0);
static DRAINS_OVER_1S: AtomicU64 = AtomicU64::new(0);

/// Unix-millis stamp of the last drain tick. Read by the watchdog
/// thread to detect GPUI-thread stalls. 0 means "no drain has run
/// yet" and the watchdog treats that as disarmed.
static LAST_DRAIN_UNIX_MS: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn note_drain_output(nbytes: usize) {
    DRAINED_OUTPUT_BYTES.fetch_add(nbytes as u64, Ordering::Relaxed);
    DRAINED_OUTPUT_CHUNKS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn set_live_counts(tabs: usize, snapshots: usize) {
    LIVE_TABS.store(tabs, Ordering::Relaxed);
    LIVE_SNAPSHOTS.store(snapshots, Ordering::Relaxed);
}

// `DrainTickGuard` and `drain_tick_guard` were removed as part of
// Phase 5e (design 01 / #158): the GPUI drain no longer parses VT,
// so per-tick durations stay sub-millisecond and the watchdog has
// nothing meaningful to report. The drain-time histogram fields
// (DRAIN_*_NS, DRAINS_OVER_*) are kept compiled for the sampler
// dump but never bumped; deletion of the unused atomics happens in
// a follow-up tidy pass.

/// Spawn the 1 Hz sampler and the watchdog. No-op when neither
/// `debug_assertions` nor the `leakscope` feature is active, and
/// idempotent on repeat calls.
pub fn start_sampler() {
    if !another_one_core::leakscope::sampler_auto_enabled() {
        return;
    }
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::Builder::new()
        .name("leakscope-sampler".into())
        .spawn(run_sampler_loop)
        .expect("spawn leakscope sampler");
    thread::Builder::new()
        .name("leakscope-watchdog".into())
        .spawn(run_watchdog_loop)
        .expect("spawn leakscope watchdog");
    eprintln!("LEAKSCOPE sampler + watchdog started");
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
        let drain_max_ms = DRAIN_MAX_NS.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        let drain_sum_ms = DRAIN_SUM_NS.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        let drain_over_16 = DRAINS_OVER_16MS.load(Ordering::Relaxed);
        let drain_over_100 = DRAINS_OVER_100MS.load(Ordering::Relaxed);
        let drain_over_1s = DRAINS_OVER_1S.load(Ordering::Relaxed);
        let sb = another_one_core::leakscope::send_block_stats();
        eprintln!(
            "LEAKSCOPE t={t}s rss={rss} pty_bytes={pty} pty_chunks={pc} \
             drained_bytes={db} drained_chunks={dc} drains={dr} \
             in_flight={inf} live_tabs={tabs} snapshots={snaps} \
             drain_max_ms={dmx:.1} drain_sum_ms={dsm:.1} \
             drain_over_16ms={do16} drain_over_100ms={do100} drain_over_1s={do1s} \
             {send_block}",
            rss = rss.map(format_bytes).unwrap_or_else(|| "?".to_string()),
            pty = format_bytes(pty_bytes),
            pc = pty_chunks,
            db = format_bytes(drained_bytes),
            dc = drained_chunks,
            dr = drains,
            inf = format_bytes(in_flight),
            dmx = drain_max_ms,
            dsm = drain_sum_ms,
            do16 = drain_over_16,
            do100 = drain_over_100,
            do1s = drain_over_1s,
            send_block = format_send_block(&sb),
        );
    }
}

fn format_send_block(sb: &SendBlockStats) -> String {
    let max_ms = sb.max_ns as f64 / 1_000_000.0;
    let sum_ms = sb.sum_ns as f64 / 1_000_000.0;
    format!(
        "send_block_total={tot} send_block_max_ms={max:.1} send_block_sum_ms={sum:.1} \
         send_block_over_1ms={o1} send_block_over_10ms={o10} \
         send_block_over_100ms={o100} send_block_over_1s={o1s}",
        tot = sb.total,
        max = max_ms,
        sum = sum_ms,
        o1 = sb.over_1ms,
        o10 = sb.over_10ms,
        o100 = sb.over_100ms,
        o1s = sb.over_1s,
    )
}

// ---------------------------------------------------------------
// Watchdog
// ---------------------------------------------------------------

/// Default stall threshold. A responsive app drains on every GPUI
/// tick (~16 ms at 60 Hz, less on idle ticks). 2 seconds is well
/// past "I noticed the window is frozen"; below that we'd false-fire
/// on any heavy-but-recoverable drain tick.
const DEFAULT_WATCHDOG_MS: u64 = 2000;
/// Cooldown after a capture so a persistent freeze doesn't spam the
/// filesystem with 30 consecutive stack dumps. Tuned for "enough to
/// see the lockup evolve across a couple of captures" without
/// flooding logs.
const WATCHDOG_CAPTURE_COOLDOWN_MS: u64 = 30_000;

fn watchdog_threshold_ms() -> u64 {
    std::env::var("ANOTHER_ONE_WATCHDOG_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&v| v >= 100)
        .unwrap_or(DEFAULT_WATCHDOG_MS)
}

fn run_watchdog_loop() {
    let threshold_ms = watchdog_threshold_ms();
    eprintln!(
        "LEAKSCOPE watchdog: stall threshold = {threshold_ms}ms \
         (override via ANOTHER_ONE_WATCHDOG_MS)"
    );
    let mut last_capture_ms: u64 = 0;
    loop {
        // Poll at a fraction of the threshold so the minimum
        // detection latency is `threshold_ms + ~threshold_ms/4`.
        // 250 ms is a fine floor — short enough to feel snappy,
        // long enough that the watchdog itself isn't a perf concern.
        let poll = (threshold_ms / 4).clamp(250, 1000);
        thread::sleep(Duration::from_millis(poll));

        let last_drain = LAST_DRAIN_UNIX_MS.load(Ordering::Relaxed);
        if last_drain == 0 {
            // Drain loop hasn't run yet. Don't arm the watchdog; a
            // cold app is not a lockup.
            continue;
        }
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let stale_ms = now_ms.saturating_sub(last_drain);
        if stale_ms < threshold_ms {
            continue;
        }
        if now_ms.saturating_sub(last_capture_ms) < WATCHDOG_CAPTURE_COOLDOWN_MS {
            continue;
        }
        last_capture_ms = now_ms;
        capture_lockup(stale_ms);
    }
}

fn capture_lockup(stale_ms: u64) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out_path = match lockup_capture_path(ts) {
        Some(path) => path,
        None => {
            eprintln!(
                "WATCHDOG: GPUI heartbeat stale {stale_ms}ms but no writable cache dir; \
                 skipping capture"
            );
            return;
        }
    };
    eprintln!(
        "WATCHDOG: GPUI heartbeat stale {stale_ms}ms; capturing to {}",
        out_path.display()
    );

    let pid = std::process::id();
    let header = format!(
        "# another-one watchdog capture\n\
         # unix_ts={ts} stale_ms={stale_ms} pid={pid}\n\
         # last_drain_unix_ms={last}\n\
         # drain_max_ms={dmx:.1} drain_sum_ms={dsm:.1} drains={drains}\n\
         # drain_over_16ms={do16} drain_over_100ms={do100} drain_over_1s={do1s}\n\
         # pty_bytes={pty} drained_bytes={db} in_flight={inf}\n\
         # live_tabs={tabs} snapshots={snaps}\n\
         # {send_block}\n\n",
        last = LAST_DRAIN_UNIX_MS.load(Ordering::Relaxed),
        dmx = DRAIN_MAX_NS.load(Ordering::Relaxed) as f64 / 1_000_000.0,
        dsm = DRAIN_SUM_NS.load(Ordering::Relaxed) as f64 / 1_000_000.0,
        drains = DRAIN_CALLS.load(Ordering::Relaxed),
        do16 = DRAINS_OVER_16MS.load(Ordering::Relaxed),
        do100 = DRAINS_OVER_100MS.load(Ordering::Relaxed),
        do1s = DRAINS_OVER_1S.load(Ordering::Relaxed),
        pty = another_one_core::leakscope::bytes_read_total(),
        db = DRAINED_OUTPUT_BYTES.load(Ordering::Relaxed),
        inf = another_one_core::leakscope::bytes_read_total()
            .saturating_sub(DRAINED_OUTPUT_BYTES.load(Ordering::Relaxed)),
        tabs = LIVE_TABS.load(Ordering::Relaxed),
        snaps = LIVE_SNAPSHOTS.load(Ordering::Relaxed),
        send_block = format_send_block(&another_one_core::leakscope::send_block_stats()),
    );

    if let Err(e) = std::fs::write(&out_path, header.as_bytes()) {
        eprintln!("WATCHDOG: failed to write capture header: {e}");
        return;
    }

    // Shell out to the platform sampler. Both branches append their
    // output to the file we just wrote, so the stack dump sits
    // immediately after the header for grep-ability.
    let sampler_result = platform_capture_stacks(pid, &out_path);
    match sampler_result {
        Ok(tool) => {
            eprintln!(
                "WATCHDOG: stack dump via `{tool}` written to {}",
                out_path.display()
            );
        }
        Err(e) => {
            eprintln!(
                "WATCHDOG: no stack sampler succeeded ({e}); header-only capture at {}",
                out_path.display()
            );
        }
    }
}

fn lockup_capture_path(ts: u64) -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("another-one");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("lockup-{ts}.txt")))
}

/// Attempt to capture all-thread backtraces. Returns the name of the
/// tool that succeeded, or an error message listing what we tried.
///
/// Ordering:
/// * macOS: `sample` ships with Xcode CLT and works for same-user
///   processes without a helper.
/// * Linux: `eu-stack` (elfutils) is quick and doesn't need gdb; fall
///   back to `gdb -batch` because ptrace_scope=1 distros still allow
///   same-uid attaches.
fn platform_capture_stacks(pid: u32, out_path: &PathBuf) -> Result<&'static str, String> {
    let mut attempts: Vec<String> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("sample")
            .arg(pid.to_string())
            .arg("3") // seconds of sampling
            .arg("-file")
            .arg(out_path)
            .arg("-mayDie")
            .status();
        match status {
            Ok(s) if s.success() => return Ok("sample"),
            Ok(s) => attempts.push(format!("sample exited with {s}")),
            Err(e) => attempts.push(format!("sample spawn failed: {e}")),
        }
    }

    #[cfg(target_os = "linux")]
    {
        // `eu-stack -p <pid>` prints all-thread stacks without
        // ptrace-stopping the process the way gdb does. Append to
        // the file we already seeded with the header.
        if let Ok(file) = std::fs::OpenOptions::new().append(true).open(out_path) {
            let stdout = file.try_clone().ok();
            let stderr = file;
            let mut cmd = Command::new("eu-stack");
            cmd.arg("-p").arg(pid.to_string());
            if let Some(stdout) = stdout {
                cmd.stdout(stdout);
            }
            cmd.stderr(stderr);
            match cmd.status() {
                Ok(s) if s.success() => return Ok("eu-stack"),
                Ok(s) => attempts.push(format!("eu-stack exited with {s}")),
                Err(e) => attempts.push(format!("eu-stack spawn failed: {e}")),
            }
        } else {
            attempts.push("eu-stack: cannot reopen capture file".into());
        }

        if let Ok(file) = std::fs::OpenOptions::new().append(true).open(out_path) {
            let stdout = file.try_clone().ok();
            let stderr = file;
            let mut cmd = Command::new("gdb");
            cmd.arg("-batch")
                .arg("-nx")
                .arg("-p")
                .arg(pid.to_string())
                .arg("-ex")
                .arg("set pagination off")
                .arg("-ex")
                .arg("thread apply all bt")
                .arg("-ex")
                .arg("detach")
                .arg("-ex")
                .arg("quit");
            if let Some(stdout) = stdout {
                cmd.stdout(stdout);
            }
            cmd.stderr(stderr);
            match cmd.status() {
                Ok(s) if s.success() => return Ok("gdb"),
                Ok(s) => attempts.push(format!("gdb exited with {s}")),
                Err(e) => attempts.push(format!("gdb spawn failed: {e}")),
            }
        } else {
            attempts.push("gdb: cannot reopen capture file".into());
        }
    }

    // Silence "unused" warnings on platforms where neither block
    // compiled; on mac/linux both branches ran and `pid`/`out_path`
    // are used via the Command builders above.
    let _ = (pid, out_path);

    Err(if attempts.is_empty() {
        "no platform sampler available".to_string()
    } else {
        attempts.join("; ")
    })
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
                let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
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
