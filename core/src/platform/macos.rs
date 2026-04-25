use std::process::Command;

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
        assert!(memory.is_some(), "expected sysctlbyname to succeed on macOS");
        assert!(
            memory.unwrap() > 0,
            "expected total memory > 0, got {:?}",
            memory
        );
    }
}
