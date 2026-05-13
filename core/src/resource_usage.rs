//! Daemon-side resource-usage sampler. Walks `/proc/<pid>/stat` (Linux)
//! / Mach task_info (macOS) for the daemon-host process tree and
//! produces a `daemon_proto::DaemonResourceUsageWire` snapshot that
//! rides every `UiSnapshot` projection. Replaces the earlier
//! `app/src/resource_usage.rs` client-side sampler — clients now
//! render purely from the projection (#156).
//!
//! The sampler holds CPU-time deltas across calls (CPU% needs two
//! samples to be meaningful), so callers should reuse the same
//! `ResourceUsageSampler` instance from one tick to the next.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use daemon_proto::{
    DaemonResourceUsageProjectWire, DaemonResourceUsageRowWire, DaemonResourceUsageSessionWire,
    DaemonResourceUsageTaskWire, DaemonResourceUsageWire,
};

use crate::platform::{CurrentPlatform, HeadlessPlatform};
use crate::process::{RawProcessSample, TrackedProcess};

/// Older CPU samples than this are dropped — extending the integration
/// window past ~15s lets a single rare big-tick event skew %CPU for
/// minutes. The sampler returns `0%` for the affected pid until the
/// next sample lands.
const MAX_CPU_SAMPLE_WINDOW: Duration = Duration::from_secs(15);

#[derive(Debug, Default)]
pub struct ResourceUsageSampler {
    previous_cpu_samples: HashMap<u32, CpuUsageSample>,
}

#[derive(Clone, Copy, Debug)]
struct CpuUsageSample {
    total_cpu_time_ns: u64,
    sampled_at: Instant,
}

#[derive(Clone, Debug)]
struct ProcessSample {
    pid: u32,
    ppid: u32,
    cpu_percent: f32,
    memory_bytes: u64,
}

impl ResourceUsageSampler {
    /// Sample now, using `CurrentPlatform::read_process_samples` for
    /// raw CPU/RSS reads. `app_pid` is the daemon-host process; its
    /// row is reported as the "app shell" entry on the wire.
    pub fn sample(
        &mut self,
        app_pid: u32,
        tracked_processes: &[TrackedProcess],
    ) -> DaemonResourceUsageWire {
        self.sample_at(
            Instant::now(),
            app_pid,
            tracked_processes,
            CurrentPlatform::read_process_samples(app_pid, tracked_processes),
        )
    }

    fn sample_at(
        &mut self,
        now: Instant,
        app_pid: u32,
        tracked_processes: &[TrackedProcess],
        processes: Vec<RawProcessSample>,
    ) -> DaemonResourceUsageWire {
        let mut current_cpu_samples = HashMap::with_capacity(processes.len());
        let mut resolved_processes = Vec::with_capacity(processes.len());

        for process in processes {
            let cpu_percent = self
                .previous_cpu_samples
                .get(&process.pid)
                .and_then(|previous| cpu_percent_between(previous, process.total_cpu_time_ns, now))
                .unwrap_or(0.0);
            current_cpu_samples.insert(
                process.pid,
                CpuUsageSample {
                    total_cpu_time_ns: process.total_cpu_time_ns,
                    sampled_at: now,
                },
            );
            resolved_processes.push(ProcessSample {
                pid: process.pid,
                ppid: process.ppid,
                cpu_percent,
                memory_bytes: process.memory_bytes,
            });
        }

        self.previous_cpu_samples = current_cpu_samples;
        build_resource_usage_snapshot(app_pid, tracked_processes, resolved_processes)
    }
}

fn cpu_percent_between(
    previous: &CpuUsageSample,
    total_cpu_time_ns: u64,
    now: Instant,
) -> Option<f32> {
    let elapsed = now.checked_duration_since(previous.sampled_at)?;
    if elapsed.is_zero() {
        return Some(0.0);
    }
    if elapsed > MAX_CPU_SAMPLE_WINDOW {
        return None;
    }

    let cpu_delta_ns = total_cpu_time_ns.saturating_sub(previous.total_cpu_time_ns) as f64;
    let elapsed_ns = elapsed.as_secs_f64() * 1_000_000_000.0;
    Some((cpu_delta_ns / elapsed_ns * 100.0) as f32)
}

