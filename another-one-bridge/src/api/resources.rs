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
use another_one_core::resource_usage::{
    ResourceUsageProject, ResourceUsageSession, ResourceUsageSnapshot, ResourceUsageTask,
};

use crate::local_registry::local_registry;

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

/// FRB-exposed projection of `core::resource_usage::ResourceUsageSnapshot`.
/// Identical numeric fields plus the nested project → task → session
/// tree the popover renders. The `app` row is always populated; the
/// `projects` vec is empty when no PTY children are tracked.
pub struct ResourceUsageSnapshotDto {
    pub total_cpu_percent: f64,
    pub total_memory_bytes: u64,
    pub ram_share_percent: f64,
    pub session_count: u64,
    pub app_label: String,
    pub app_cpu_percent: f64,
    pub app_memory_bytes: u64,
    pub projects: Vec<ResourceUsageProjectDto>,
}

pub struct ResourceUsageProjectDto {
    pub key: String,
    pub label: String,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub tasks: Vec<ResourceUsageTaskDto>,
}

pub struct ResourceUsageTaskDto {
    pub key: String,
    pub label: String,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub sessions: Vec<ResourceUsageSessionDto>,
}

pub struct ResourceUsageSessionDto {
    pub key: String,
    pub label: String,
    pub icon_path: String,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
}

/// Hierarchical resource snapshot — host UI process + every tracked
/// PTY child grouped by project → task → session, sorted descending
/// by memory then CPU then label (matching the GPUI desktop's
/// `ResourceIndicator`). Returns `None` if the embedded daemon
/// hasn't booted yet — the popover renders the empty-state pill in
/// that case.
pub fn read_resource_usage_snapshot() -> Option<ResourceUsageSnapshotDto> {
    let registry = local_registry()?;
    let app_pid = std::process::id();
    let snapshot = {
        let mut state = registry.lock().ok()?;
        state.sample_resource_usage(app_pid)
    };
    Some(snapshot_to_dto(snapshot))
}

fn snapshot_to_dto(snapshot: ResourceUsageSnapshot) -> ResourceUsageSnapshotDto {
    ResourceUsageSnapshotDto {
        total_cpu_percent: snapshot.total_cpu_percent as f64,
        total_memory_bytes: snapshot.total_memory_bytes,
        ram_share_percent: snapshot.ram_share_percent as f64,
        session_count: snapshot.session_count as u64,
        app_label: snapshot.app.label,
        app_cpu_percent: snapshot.app.cpu_percent as f64,
        app_memory_bytes: snapshot.app.memory_bytes,
        projects: snapshot.projects.into_iter().map(project_to_dto).collect(),
    }
}

fn project_to_dto(project: ResourceUsageProject) -> ResourceUsageProjectDto {
    ResourceUsageProjectDto {
        key: project.key,
        label: project.label,
        cpu_percent: project.cpu_percent as f64,
        memory_bytes: project.memory_bytes,
        tasks: project.tasks.into_iter().map(task_to_dto).collect(),
    }
}

fn task_to_dto(task: ResourceUsageTask) -> ResourceUsageTaskDto {
    ResourceUsageTaskDto {
        key: task.key,
        label: task.label,
        cpu_percent: task.cpu_percent as f64,
        memory_bytes: task.memory_bytes,
        sessions: task.sessions.into_iter().map(session_to_dto).collect(),
    }
}

fn session_to_dto(session: ResourceUsageSession) -> ResourceUsageSessionDto {
    ResourceUsageSessionDto {
        key: session.key,
        label: session.label,
        icon_path: session.icon_path,
        cpu_percent: session.cpu_percent as f64,
        memory_bytes: session.memory_bytes,
    }
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
