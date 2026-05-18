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

use tracing::{debug, trace};

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
        let now = Instant::now();
        let raw = CurrentPlatform::read_process_samples(app_pid, tracked_processes);
        let elapsed_ms = now.elapsed().as_millis();
        trace!(elapsed_ms, tracked = tracked_processes.len(), "resource sample I/O");
        if elapsed_ms > 100 {
            tracing::warn!(elapsed_ms, tracked = tracked_processes.len(), "resource sample took >100ms");
        }
        self.sample_at(now, app_pid, tracked_processes, raw)
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
            debug!(pid = tracked.pid, session = %tracked.key, "tracked process absent from sample — likely exited");
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
        cpu_core_count: CurrentPlatform::num_logical_cpus(),
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
    projects.sort_by(|a, b| {
        compare_usage_rows(a.memory_bytes, b.memory_bytes, a.cpu_percent, b.cpu_percent, &a.label, &b.label)
    });
}

fn sort_tasks(tasks: &mut [DaemonResourceUsageTaskWire]) {
    tasks.sort_by(|a, b| {
        compare_usage_rows(a.memory_bytes, b.memory_bytes, a.cpu_percent, b.cpu_percent, &a.label, &b.label)
    });
}

fn sort_sessions(sessions: &mut [DaemonResourceUsageSessionWire]) {
    sessions.sort_by(|a, b| {
        compare_usage_rows(a.memory_bytes, b.memory_bytes, a.cpu_percent, b.cpu_percent, &a.label, &b.label)
    });
}