fn build_resource_usage_snapshot(
    app_pid: u32,
    tracked_processes: &[TrackedProcess],
    processes: Vec<ProcessSample>,
) -> DaemonResourceUsageWire {
    let mut process_by_pid = HashMap::new();
    let mut children_by_pid: HashMap<u32, Vec<u32>> = HashMap::new();

    for process in processes {
        children_by_pid
            .entry(process.ppid)
            .or_default()
            .push(process.pid);
        process_by_pid.insert(process.pid, process);
    }

    let mut project_builders = HashMap::<String, ProjectBuilder>::new();
    let mut session_count = 0;

    for tracked in tracked_processes {
        let tree = collect_process_tree(tracked.pid, &children_by_pid, &process_by_pid);
        if tree.is_empty() {
            continue;
        }

        let (cpu_percent, memory_bytes) = aggregate_usage(&tree, &process_by_pid);
        session_count += 1;

        let project = project_builders
            .entry(tracked.project_key.clone())
            .or_insert_with(|| ProjectBuilder {
                key: tracked.project_key.clone(),
                label: tracked.project_label.clone(),
                cpu_percent: 0.0,
                memory_bytes: 0,
                tasks: HashMap::new(),
            });
        project.cpu_percent += cpu_percent;
        project.memory_bytes += memory_bytes;

        let task = project
            .tasks
            .entry(tracked.task_key.clone())
            .or_insert_with(|| TaskBuilder {
                key: tracked.task_key.clone(),
                label: tracked.task_label.clone(),
                cpu_percent: 0.0,
                memory_bytes: 0,
                sessions: Vec::new(),
            });
        task.cpu_percent += cpu_percent;
        task.memory_bytes += memory_bytes;
        task.sessions.push(DaemonResourceUsageSessionWire {
            key: tracked.key.clone(),
            label: tracked.label.clone(),
            icon_path: tracked.icon_path.to_string(),
            cpu_percent,
            memory_bytes,
        });
    }

    let mut projects = project_builders
        .into_values()
        .map(ProjectBuilder::into_project)
        .collect::<Vec<_>>();
    sort_projects(&mut projects);

    let (app_cpu_percent, app_memory_bytes) = usage_for_process(app_pid, &process_by_pid);
    let total_cpu_percent =
        app_cpu_percent + projects.iter().map(|row| row.cpu_percent).sum::<f32>();
    let total_memory_bytes =
        app_memory_bytes + projects.iter().map(|row| row.memory_bytes).sum::<u64>();
    let ram_share_percent = CurrentPlatform::total_system_memory_bytes()
        .map(|total_system_memory| {
            if total_system_memory == 0 {
                0.0
            } else {
                (total_memory_bytes as f64 / total_system_memory as f64 * 100.0) as f32
            }
        })
        .unwrap_or(0.0);

    DaemonResourceUsageWire {
        total_cpu_percent,
        total_memory_bytes,
        ram_share_percent,
        session_count,
        app: DaemonResourceUsageRowWire {
            // Display label is rendered client-side; the wire just
            // carries it so older / leaner clients don't have to
            // know about the daemon-host's identity.
            label: "AnotherOne App".to_string(),
            detail: Some("app shell".to_string()),
            cpu_percent: app_cpu_percent,
            memory_bytes: app_memory_bytes,
        },
        projects,
    }
}

/// Render a byte count in the indicator's "1.5 MB" / "3.0 GB" /
/// "240 KB" form. Lives here so both the client renderer and any
/// future daemon-side logging share one formatter.
pub fn format_memory(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.0} KB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn aggregate_usage(
    pids: &HashSet<u32>,
    process_by_pid: &HashMap<u32, ProcessSample>,
) -> (f32, u64) {
    let mut cpu_percent = 0.0;
    let mut memory_bytes = 0;

    for pid in pids {
        if let Some(process) = process_by_pid.get(pid) {
            cpu_percent += process.cpu_percent;
            memory_bytes += process.memory_bytes;
        }
    }

    (cpu_percent, memory_bytes)
}

fn usage_for_process(pid: u32, process_by_pid: &HashMap<u32, ProcessSample>) -> (f32, u64) {
    process_by_pid
        .get(&pid)
        .map(|process| (process.cpu_percent, process.memory_bytes))
        .unwrap_or((0.0, 0))
}

