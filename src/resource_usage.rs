use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const MAX_CPU_SAMPLE_WINDOW: Duration = Duration::from_secs(15);

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

#[derive(Debug, Default)]
pub(crate) struct ResourceUsageSampler {
    previous_cpu_samples: HashMap<u32, CpuUsageSample>,
}

#[derive(Clone, Copy, Debug)]
struct CpuUsageSample {
    total_cpu_time_ns: u64,
    sampled_at: Instant,
}

#[derive(Clone, Copy, Debug)]
struct RawProcessSample {
    pid: u32,
    ppid: u32,
    total_cpu_time_ns: u64,
    memory_bytes: u64,
}

#[derive(Clone, Debug)]
struct ProcessSample {
    pid: u32,
    ppid: u32,
    cpu_percent: f32,
    memory_bytes: u64,
}

impl ResourceUsageSampler {
    pub(crate) fn sample(
        &mut self,
        app_pid: u32,
        tracked_processes: &[TrackedProcess],
    ) -> ResourceUsageSnapshot {
        self.sample_at(
            Instant::now(),
            app_pid,
            tracked_processes,
            read_process_samples(app_pid, tracked_processes),
        )
    }

    fn sample_at(
        &mut self,
        now: Instant,
        app_pid: u32,
        tracked_processes: &[TrackedProcess],
        processes: Vec<RawProcessSample>,
    ) -> ResourceUsageSnapshot {
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
) -> ResourceUsageSnapshot {
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

    let (app_cpu_percent, app_memory_bytes) = usage_for_process(app_pid, &process_by_pid);
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
            detail: Some("app shell".to_string()),
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

fn read_process_samples(
    app_pid: u32,
    tracked_processes: &[TrackedProcess],
) -> Vec<RawProcessSample> {
    #[cfg(target_os = "macos")]
    {
        return read_process_samples_macos(app_pid, tracked_processes);
    }

    #[cfg(target_os = "linux")]
    {
        let _ = app_pid;
        let _ = tracked_processes;
        return read_process_samples_linux();
    }

    #[allow(unreachable_code)]
    Vec::new()
}

#[cfg(target_os = "macos")]
fn read_process_samples_macos(
    app_pid: u32,
    tracked_processes: &[TrackedProcess],
) -> Vec<RawProcessSample> {
    let mut roots = Vec::with_capacity(1 + tracked_processes.len());
    roots.push(app_pid);
    roots.extend(
        tracked_processes
            .iter()
            .map(|process| process.pid)
            .filter(|pid| *pid != app_pid),
    );

    let mut visited = HashSet::new();
    let mut stack = roots;
    let mut samples = Vec::new();

    while let Some(pid) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }

        if let Some(sample) = read_process_sample_macos(pid) {
            stack.extend(list_child_pids_macos(pid));
            samples.push(sample);
        }
    }

    samples
}

#[cfg(target_os = "macos")]
fn read_process_sample_macos(pid: u32) -> Option<RawProcessSample> {
    let bsdinfo = proc_pid_bsdinfo(pid)?;
    let usage = proc_pid_rusage_info(pid)?;
    Some(RawProcessSample {
        pid,
        ppid: bsdinfo.pbi_ppid,
        total_cpu_time_ns: mach_time_units_to_nanos(
            usage.ri_user_time.saturating_add(usage.ri_system_time),
        ),
        memory_bytes: usage.ri_phys_footprint,
    })
}

#[cfg(target_os = "macos")]
fn list_child_pids_macos(ppid: u32) -> Vec<u32> {
    let mut child_pids = vec![0_i32; 32];

    loop {
        let count = unsafe {
            proc_listchildpids(
                ppid as libc::pid_t,
                child_pids.as_mut_ptr().cast(),
                child_pids.len() as i32,
            )
        };
        if count <= 0 {
            return Vec::new();
        }

        if (count as usize) < child_pids.len() {
            return child_pids
                .into_iter()
                .take(count as usize)
                .filter_map(|pid| u32::try_from(pid).ok())
                .collect();
        }

        child_pids.resize(child_pids.len() * 2, 0);
    }
}

