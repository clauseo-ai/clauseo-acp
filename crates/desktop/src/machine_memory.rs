//! The machine's total physical memory, read once at start-up and passed to the host
//! (`HostConfig::with_machine_memory`). The host sets how many sessions get a browser at once
//! from it (`browser::limits::slot_limit`); without it the small machine's limit applies, so a
//! failed read costs browsers, not the app.
//!
//! macOS: `sysctlbyname("hw.memsize")`. Windows: `GlobalMemoryStatusEx`. Linux: `MemTotal` in
//! `/proc/meminfo`. Any other system: no read.
//!
//! The read runs on the thread that starts the host, before `Host::start`, never on the UI thread.

use std::io;

/// The machine's total physical memory in bytes, or why it could not be read.
#[cfg(target_os = "linux")]
pub(crate) fn total_bytes() -> io::Result<u64> {
    let text = std::fs::read_to_string("/proc/meminfo")?;
    parse_meminfo(&text).ok_or_else(|| io::Error::other("no MemTotal in /proc/meminfo"))
}

/// The machine's total physical memory in bytes, or why it could not be read.
#[cfg(target_os = "macos")]
pub(crate) fn total_bytes() -> io::Result<u64> {
    mac::total_bytes()
}

/// The machine's total physical memory in bytes, or why it could not be read.
#[cfg(windows)]
pub(crate) fn total_bytes() -> io::Result<u64> {
    win::total_bytes()
}

/// The machine's total physical memory in bytes, or why it could not be read.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) fn total_bytes() -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no total-memory read on this system",
    ))
}

/// The `MemTotal` line of `/proc/meminfo`'s text, in bytes: `MemTotal:   16318584 kB`. The unit
/// is kibibytes (the kernel's `kB`); any other unit, a missing line or a value that overflows
/// gives `None`.
#[cfg(any(test, target_os = "linux"))]
fn parse_meminfo(text: &str) -> Option<u64> {
    let rest = text
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))?;
    let mut fields = rest.split_whitespace();
    let kib: u64 = fields.next()?.parse().ok()?;
    if fields.next()? != "kB" || fields.next().is_some() {
        return None;
    }
    kib.checked_mul(1024)
}

/// `config` with the machine's memory passed through `set`, when `read` gives it. When the read
/// fails `set` is not called, `config` goes back unchanged, and the failure is logged at warn:
/// the host then keeps its default browser limit.
pub(crate) fn pass_to<C>(
    config: C,
    read: impl FnOnce() -> io::Result<u64>,
    set: impl FnOnce(C, u64) -> C,
) -> C {
    match read() {
        Ok(bytes) => set(config, bytes),
        Err(err) => {
            tracing::warn!(
                %err,
                "the machine's memory could not be read: the host keeps its default browser limit"
            );
            config
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    #![expect(
        unsafe_code,
        reason = "sysctlbyname, which the libc crate binds and no other call here does"
    )]

    use std::io;

    /// `hw.memsize`: the physical memory as a 64-bit count of bytes.
    pub(super) fn total_bytes() -> io::Result<u64> {
        let mut bytes: u64 = 0;
        let mut len = size_of::<u64>();
        // SAFETY: the name is a NUL-terminated C string; `bytes` and `len` are the value and
        // the size sysctl writes, and no new value is passed (`newp` null, `newlen` 0).
        let status = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&raw mut bytes).cast(),
                &raw mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if status != 0 {
            return Err(io::Error::last_os_error());
        }
        if len != size_of::<u64>() {
            return Err(io::Error::other("hw.memsize is not a 64-bit value"));
        }
        Ok(bytes)
    }
}

#[cfg(windows)]
mod win {
    #![expect(
        unsafe_code,
        reason = "GlobalMemoryStatusEx, the Win32 call the windows crate binds"
    )]

    use std::io;

    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    /// `ullTotalPhys` from `GlobalMemoryStatusEx`: the physical memory in bytes.
    pub(super) fn total_bytes() -> io::Result<u64> {
        let length = u32::try_from(size_of::<MEMORYSTATUSEX>()).map_err(io::Error::other)?;
        let mut status = MEMORYSTATUSEX {
            dwLength: length,
            ..Default::default()
        };
        // SAFETY: `status` is a live MEMORYSTATUSEX whose `dwLength` is its size, as the call
        // requires.
        unsafe { GlobalMemoryStatusEx(&raw mut status) }.map_err(io::Error::other)?;
        Ok(status.ullTotalPhys)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io;

    use super::{parse_meminfo, pass_to};

    const MEMINFO: &str = "MemTotal:       16318584 kB\nMemFree:         1234567 kB\n";

    #[test]
    fn meminfo_memtotal_is_kibibytes() {
        assert_eq!(parse_meminfo(MEMINFO), Some(16_318_584 * 1024));
    }

    #[test]
    fn meminfo_without_memtotal_or_with_another_unit_is_none() {
        assert_eq!(parse_meminfo("MemFree:  1 kB\n"), None);
        assert_eq!(parse_meminfo(""), None);
        assert_eq!(parse_meminfo("MemTotal:  16 MB\n"), None);
        assert_eq!(parse_meminfo("MemTotal:  16\n"), None);
        assert_eq!(parse_meminfo("MemTotal:  lots kB\n"), None);
        assert_eq!(parse_meminfo("MemTotal:  16 kB extra\n"), None);
        assert_eq!(parse_meminfo(&format!("MemTotal: {} kB\n", u64::MAX)), None);
    }

    #[test]
    fn a_read_that_works_passes_its_bytes_to_set() {
        let calls = Cell::new(0);
        let config = pass_to(
            "config",
            || Ok(8 << 30),
            |config, bytes| {
                calls.set(calls.get() + 1);
                assert_eq!(bytes, 8 << 30);
                config
            },
        );
        assert_eq!(config, "config");
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn a_failed_read_makes_no_call_and_keeps_the_config() {
        let reads = Cell::new(0);
        let config = pass_to(
            "config",
            || {
                reads.set(reads.get() + 1);
                Err(io::Error::other("no /proc/meminfo"))
            },
            |_, _| panic!("set must not run when the read fails"),
        );
        assert_eq!(config, "config");
        assert_eq!(reads.get(), 1);
    }
}