fn collect_process_tree(
    root_pid: u32,
    children_by_pid: &HashMap<u32, Vec<u32>>,
    process_by_pid: &HashMap<u32, ProcessSample>,
) -> HashSet<u32> {
    if !process_by_pid.contains_key(&root_pid) {
        return HashSet::new();
    }

    let mut visited = HashSet::new();
    let mut stack = vec![root_pid];

    while let Some(pid) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }
        if let Some(children) = children_by_pid.get(&pid) {
            stack.extend(children.iter().copied());
        }
    }

    visited
}

#[derive(Clone, Debug)]
struct ProjectBuilder {
    key: String,
    label: String,
    cpu_percent: f32,
    memory_bytes: u64,
    tasks: HashMap<String, TaskBuilder>,
}

#[derive(Clone, Debug)]
struct TaskBuilder {
    key: String,
    label: String,
    cpu_percent: f32,
    memory_bytes: u64,
    sessions: Vec<DaemonResourceUsageSessionWire>,
}

impl ProjectBuilder {
    fn into_project(self) -> DaemonResourceUsageProjectWire {
        let mut tasks = self
            .tasks
            .into_values()
            .map(TaskBuilder::into_task)
            .collect::<Vec<_>>();
        sort_tasks(&mut tasks);

        DaemonResourceUsageProjectWire {
            key: self.key,
            label: self.label,
            cpu_percent: self.cpu_percent,
            memory_bytes: self.memory_bytes,
            tasks,
        }
    }
}

impl TaskBuilder {
    fn into_task(mut self) -> DaemonResourceUsageTaskWire {
        sort_sessions(&mut self.sessions);

        DaemonResourceUsageTaskWire {
            key: self.key,
            label: self.label,
            cpu_percent: self.cpu_percent,
            memory_bytes: self.memory_bytes,
            sessions: self.sessions,
        }
    }
}

fn sort_projects(projects: &mut [DaemonResourceUsageProjectWire]) {
    projects.sort_by(|left, right| {
        compare_usage_rows(
            right.memory_bytes,
            left.memory_bytes,
            right.cpu_percent,
            left.cpu_percent,
            &left.label,
            &right.label,
        )
    });
}

fn sort_tasks(tasks: &mut [DaemonResourceUsageTaskWire]) {
    tasks.sort_by(|left, right| {
        compare_usage_rows(
            right.memory_bytes,
            left.memory_bytes,
            right.cpu_percent,
            left.cpu_percent,
            &left.label,
            &right.label,
        )
    });
}

fn sort_sessions(sessions: &mut [DaemonResourceUsageSessionWire]) {
    sessions.sort_by(|left, right| {
        compare_usage_rows(
            right.memory_bytes,
            left.memory_bytes,
            right.cpu_percent,
            left.cpu_percent,
            &left.label,
            &right.label,
        )
    });
}

fn compare_usage_rows(
    left_memory_bytes: u64,
    right_memory_bytes: u64,
    left_cpu_percent: f32,
    right_cpu_percent: f32,
    left_label: &str,
    right_label: &str,
) -> std::cmp::Ordering {
    left_memory_bytes
        .cmp(&right_memory_bytes)
        .then_with(|| left_cpu_percent.total_cmp(&right_cpu_percent))
        .then_with(|| right_label.cmp(left_label))
}

#[cfg(test)]
mod tests {
    use super::{
        collect_process_tree, cpu_percent_between, format_memory, CpuUsageSample, ProcessSample,
        ResourceUsageSampler, MAX_CPU_SAMPLE_WINDOW,
    };
    use crate::process::{RawProcessSample, TrackedProcess};
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    #[test]
    fn collects_process_descendants() {
        let mut process_by_pid = HashMap::new();
        process_by_pid.insert(
            10,
            ProcessSample {
                pid: 10,
                ppid: 1,
                cpu_percent: 0.0,
                memory_bytes: 0,
            },
        );
        process_by_pid.insert(
            11,
            ProcessSample {
                pid: 11,
                ppid: 10,
                cpu_percent: 0.0,
                memory_bytes: 0,
            },
        );
        process_by_pid.insert(
            12,
            ProcessSample {
                pid: 12,
                ppid: 11,
                cpu_percent: 0.0,
                memory_bytes: 0,
            },
        );

        let children_by_pid = HashMap::from([(10, vec![11]), (11, vec![12])]);
        let tree = collect_process_tree(10, &children_by_pid, &process_by_pid);

        assert_eq!(tree.len(), 3);
        assert!(tree.contains(&10));
        assert!(tree.contains(&11));
        assert!(tree.contains(&12));
    }

