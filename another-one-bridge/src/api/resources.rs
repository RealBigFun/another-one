//! FRB-exposed resource sampler — feeds the desktop titlebar's
//! CPU% / memory MB indicator.
//!
//! Stateless: each call collects one sample of the host UI's own
//! process via `core::platform::HeadlessPlatform::read_process_samples`.
//! The Dart caller polls on a timer and computes CPU% from
//! cumulative-time deltas (`cpu_time_ns` over `timestamp_ms`); we
//! deliberately don't keep state on the Rust side so multiple
//! pollers (tests, future multi-window UIs) don't collide. The
//! delta math + label formatting are display-layer concerns and
//! live in Dart.

use std::time::{SystemTime, UNIX_EPOCH};

use another_one_core::platform::{CurrentPlatform, HeadlessPlatform};

/// Single point-in-time resource snapshot.
pub struct ResourceSample {
    /// Wall-clock millis since the unix epoch — used to compute the
    /// elapsed denominator in CPU%.
    pub timestamp_ms: u64,
    /// Cumulative CPU time the host UI process has consumed in
    /// nanoseconds. Take the delta between two samples to get CPU%.
    pub cpu_time_ns: u64,
    /// Resident set size at this instant (bytes).
    pub memory_bytes: u64,
    /// Total system memory if the platform can report it. Linux/macOS
    /// fill this; Windows currently returns `None` (matches the
    /// GPUI desktop's "hide the total when missing" behaviour).
    pub total_memory_bytes: Option<u64>,
}

/// One-shot sample of the host UI's own CPU + memory. Returns
/// zeros for `cpu_time_ns` / `memory_bytes` if the platform
/// sampler returned nothing for our pid (Windows path today).
pub fn read_app_resource_sample() -> ResourceSample {
    let pid = std::process::id();
    let samples = CurrentPlatform::read_process_samples(pid, &[]);
    let mine = samples.into_iter().find(|s| s.pid == pid);
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let total = CurrentPlatform::total_system_memory_bytes();
    match mine {
        Some(s) => ResourceSample {
            timestamp_ms,
            cpu_time_ns: s.total_cpu_time_ns,
            memory_bytes: s.memory_bytes,
            total_memory_bytes: total,
        },
        None => ResourceSample {
            timestamp_ms,
            cpu_time_ns: 0,
            memory_bytes: 0,
            total_memory_bytes: total,
        },
    }
}
