use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use another_one_core::platform::{CurrentPlatform as CorePlatform, HeadlessPlatform};
use gpui::{point, px, App, TitlebarOptions, Window, WindowDecorations};

use super::PlatformServices;
use crate::assets::asset_root;
use crate::open_in::{command_exists, OpenInAppKind};
use crate::resource_usage::{RawProcessSample, TrackedProcess};

pub struct MacPlatform;

impl PlatformServices for MacPlatform {
    fn open_external_url(url: &str) -> Result<(), String> {
        // Single source of truth lives in `core::platform::HeadlessPlatform`.
        CorePlatform::open_external_url(url)
    }

    fn platform_modifier_label() -> &'static str {
        // Single source of truth lives in `core::platform::HeadlessPlatform`.
        // This wrapper exists only because the desktop `PlatformServices`
        // trait predates the core abstraction; it'll be removed when the
        // GPUI binary is deleted in the Flutter migration's Phase 6.
        CorePlatform::modifier_label()
    }

    fn read_process_samples(
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

            if let Some(sample) = read_process_sample(pid) {
                stack.extend(list_child_pids(pid));
                samples.push(sample);
            }
        }

        samples
    }

    fn total_system_memory_bytes() -> Option<u64> {
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

    fn is_open_in_app_available(app: OpenInAppKind) -> bool {
        match app {
            OpenInAppKind::Cursor => {
                macos_app_exists("Cursor") || command_exists(&["cursor", "cursor-cli"])
            }
            OpenInAppKind::Zed => macos_app_exists("Zed") || command_exists(&["zed"]),
            OpenInAppKind::VsCode => {
                macos_app_exists("Visual Studio Code") || command_exists(&["code"])
            }
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
            OpenInAppKind::FileManager => {
                command.arg(path);
            }
        }
        command
    }

    fn titlebar_options(_title: &str) -> TitlebarOptions {
        TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(point(px(13.), px(10.))),
        }
    }

    fn window_decorations() -> Option<WindowDecorations> {
        None
    }

    fn traffic_light_pad_px() -> f32 {
        76.
    }

    fn toggle_left_margin_px() -> f32 {
        0.
    }

    fn set_app_dock_icon(_cx: &mut App) {
        use cocoa::appkit::{NSApp, NSApplication, NSImage};
        use cocoa::base::nil;
        use cocoa::foundation::NSString;
        use objc::runtime::Object;

        let asset_root = asset_root();
        let icon_path = [
            asset_root.join("assets/app-icon/source/another-one.png"),
            asset_root.join("assets/app-icon/macos/AnotherOne.icns"),
            asset_root.join("AnotherOne.icns"),
        ]
        .into_iter()
        .find(|path| path.exists());

        let Some(icon_path) = icon_path else {
            return;
        };

        unsafe {
            let path_str = NSString::alloc(nil).init_str(icon_path.to_str().unwrap());
            let image: *mut Object = NSImage::alloc(nil).initWithContentsOfFile_(path_str);
            if image != nil {
                let app = NSApp();
                app.setApplicationIconImage_(image);
            }
        }
    }

    fn supports_custom_chrome(_window: &Window) -> bool {
        true
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
        PathBuf::from("/System/Applications").join(&bundle_name),
        PathBuf::from("/System/Library/CoreServices").join(&bundle_name),
    ];

    if let Some(home_dir) = dirs::home_dir() {
        candidates.push(home_dir.join("Applications").join(bundle_name));
    }

    candidates
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
