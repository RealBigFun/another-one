use std::collections::{HashMap, HashSet};
use std::process::Command;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ResourceUsageSnapshot {
    pub total_cpu_percent: f32,
    pub total_memory_bytes: u64,
    pub ram_share_percent: f32,
    pub session_count: usize,
    pub app: ResourceUsageRow,
    pub projects: Vec<ResourceUsageProject>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ResourceUsageRow {
    pub label: String,
    pub detail: Option<String>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ResourceUsageProject {
    pub key: String,
    pub label: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub tasks: Vec<ResourceUsageTask>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ResourceUsageTask {
    pub key: String,
    pub label: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub sessions: Vec<ResourceUsageSession>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ResourceUsageSession {
    pub key: String,
    pub label: String,
    pub icon_path: &'static str,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrackedProcess {
    pub pid: u32,
    pub key: String,
    pub label: String,
    pub project_key: String,
    pub project_label: String,
    pub task_key: String,
    pub task_label: String,
    pub icon_path: &'static str,
}

#[derive(Clone, Debug)]
struct ProcessSample {
    pid: u32,
    ppid: u32,
    cpu_percent: f32,
    memory_bytes: u64,
}

pub(crate) fn sample_resource_usage(
    app_pid: u32,
    tracked_processes: &[TrackedProcess],
) -> ResourceUsageSnapshot {
    let processes = read_process_samples();
    let mut process_by_pid = HashMap::new();
    let mut children_by_pid: HashMap<u32, Vec<u32>> = HashMap::new();

    for process in processes {
        children_by_pid
            .entry(process.ppid)
            .or_default()
            .push(process.pid);
        process_by_pid.insert(process.pid, process);
    }

    let app_tree = collect_process_tree(app_pid, &children_by_pid, &process_by_pid);
    let mut project_builders = HashMap::<String, ProjectBuilder>::new();
    let mut session_pids = HashSet::new();
    let mut session_count = 0;

    for tracked in tracked_processes {
        let tree = collect_process_tree(tracked.pid, &children_by_pid, &process_by_pid);
        if tree.is_empty() {
            continue;
        }

        session_pids.extend(tree.iter().copied());
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
        task.sessions.push(ResourceUsageSession {
            key: tracked.key.clone(),
            label: tracked.label.clone(),
            icon_path: tracked.icon_path,
            cpu_percent,
            memory_bytes,
        });
    }

    let mut projects = project_builders
        .into_values()
        .map(ProjectBuilder::into_project)
        .collect::<Vec<_>>();
    sort_projects(&mut projects);

    let app_only_pids = app_tree
        .into_iter()
        .filter(|pid| !session_pids.contains(pid))
        .collect::<HashSet<_>>();
    let (app_cpu_percent, app_memory_bytes) = aggregate_usage(&app_only_pids, &process_by_pid);

    let total_cpu_percent =
        app_cpu_percent + projects.iter().map(|row| row.cpu_percent).sum::<f32>();
    let total_memory_bytes =
        app_memory_bytes + projects.iter().map(|row| row.memory_bytes).sum::<u64>();
    let ram_share_percent = total_system_memory_bytes()
        .map(|total_system_memory| {
            if total_system_memory == 0 {
                0.0
            } else {
                (total_memory_bytes as f64 / total_system_memory as f64 * 100.0) as f32
            }
        })
        .unwrap_or(0.0);

    ResourceUsageSnapshot {
        total_cpu_percent,
        total_memory_bytes,
        ram_share_percent,
        session_count,
        app: ResourceUsageRow {
            label: "AnotherOne App".to_string(),
            detail: Some("internal threads".to_string()),
            cpu_percent: app_cpu_percent,
            memory_bytes: app_memory_bytes,
        },
        projects,
    }
}

pub(crate) fn format_memory(bytes: u64) -> String {
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
    sessions: Vec<ResourceUsageSession>,
}

impl ProjectBuilder {
    fn into_project(self) -> ResourceUsageProject {
        let mut tasks = self
            .tasks
            .into_values()
            .map(TaskBuilder::into_task)
            .collect::<Vec<_>>();
        sort_tasks(&mut tasks);

        ResourceUsageProject {
            key: self.key,
            label: self.label,
            cpu_percent: self.cpu_percent,
            memory_bytes: self.memory_bytes,
            tasks,
        }
    }
}

impl TaskBuilder {
    fn into_task(mut self) -> ResourceUsageTask {
        sort_sessions(&mut self.sessions);

        ResourceUsageTask {
            key: self.key,
            label: self.label,
            cpu_percent: self.cpu_percent,
            memory_bytes: self.memory_bytes,
            sessions: self.sessions,
        }
    }
}

fn sort_projects(projects: &mut [ResourceUsageProject]) {
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

fn sort_tasks(tasks: &mut [ResourceUsageTask]) {
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

fn sort_sessions(sessions: &mut [ResourceUsageSession]) {
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

fn read_process_samples() -> Vec<ProcessSample> {
    let output = Command::new("ps")
        .args(["-ax", "-o", "pid=,ppid=,%cpu=,rss=,comm="])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_process_sample)
        .collect()
}

fn parse_process_sample(line: &str) -> Option<ProcessSample> {
    let mut parts = line.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let ppid = parts.next()?.parse().ok()?;
    let cpu_percent = parts.next()?.parse().ok()?;
    let rss_kib = parts.next()?.parse::<u64>().ok()?;

    Some(ProcessSample {
        pid,
        ppid,
        cpu_percent,
        memory_bytes: rss_kib.saturating_mul(1024),
    })
}

fn total_system_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let bytes = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .ok()?;
        return Some(bytes);
    }

    #[cfg(target_os = "linux")]
    {
        let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
        let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
        let kib = line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u64>().ok())?;
        return Some(kib.saturating_mul(1024));
    }

    #[allow(unreachable_code)]
    None
}

#[cfg(test)]
mod tests {
    use super::{collect_process_tree, format_memory, parse_process_sample, ProcessSample};
    use std::collections::HashMap;

    #[test]
    fn parses_ps_output_line() {
        let sample = parse_process_sample("123 1 4.2 8192 /bin/zsh").unwrap();

        assert_eq!(sample.pid, 123);
        assert_eq!(sample.ppid, 1);
        assert_eq!(sample.cpu_percent, 4.2);
        assert_eq!(sample.memory_bytes, 8_388_608);
    }

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
}