#[cfg(target_os = "macos")]
fn proc_pid_bsdinfo(pid: u32) -> Option<ProcBsdInfo> {
    let mut info = ProcBsdInfo::default();
    let result = unsafe {
        proc_pidinfo(
            pid as i32,
            PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut ProcBsdInfo).cast(),
            std::mem::size_of::<ProcBsdInfo>() as i32,
        )
    };
    (result == std::mem::size_of::<ProcBsdInfo>() as i32).then_some(info)
}

#[cfg(target_os = "macos")]
fn proc_pid_rusage_info(pid: u32) -> Option<RusageInfoV6> {
    let mut info = RusageInfoV6::default();
    let result = unsafe {
        proc_pid_rusage(
            pid as i32,
            RUSAGE_INFO_CURRENT,
            (&mut info as *mut RusageInfoV6).cast(),
        )
    };
    (result == 0).then_some(info)
}

#[cfg(target_os = "macos")]
fn mach_time_units_to_nanos(value: u64) -> u64 {
    let timebase = mach_timebase();
    value.saturating_mul(timebase.numer as u64) / timebase.denom as u64
}

#[cfg(target_os = "macos")]
fn mach_timebase() -> &'static MachTimebaseInfo {
    static TIMEBASE: OnceLock<MachTimebaseInfo> = OnceLock::new();
    TIMEBASE.get_or_init(|| {
        let mut info = MachTimebaseInfo::default();
        let result = unsafe { mach_timebase_info(&mut info) };
        if result != 0 || info.numer == 0 || info.denom == 0 {
            MachTimebaseInfo { numer: 1, denom: 1 }
        } else {
            info
        }
    })
}

