//! When two stamps of one file (its size and modification time, as the stat cache keeps them)
//! count as the same content, at each place that compares them. Kept in one module, one
//! constant per place, so a sync client's row can change one without the others.
//!
//! The size must be equal; the modification times may differ by at most the place's
//! tolerance, as an absolute difference in nanoseconds (never by cutting both to whole
//! microseconds, which can put two times a few nanoseconds apart on either side of a
//! boundary). Neither the status-change time nor the creation time is ever compared: both move
//! when a sync client evicts or downloads a file.
//!
//! Why a tolerance: iCloud Drive hands a file's modification time back rounded once, on its
//! first download (seen on the Mac mini, 2026-11-17, three runs: +40 ns, -111 ns and +95 ns,
//! e.g. 1794905650.790433128 local and after eviction, 1794905650.790433168 after the first
//! download, stable after that). Evicting keeps the time and the size exactly. `OneDrive` has no
//! row yet.

use crate::stat::{FileTime, Stat};

/// The first-read rule's tolerance (`first_read.rs`, `gone.rs`, through
/// `skip_record::same_stat`): a file skipped as cloud-only, read after the agent's read
/// downloaded it, is unchanged (a baseline, not changed with no earlier copy) within this. 1 us.
pub(crate) const FIRST_READ_NS: u64 = 1_000;

/// Carrying a held file forward (`checkpoint/walk_folder.rs` `carried`): a file found cloud-only
/// holds the blob the cache read it as within this. 1 us.
pub(crate) const CARRY_FORWARD_NS: u64 = 1_000;

/// Whether a file of `size` modified at `mtime` has the stamp `stat` within `tolerance` ns.
pub(crate) fn same(size: u32, mtime: FileTime, stat: &Stat, tolerance: u64) -> bool {
    size == stat.size && apart(mtime, stat.mtime) <= tolerance
}

/// How far apart `a` and `b` are, in nanoseconds.
fn apart(a: FileTime, b: FileTime) -> u64 {
    let ns = |time: FileTime| u64::from(time.secs) * 1_000_000_000 + u64::from(time.nanos);
    ns(a).abs_diff(ns(b))
}

#[cfg(test)]
mod tests {
    use super::{FIRST_READ_NS, same};
    use crate::stat::{FileTime, Stat};

    fn stat(size: u32, secs: u32, nanos: u32) -> Stat {
        Stat {
            size,
            mtime: FileTime { secs, nanos },
            ..Stat::default()
        }
    }

    #[test]
    fn the_mini_s_first_download_rounding_matches_and_more_does_not() {
        let local = FileTime {
            secs: 1_794_905_650,
            nanos: 790_433_128,
        };
        // The mini's three first downloads: +40 ns, -111 ns, +95 ns.
        assert!(same(
            3677,
            local,
            &stat(3677, 1_794_905_650, 790_433_168),
            FIRST_READ_NS
        ));
        assert!(same(
            3677,
            local,
            &stat(3677, 1_794_905_650, 790_433_017),
            FIRST_READ_NS
        ));
        assert!(same(
            3677,
            local,
            &stat(3677, 1_794_905_650, 790_433_223),
            FIRST_READ_NS
        ));
        // 1 us is the edge; 2 us apart is another time.
        assert!(same(
            3677,
            local,
            &stat(3677, 1_794_905_650, 790_434_128),
            FIRST_READ_NS
        ));
        assert!(!same(
            3677,
            local,
            &stat(3677, 1_794_905_650, 790_435_128),
            FIRST_READ_NS
        ));
        // Across a second's boundary, by the absolute difference.
        let late = FileTime {
            secs: 1_794_905_650,
            nanos: 999_999_900,
        };
        assert!(same(
            3677,
            late,
            &stat(3677, 1_794_905_651, 50),
            FIRST_READ_NS
        ));
        // Another size is another file, whatever the time.
        assert!(!same(
            3677,
            local,
            &stat(3678, 1_794_905_650, 790_433_128),
            FIRST_READ_NS
        ));
    }
}
