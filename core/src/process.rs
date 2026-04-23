//! Process-tracking data types shared by the terminal manager and the
//! desktop's resource-usage sampler.
//!
//! Pure data only. The sampler itself (reading `/proc` or sysctl into
//! live CPU/memory numbers) lives in `desktop/src/resource_usage.rs`
//! because it dispatches through the desktop's GPUI-coupled
//! `PlatformServices` trait. The types here are the vocabulary both
//! sides use to talk about tracked processes without one reaching into
//! the other.

/// A child process spawned by an agent session, tagged with enough
/// project/task labeling to aggregate its CPU/memory usage back into the
/// UI's resource panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackedProcess {
    pub pid: u32,
    pub key: String,
    pub label: String,
    pub project_key: String,
    pub project_label: String,
    pub task_key: String,
    pub task_label: String,
    pub icon_path: &'static str,
}

/// One snapshot of a process's CPU + memory at a given instant, as read
/// out of the OS by a platform-specific sampler. The sampler produces a
/// stream of these; the aggregator in desktop turns them into
/// `ResourceUsageSnapshot`s.
#[derive(Clone, Copy, Debug)]
pub struct RawProcessSample {
    pub pid: u32,
    pub ppid: u32,
    pub total_cpu_time_ns: u64,
    pub memory_bytes: u64,
}
