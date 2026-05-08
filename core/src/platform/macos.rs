use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::open_in::{command_exists, OpenInAppKind};
use crate::process::{RawProcessSample, TrackedProcess};

use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct MacosPlatform;

impl HeadlessPlatform for MacosPlatform {
    fn name() -> &'static str {
        "macos"
    }

    fn modifier_label() -> &'static str {
        "Cmd"
    }

    fn open_external_url(url: &str) -> Result<(), String> {
        Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("Could not open URL externally: {err}"))
    }

    fn total_system_memory_bytes() -> Option<u64> {
        sysctl_hw_memsize()
    }

    fn read_process_samples(
        app_pid: u32,
        tracked_processes: &[TrackedProcess],
    ) -> Vec<RawProcessSample> {
        darwin_read_process_samples(app_pid, tracked_processes)
    }

    fn is_open_in_app_available(app: OpenInAppKind) -> bool {
        match app {
            OpenInAppKind::Cursor => {
                macos_app_exists("Cursor") || command_exists(&["cursor", "cursor-cli"])
            }
            OpenInAppKind::Zed => macos_app_exists("Zed") || command_exists(&["zed"]),
            OpenInAppKind::VsCode => {
                macos_app_exists("Visual Studio Code") || command_exists(&["code"])
            }
            OpenInAppKind::PhpStorm => {
                macos_app_exists("PhpStorm") || command_exists(&["phpstorm"])
            }
            OpenInAppKind::Ghostty => macos_app_exists("Ghostty") || command_exists(&["ghostty"]),
            OpenInAppKind::WezTerm => macos_app_exists("WezTerm") || command_exists(&["wezterm"]),
            OpenInAppKind::SystemTerminal => macos_app_exists("Terminal"),
            OpenInAppKind::FileManager => macos_app_exists("Finder"),
        }
    }

    fn command_for_open_in(app: OpenInAppKind, path: &Path) -> Command {
        let mut command = Command::new("open");
        match app {
            OpenInAppKind::Cursor => {
                command.args(["-a", "Cursor"]).arg(path);
            }
            OpenInAppKind::Zed => {
                command.args(["-a", "Zed"]).arg(path);
            }
            OpenInAppKind::VsCode => {
                command.args(["-a", "Visual Studio Code"]).arg(path);
            }
            OpenInAppKind::PhpStorm => {
                command.args(["-a", "PhpStorm"]).arg(path);
            }
            OpenInAppKind::Ghostty => {
                command.args(["-a", "Ghostty"]).arg(path);
            }
            OpenInAppKind::WezTerm => {
                command
                    .args(["-a", "WezTerm", "--args", "start", "--cwd"])
                    .arg(path);
            }
            OpenInAppKind::SystemTerminal => {
                command.args(["-a", "Terminal"]).arg(path);
            }
            OpenInAppKind::FileManager => {
                command.arg(path);
            }
        }
        command
    }
}

fn macos_app_exists(app_name: &str) -> bool {
    macos_app_candidates(app_name)
        .into_iter()
        .any(|path| path.exists())
}

fn macos_app_candidates(app_name: &str) -> Vec<PathBuf> {
    let bundle_name = format!("{app_name}.app");
    let mut candidates = vec![
        PathBuf::from("/Applications").join(&bundle_name),
        PathBuf::from("/Applications/Utilities").join(&bundle_name),
        PathBuf::from("/System/Applications").join(&bundle_name),
        PathBuf::from("/System/Applications/Utilities").join(&bundle_name),
        PathBuf::from("/System/Library/CoreServices").join(&bundle_name),
    ];

    if let Some(home_dir) = dirs::home_dir() {
        candidates.push(home_dir.join("Applications").join(bundle_name));
    }

    candidates
}

/// Query `hw.memsize` via `sysctlbyname`. Shared with `IosPlatform`,
/// which uses the same Darwin syscall.
pub(super) fn sysctl_hw_memsize() -> Option<u64> {
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
    (result == 0).then_some(bytes)
}

/// Sample `app_pid` itself plus the descendants of each tracked
/// process root, returning a [`RawProcessSample`] per process the
/// kernel will let us read. Shared with `IosPlatform` because the Darwin
/// `proc_pidinfo` / `proc_pid_rusage` interfaces are identical on
/// both — though iOS sandboxing may scope down which descendants are
/// actually visible.
pub(super) fn darwin_read_process_samples(
    app_pid: u32,
    tracked_processes: &[TrackedProcess],
) -> Vec<RawProcessSample> {
    let mut visited = HashSet::new();
    let mut samples = Vec::new();

    if visited.insert(app_pid) {
        if let Some(sample) = read_process_sample(app_pid) {
            samples.push(sample);
        }
    }

    for tracked in tracked_processes {
        collect_darwin_process_tree(tracked.pid, &mut visited, &mut samples);
    }

    samples
}