#[cfg(target_os = "linux")]
fn read_process_samples_linux() -> Vec<RawProcessSample> {
    let clock_ticks_per_second = match sysconf_u64(libc::_SC_CLK_TCK) {
        Some(value) if value > 0 => value,
        _ => return Vec::new(),
    };
    let page_size = match sysconf_u64(libc::_SC_PAGESIZE) {
        Some(value) if value > 0 => value,
        _ => return Vec::new(),
    };

    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_name = entry.file_name();
            let file_name = file_name.to_str()?;
            let pid = file_name.parse::<u32>().ok()?;
            let stat_path = entry.path().join("stat");
            let stat = std::fs::read_to_string(stat_path).ok()?;
            parse_linux_process_sample(&stat, pid, clock_ticks_per_second, page_size)
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn parse_linux_process_sample(
    stat_line: &str,
    pid: u32,
    clock_ticks_per_second: u64,
    page_size: u64,
) -> Option<RawProcessSample> {
    let comm_end = stat_line.rfind(") ")?;
    let fields = stat_line
        .get(comm_end + 2..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    let ppid = fields.get(1)?.parse::<u32>().ok()?;
    let utime_ticks = fields.get(11)?.parse::<u64>().ok()?;
    let stime_ticks = fields.get(12)?.parse::<u64>().ok()?;
    let rss_pages = fields.get(21)?.parse::<i64>().ok()?.max(0) as u64;

    Some(RawProcessSample {
        pid,
        ppid,
        total_cpu_time_ns: ticks_to_nanos(
            utime_ticks.saturating_add(stime_ticks),
            clock_ticks_per_second,
        ),
        memory_bytes: rss_pages.saturating_mul(page_size),
    })
}

#[cfg(target_os = "linux")]
fn ticks_to_nanos(ticks: u64, clock_ticks_per_second: u64) -> u64 {
    ticks.saturating_mul(1_000_000_000) / clock_ticks_per_second
}

#[cfg(target_os = "linux")]
fn sysconf_u64(name: libc::c_int) -> Option<u64> {
    let value = unsafe { libc::sysconf(name) };
    (value > 0).then_some(value as u64)
}

fn total_system_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut bytes = 0_u64;
        let mut size = std::mem::size_of::<u64>();
        let name = std::ffi::CString::new("hw.memsize").ok()?;
        let result = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                (&mut bytes as *mut u64).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        return (result == 0).then_some(bytes);
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

#[cfg(target_os = "macos")]
const PROC_PIDTBSDINFO: i32 = 3;
#[cfg(target_os = "macos")]
const RUSAGE_INFO_CURRENT: i32 = 6;

#[cfg(target_os = "macos")]
#[link(name = "proc")]
unsafe extern "C" {
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> libc::c_int;
    fn proc_listchildpids(ppid: libc::pid_t, buffer: *mut libc::c_void, buffersize: i32) -> i32;
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut libc::c_void,
        buffersize: i32,
    ) -> i32;
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut libc::c_void) -> i32;
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct ProcBsdInfo {
    pbi_flags: u32,
    pbi_status: u32,
    pbi_xstatus: u32,
    pbi_pid: u32,
    pbi_ppid: u32,
    pbi_uid: libc::uid_t,
    pbi_gid: libc::gid_t,
    pbi_ruid: libc::uid_t,
    pbi_rgid: libc::gid_t,
    pbi_svuid: libc::uid_t,
    pbi_svgid: libc::gid_t,
    rfu_1: u32,
    pbi_comm: [libc::c_char; 16],
    pbi_name: [libc::c_char; 32],
    pbi_nfiles: u32,
    pbi_pgid: u32,
    pbi_pjobc: u32,
    e_tdev: u32,
    e_tpgid: u32,
    pbi_nice: i32,
    pbi_start_tvsec: u64,
    pbi_start_tvusec: u64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct RusageInfoV6 {
    ri_uuid: [u8; 16],
    ri_user_time: u64,
    ri_system_time: u64,
    ri_pkg_idle_wkups: u64,
    ri_interrupt_wkups: u64,
    ri_pageins: u64,
    ri_wired_size: u64,
    ri_resident_size: u64,
    ri_phys_footprint: u64,
    ri_proc_start_abstime: u64,
    ri_proc_exit_abstime: u64,
    ri_child_user_time: u64,
    ri_child_system_time: u64,
    ri_child_pkg_idle_wkups: u64,
    ri_child_interrupt_wkups: u64,
    ri_child_pageins: u64,
    ri_child_elapsed_abstime: u64,
    ri_diskio_bytesread: u64,
    ri_diskio_byteswritten: u64,
    ri_cpu_time_qos_default: u64,
    ri_cpu_time_qos_maintenance: u64,
    ri_cpu_time_qos_background: u64,
    ri_cpu_time_qos_utility: u64,
    ri_cpu_time_qos_legacy: u64,
    ri_cpu_time_qos_user_initiated: u64,
    ri_cpu_time_qos_user_interactive: u64,
    ri_billed_system_time: u64,
    ri_serviced_system_time: u64,
    ri_logical_writes: u64,
    ri_lifetime_max_phys_footprint: u64,
    ri_instructions: u64,
    ri_cycles: u64,
    ri_billed_energy: u64,
    ri_serviced_energy: u64,
    ri_interval_max_phys_footprint: u64,
    ri_runnable_time: u64,
    ri_flags: u64,
    ri_user_ptime: u64,
    ri_system_ptime: u64,
    ri_pinstructions: u64,
    ri_pcycles: u64,
    ri_energy_nj: u64,
    ri_penergy_nj: u64,
    ri_secure_time_in_system: u64,
    ri_secure_ptime_in_system: u64,
    ri_neural_footprint: u64,
    ri_lifetime_max_neural_footprint: u64,
    ri_interval_max_neural_footprint: u64,
    ri_reserved: [u64; 9],
}

#[cfg(test)]
mod tests {
    use super::{
        collect_process_tree, cpu_percent_between, format_memory, CpuUsageSample, ProcessSample,
        RawProcessSample, ResourceUsageSampler, TrackedProcess, MAX_CPU_SAMPLE_WINDOW,
    };
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    #[cfg(target_os = "linux")]
    use super::ticks_to_nanos;

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

    #[cfg(target_os = "linux")]
    #[test]
    fn converts_linux_ticks_to_nanoseconds() {
        assert_eq!(ticks_to_nanos(250, 100), 2_500_000_000);
    }
}
