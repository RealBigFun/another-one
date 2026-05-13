//! Compat shim re-exporting the resource-usage types now that the
//! sampler lives daemon-side in `another_one_core::resource_usage`.
//! The desktop's resource-indicator widget renders directly from the
//! `daemon_proto::DaemonResourceUsageWire` projection (#156); this
//! module just gives existing call sites stable paths to import.

pub(crate) use another_one_core::process::TrackedProcess;
pub(crate) use another_one_core::resource_usage::format_memory;
pub(crate) use daemon_proto::{
    DaemonResourceUsageProjectWire as ResourceUsageProject,
    DaemonResourceUsageSessionWire as ResourceUsageSession,
    DaemonResourceUsageTaskWire as ResourceUsageTask, DaemonResourceUsageWire as ResourceUsageSnapshot,
};