fn collect_darwin_process_tree(
    root_pid: u32,
    visited: &mut HashSet<u32>,
    samples: &mut Vec<RawProcessSample>,
) {
    let mut stack = vec![root_pid];
    while let Some(pid) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }

        if let Some(sample) = read_process_sample(pid) {
            stack.extend(list_child_pids(pid));
            samples.push(sample);
        }
    }
}

fn read_process_sample(pid: u32) -> Option<RawProcessSample> {
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

fn list_child_pids(ppid: u32) -> Vec<u32> {
    let mut child_pids = vec![0_i32; 32];

    loop {
        let count = unsafe {
            ffi::proc_listchildpids(
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

fn proc_pid_bsdinfo(pid: u32) -> Option<ffi::ProcBsdInfo> {
    let mut info = ffi::ProcBsdInfo::default();
    let result = unsafe {
        ffi::proc_pidinfo(
            pid as i32,
            ffi::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut ffi::ProcBsdInfo).cast(),
            std::mem::size_of::<ffi::ProcBsdInfo>() as i32,
        )
    };
    (result == std::mem::size_of::<ffi::ProcBsdInfo>() as i32).then_some(info)
}

fn proc_pid_rusage_info(pid: u32) -> Option<ffi::RusageInfoV6> {
    let mut info = ffi::RusageInfoV6::default();
    let result = unsafe {
        ffi::proc_pid_rusage(
            pid as i32,
            ffi::RUSAGE_INFO_CURRENT,
            (&mut info as *mut ffi::RusageInfoV6).cast(),
        )
    };
    (result == 0).then_some(info)
}

fn mach_time_units_to_nanos(value: u64) -> u64 {
    let timebase = mach_timebase();
    value.saturating_mul(timebase.numer as u64) / timebase.denom as u64
}

fn mach_timebase() -> &'static ffi::MachTimebaseInfo {
    static TIMEBASE: OnceLock<ffi::MachTimebaseInfo> = OnceLock::new();
    TIMEBASE.get_or_init(|| {
        let mut info = ffi::MachTimebaseInfo::default();
        let result = unsafe { ffi::mach_timebase_info(&mut info) };
        if result != 0 || info.numer == 0 || info.denom == 0 {
            ffi::MachTimebaseInfo { numer: 1, denom: 1 }
        } else {
            info
        }
    })
}

mod ffi {
    pub const PROC_PIDTBSDINFO: i32 = 3;
    pub const RUSAGE_INFO_CURRENT: i32 = 6;

    #[link(name = "proc")]
    unsafe extern "C" {
        pub fn mach_timebase_info(info: *mut MachTimebaseInfo) -> libc::c_int;
        pub fn proc_listchildpids(
            ppid: libc::pid_t,
            buffer: *mut libc::c_void,
            buffersize: i32,
        ) -> i32;
        pub fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut libc::c_void,
            buffersize: i32,
        ) -> i32;
        pub fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut libc::c_void) -> i32;
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct MachTimebaseInfo {
        pub numer: u32,
        pub denom: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct ProcBsdInfo {
        pbi_flags: u32,
        pbi_status: u32,
        pbi_xstatus: u32,
        pbi_pid: u32,
        pub pbi_ppid: u32,
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

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct RusageInfoV6 {
        ri_uuid: [u8; 16],
        pub ri_user_time: u64,
        pub ri_system_time: u64,
        ri_pkg_idle_wkups: u64,
        ri_interrupt_wkups: u64,
        ri_pageins: u64,
        ri_wired_size: u64,
        ri_resident_size: u64,
        pub ri_phys_footprint: u64,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_macos() {
        assert_eq!(MacosPlatform::name(), "macos");
    }

    #[test]
    fn modifier_label_returns_cmd() {
        assert_eq!(MacosPlatform::modifier_label(), "Cmd");
    }

    #[test]
    fn total_system_memory_bytes_is_positive() {
        let memory = MacosPlatform::total_system_memory_bytes();
        assert!(
            memory.is_some(),
            "expected sysctlbyname to succeed on macOS"
        );
        assert!(
            memory.unwrap() > 0,
            "expected total memory > 0, got {:?}",
            memory
        );
    }

    #[test]
    fn read_process_samples_returns_self() {
        let pid = std::process::id();
        let samples = MacosPlatform::read_process_samples(pid, &[]);
        assert!(
            samples.iter().any(|s| s.pid == pid),
            "expected the process tree walk to include our own pid {}",
            pid
        );
    }
}