    #[test]
    fn formats_memory_in_human_units() {
        assert_eq!(format_memory(1_572_864), "1.5 MB");
        assert_eq!(format_memory(3_221_225_472), "3.0 GB");
    }

    #[test]
    fn samples_shell_metrics_from_the_app_process_only() {
        let mut sampler = ResourceUsageSampler::default();
        let tracked = [TrackedProcess {
            pid: 12,
            key: "resource-session:1".to_string(),
            label: "Codex".to_string(),
            project_key: "resource-project:test".to_string(),
            project_label: "Test Project".to_string(),
            task_key: "resource-task:test".to_string(),
            task_label: "main".to_string(),
            icon_path: "assets/icons/icons__codex-ai.svg",
        }];
        let start = Instant::now();

        let _ = sampler.sample_at(
            start,
            10,
            &tracked,
            vec![
                RawProcessSample {
                    pid: 10,
                    ppid: 1,
                    total_cpu_time_ns: 1_000_000_000,
                    memory_bytes: 100,
                },
                RawProcessSample {
                    pid: 11,
                    ppid: 10,
                    total_cpu_time_ns: 2_000_000_000,
                    memory_bytes: 200,
                },
                RawProcessSample {
                    pid: 12,
                    ppid: 10,
                    total_cpu_time_ns: 3_000_000_000,
                    memory_bytes: 300,
                },
                RawProcessSample {
                    pid: 13,
                    ppid: 12,
                    total_cpu_time_ns: 5_000_000_000,
                    memory_bytes: 400,
                },
            ],
        );

        let snapshot = sampler.sample_at(
            start + Duration::from_secs(2),
            10,
            &tracked,
            vec![
                RawProcessSample {
                    pid: 10,
                    ppid: 1,
                    total_cpu_time_ns: 1_600_000_000,
                    memory_bytes: 100,
                },
                RawProcessSample {
                    pid: 11,
                    ppid: 10,
                    total_cpu_time_ns: 3_000_000_000,
                    memory_bytes: 200,
                },
                RawProcessSample {
                    pid: 12,
                    ppid: 10,
                    total_cpu_time_ns: 4_200_000_000,
                    memory_bytes: 300,
                },
                RawProcessSample {
                    pid: 13,
                    ppid: 12,
                    total_cpu_time_ns: 6_600_000_000,
                    memory_bytes: 400,
                },
            ],
        );

        assert_eq!(snapshot.app.memory_bytes, 100);
        assert!((snapshot.app.cpu_percent - 30.0).abs() < 0.01);
        assert!((snapshot.total_cpu_percent - 170.0).abs() < 0.01);
        assert_eq!(snapshot.total_memory_bytes, 800);
        assert_eq!(snapshot.session_count, 1);
        assert_eq!(snapshot.projects.len(), 1);
        assert!((snapshot.projects[0].cpu_percent - 140.0).abs() < 0.01);
        assert_eq!(snapshot.projects[0].memory_bytes, 700);
    }

    #[test]
    fn cpu_percent_ignores_stale_samples() {
        let previous = CpuUsageSample {
            total_cpu_time_ns: 1_000_000_000,
            sampled_at: Instant::now(),
        };

        let cpu = cpu_percent_between(
            &previous,
            2_000_000_000,
            previous.sampled_at + MAX_CPU_SAMPLE_WINDOW + Duration::from_secs(1),
        );

        assert_eq!(cpu, None);
    }

    #[test]
    fn cpu_percent_allows_closed_interval_with_jitter() {
        let previous = CpuUsageSample {
            total_cpu_time_ns: 1_000_000_000,
            sampled_at: Instant::now(),
        };

        let cpu = cpu_percent_between(
            &previous,
            2_000_000_000,
            previous.sampled_at + Duration::from_millis(5_200),
        );

        assert!(cpu.is_some());
    }
}