/// Descending by memory, then descending by CPU, then ascending by label.
fn compare_usage_rows(
    memory_bytes_a: u64,
    memory_bytes_b: u64,
    cpu_percent_a: f32,
    cpu_percent_b: f32,
    label_a: &str,
    label_b: &str,
) -> std::cmp::Ordering {
    memory_bytes_b
        .cmp(&memory_bytes_a)
        .then_with(|| cpu_percent_b.total_cmp(&cpu_percent_a))
        .then_with(|| label_a.cmp(label_b))
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
        // Boundaries
        assert_eq!(format_memory(0), "0 B");
        assert_eq!(format_memory(1), "1 B");
        assert_eq!(format_memory(1023), "1023 B");
        assert_eq!(format_memory(1024), "1 KB");           // exactly 1 KiB
        assert_eq!(format_memory(1023 * 1024), "1023 KB"); // just below 1 MiB
        assert_eq!(format_memory(1024 * 1024), "1.0 MB");  // exactly 1 MiB
        assert_eq!(format_memory(1024 * 1024 * 1024 - 1), "1024.0 MB"); // just below 1 GiB
        assert_eq!(format_memory(1024 * 1024 * 1024), "1.0 GB"); // exactly 1 GiB
        // Mid-tier spot checks (pre-existing)
        assert_eq!(format_memory(1_572_864), "1.5 MB");
        assert_eq!(format_memory(3_221_225_472), "3.0 GB");
        // Must not panic on max value
        let _ = format_memory(u64::MAX);
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
    fn cpu_percent_none_when_now_is_before_sampled_at() {
        // Monotonicity violation or deliberate misuse in tests: must not panic, must return None.
        let future = Instant::now() + Duration::from_secs(10);
        let previous = CpuUsageSample { total_cpu_time_ns: 1_000_000_000, sampled_at: future };
        let cpu = cpu_percent_between(&previous, 2_000_000_000, Instant::now());
        assert_eq!(cpu, None);
    }

    #[test]
    fn cpu_percent_zero_when_cpu_time_regresses() {
        // PID reuse: new process has less accumulated CPU time than the old one.
        // saturating_sub yields 0 ns delta → 0.0% for this tick, not a panic or negative value.
        let previous = CpuUsageSample {
            total_cpu_time_ns: 5_000_000_000,
            sampled_at: Instant::now(),
        };
        let cpu = cpu_percent_between(
            &previous,
            1_000_000_000, // regressed
            previous.sampled_at + Duration::from_secs(1),
        );
        assert_eq!(cpu, Some(0.0));
    }

    #[test]
    fn cpu_percent_zero_for_zero_elapsed() {
        let previous = CpuUsageSample {
            total_cpu_time_ns: 1_000_000_000,
            sampled_at: Instant::now(),
        };
        let cpu = cpu_percent_between(&previous, 2_000_000_000, previous.sampled_at);
        assert_eq!(cpu, Some(0.0));
    }

    #[test]
    fn cpu_percent_some_at_exactly_max_window() {
        let previous = CpuUsageSample {
            total_cpu_time_ns: 1_000_000_000,
            sampled_at: Instant::now(),
        };
        // elapsed == MAX_CPU_SAMPLE_WINDOW: must be accepted (> check, not >=)
        let cpu = cpu_percent_between(
            &previous,
            2_000_000_000,
            previous.sampled_at + MAX_CPU_SAMPLE_WINDOW,
        );
        assert!(cpu.is_some(), "sample at exactly the window boundary should be kept");
    }

    #[test]
    fn cpu_percent_none_one_ns_past_max_window() {
        let previous = CpuUsageSample {
            total_cpu_time_ns: 1_000_000_000,
            sampled_at: Instant::now(),
        };
        let cpu = cpu_percent_between(
            &previous,
            2_000_000_000,
            previous.sampled_at + MAX_CPU_SAMPLE_WINDOW + Duration::from_nanos(1),
        );
        assert_eq!(cpu, None, "sample one ns past the window must be dropped");
    }

    #[test]
    fn sort_orders_by_memory_descending_then_cpu_descending_then_label_ascending() {
        use daemon_proto::DaemonResourceUsageProjectWire;

        let make = |label: &str, mem: u64, cpu: f32| DaemonResourceUsageProjectWire {
            key: label.to_string(),
            label: label.to_string(),
            cpu_percent: cpu,
            memory_bytes: mem,
            tasks: vec![],
        };

        let mut rows = vec![
            make("beta", 100, 50.0),  // same mem as alpha, higher cpu → second
            make("alpha", 100, 80.0), // highest cpu at this mem tier → first
            make("gamma", 200, 10.0), // highest mem → wins overall
            make("delta", 50, 99.0),  // lowest mem → last despite highest cpu
        ];

        super::sort_projects(&mut rows);

        assert_eq!(rows[0].label, "gamma");  // 200 bytes
        assert_eq!(rows[1].label, "alpha");  // 100 bytes, 80% cpu
        assert_eq!(rows[2].label, "beta");   // 100 bytes, 50% cpu
        assert_eq!(rows[3].label, "delta");  // 50 bytes
    }

    #[test]
    fn sort_breaks_ties_by_label_ascending() {
        use daemon_proto::DaemonResourceUsageProjectWire;

        let make = |label: &str| DaemonResourceUsageProjectWire {
            key: label.to_string(),
            label: label.to_string(),
            cpu_percent: 0.0,
            memory_bytes: 100,
            tasks: vec![],
        };

        let mut rows = vec![make("zebra"), make("apple"), make("mango")];
        super::sort_projects(&mut rows);

        assert_eq!(rows[0].label, "apple");
        assert_eq!(rows[1].label, "mango");
        assert_eq!(rows[2].label, "zebra");
    }

    #[test]
    fn aggregates_multi_project_multi_task_correctly() {
        // Two projects, each with two tasks, each with one session.
        // Project totals must equal sum of their tasks; task totals must equal their sessions.
        let mut sampler = ResourceUsageSampler::default();
        let tracked = [
            TrackedProcess { pid: 11, key: "s1".into(), label: "S1".into(), project_key: "p1".into(), project_label: "P1".into(), task_key: "t1".into(), task_label: "T1".into(), icon_path: "" },
            TrackedProcess { pid: 12, key: "s2".into(), label: "S2".into(), project_key: "p1".into(), project_label: "P1".into(), task_key: "t2".into(), task_label: "T2".into(), icon_path: "" },
            TrackedProcess { pid: 13, key: "s3".into(), label: "S3".into(), project_key: "p2".into(), project_label: "P2".into(), task_key: "t3".into(), task_label: "T3".into(), icon_path: "" },
            TrackedProcess { pid: 14, key: "s4".into(), label: "S4".into(), project_key: "p2".into(), project_label: "P2".into(), task_key: "t4".into(), task_label: "T4".into(), icon_path: "" },
        ];
        let start = Instant::now();
        // Warm up previous_cpu_samples
        let _ = sampler.sample_at(start, 10, &tracked, vec![
            RawProcessSample { pid: 10, ppid: 1, total_cpu_time_ns: 0, memory_bytes: 50 },
            RawProcessSample { pid: 11, ppid: 10, total_cpu_time_ns: 0, memory_bytes: 100 },
            RawProcessSample { pid: 12, ppid: 10, total_cpu_time_ns: 0, memory_bytes: 200 },
            RawProcessSample { pid: 13, ppid: 10, total_cpu_time_ns: 0, memory_bytes: 300 },
            RawProcessSample { pid: 14, ppid: 10, total_cpu_time_ns: 0, memory_bytes: 400 },
        ]);
        let snapshot = sampler.sample_at(start + Duration::from_secs(1), 10, &tracked, vec![
            RawProcessSample { pid: 10, ppid: 1,  total_cpu_time_ns: 100_000_000, memory_bytes: 50 },
            RawProcessSample { pid: 11, ppid: 10, total_cpu_time_ns: 200_000_000, memory_bytes: 100 },
            RawProcessSample { pid: 12, ppid: 10, total_cpu_time_ns: 300_000_000, memory_bytes: 200 },
            RawProcessSample { pid: 13, ppid: 10, total_cpu_time_ns: 400_000_000, memory_bytes: 300 },
            RawProcessSample { pid: 14, ppid: 10, total_cpu_time_ns: 500_000_000, memory_bytes: 400 },
        ]);

        assert_eq!(snapshot.session_count, 4);
        assert_eq!(snapshot.projects.len(), 2);

        // Find each project (HashMap ordering is not stable)
        let p1 = snapshot.projects.iter().find(|p| p.key == "p1").expect("p1 missing");
        let p2 = snapshot.projects.iter().find(|p| p.key == "p2").expect("p2 missing");

        // Memory: p1 = 100 + 200 = 300, p2 = 300 + 400 = 700
        assert_eq!(p1.memory_bytes, 300);
        assert_eq!(p2.memory_bytes, 700);

        // Task totals must equal their sessions
        let t1 = p1.tasks.iter().find(|t| t.key == "t1").expect("t1 missing");
        let t2 = p1.tasks.iter().find(|t| t.key == "t2").expect("t2 missing");
        assert_eq!(t1.memory_bytes, 100);
        assert_eq!(t2.memory_bytes, 200);
        assert_eq!(p1.memory_bytes, t1.memory_bytes + t2.memory_bytes);
        assert_eq!(p2.memory_bytes, snapshot.projects.iter().find(|p| p.key == "p2").unwrap()
            .tasks.iter().map(|t| t.memory_bytes).sum::<u64>());
    }

    #[test]
    fn collect_process_tree_terminates_on_cycle() {
        // Synthetic cycle: pid 10 → 11 → 10. Must terminate with a finite result.
        let mut process_by_pid = HashMap::new();
        for pid in [10u32, 11] {
            process_by_pid.insert(pid, ProcessSample { pid, ppid: 1, cpu_percent: 0.0, memory_bytes: 0 });
        }
        let children_by_pid = HashMap::from([
            (10u32, vec![11u32]),
            (11u32, vec![10u32]), // cycle back
        ]);
        let tree = collect_process_tree(10, &children_by_pid, &process_by_pid);
        assert_eq!(tree.len(), 2);
        assert!(tree.contains(&10));
        assert!(tree.contains(&11));
    }
}
