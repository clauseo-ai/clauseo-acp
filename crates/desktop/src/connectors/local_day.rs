//! The OS's time zone for the case-law relay's day cap (R24, decision D5): the offset from UTC
//! at a given time, so the 600 calls a day reset at the lawyer's local midnight, daylight saving
//! included. `localtime_r`'s `tm_gmtoff` on macOS and Linux; Windows converts the UTC time to
//! local time with the current time zone's rules. A failed read counts as UTC.

use clauseo_host::connectors::LocalDay;

/// The OS's time zone.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OsLocalDay;

impl LocalDay for OsLocalDay {
    fn utc_offset_secs(&self, unix_secs: i64) -> i32 {
        offset(unix_secs).unwrap_or(0)
    }
}

#[cfg(unix)]
#[expect(unsafe_code, reason = "localtime_r, the OS's time zone")]
fn offset(unix_secs: i64) -> Option<i32> {
    let time = libc::time_t::try_from(unix_secs).ok()?;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::zeroed();
    // SAFETY: `time` and `tm` are valid for the call; `localtime_r` writes only `tm` and is
    // thread-safe, unlike `localtime`.
    let filled = unsafe { libc::localtime_r(&raw const time, tm.as_mut_ptr()) };
    if filled.is_null() {
        return None;
    }
    // SAFETY: `localtime_r` returned non-null, so it filled `tm`.
    let tm = unsafe { tm.assume_init() };
    i32::try_from(tm.tm_gmtoff).ok()
}

#[cfg(windows)]
#[expect(unsafe_code, reason = "the Win32 time conversions, the OS's time zone")]
fn offset(unix_secs: i64) -> Option<i32> {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{
        FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
    };
    /// Seconds from 1601-01-01 (the `FILETIME` epoch) to 1970-01-01.
    const EPOCH_GAP: i64 = 11_644_473_600;
    /// `FILETIME` ticks (100 ns) per second.
    const TICKS: i64 = 10_000_000;
    let ticks = unix_secs.checked_add(EPOCH_GAP)?.checked_mul(TICKS)?;
    let wide = u64::try_from(ticks).ok()?;
    let utc_file = FILETIME {
        dwLowDateTime: u32::try_from(wide & 0xFFFF_FFFF).ok()?,
        dwHighDateTime: u32::try_from(wide >> 32).ok()?,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    let mut local_file = FILETIME::default();
    // SAFETY: each pointer is to a live local of the type the call takes; `None` is the current
    // time zone.
    unsafe {
        FileTimeToSystemTime(&raw const utc_file, &raw mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &raw const utc, &raw mut local).ok()?;
        SystemTimeToFileTime(&raw const local, &raw mut local_file).ok()?;
    }
    let local_ticks =
        (i64::from(local_file.dwHighDateTime) << 32) | i64::from(local_file.dwLowDateTime);
    i32::try_from(local_ticks.checked_sub(ticks)? / TICKS).ok()
}

#[cfg(not(any(unix, windows)))]
fn offset(_unix_secs: i64) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::OsLocalDay;
    use clauseo_host::connectors::LocalDay as _;

    /// Every zone on Earth is within 14 hours of UTC, in whole minutes, on either side of a
    /// year (the daylight saving change included).
    #[test]
    fn the_offset_is_a_real_time_zone_offset() {
        for unix_secs in [0, 1_767_225_600, 1_783_000_000] {
            let offset = OsLocalDay.utc_offset_secs(unix_secs);
            assert!(offset.abs() <= 14 * 3_600, "{offset}");
            assert_eq!(offset % 60, 0, "{offset}");
        }
    }
}
