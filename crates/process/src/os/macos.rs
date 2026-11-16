//! The macOS listing of an agent's descendants, through libproc: `proc_listchildpids` walks the
//! tree, `proc_pidinfo(PROC_PIDTBSDINFO)` gives each process's parent, group, start and state,
//! and `proc_pidpath` its executable. `proc_listallpids` lists every process, for the processes
//! with a given executable name and their starts (`named`). Each libproc call has one wrapper
//! here, with its `unsafe` block and the reason it is sound; nothing else in the crate calls
//! libproc.
#![expect(
    unsafe_code,
    reason = "FFI to libproc (proc_listchildpids, proc_listallpids, proc_pidinfo, proc_pidpath)"
)]

use std::ffi::{OsStr, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;

use super::name_match::{Read, Reader, Seen, Start, starts_named};
use crate::descendants::Proc;

/// `SZOMB` from `<sys/proc.h>`: `pbi_status` of a process that has exited and waits to be
/// reaped. The libc crate does not export it.
const SZOMB: u32 = 5;

/// How many child pids the first `proc_listchildpids` call makes room for; a full buffer is
/// retried with twice the room, up to [`MAX_CHILDREN`].
const FIRST_CHILDREN: usize = 64;
/// The most children of one process the walk reads.
const MAX_CHILDREN: usize = 65_536;
/// How many pids the first `proc_listallpids` call makes room for; a full buffer is retried with
/// twice the room, up to [`MAX_PIDS`].
const FIRST_PIDS: usize = 1_024;
/// The most processes [`named`] reads; a system with more fails the listing.
const MAX_PIDS: usize = 1 << 20;

/// What `proc_pidinfo(PROC_PIDTBSDINFO)` says about a process.
#[derive(Debug, Clone, Copy)]
struct Info {
    parent: u32,
    group: u32,
    /// `pbi_start_tvsec` and `pbi_start_tvusec` in microseconds: with the pid it names one
    /// process, as a reused pid has a later start.
    start: u64,
    exited: bool,
}

/// Now, as microseconds of wall time since the epoch, rounded down: the clock and unit of
/// `pbi_start_tvsec` and `pbi_start_tvusec`. `None` when the call fails.
pub(super) fn now() -> Option<u64> {
    let time = nix::time::clock_gettime(nix::time::ClockId::CLOCK_REALTIME).ok()?;
    let seconds = u64::try_from(time.tv_sec()).ok()?;
    let micros = u64::try_from(time.tv_nsec()).ok()? / 1_000;
    Some(seconds.checked_mul(1_000_000)? + micros)
}

/// The start of a running process `pid`: `None` when it is gone, unreadable, or has exited.
pub(super) fn start_of(pid: u32) -> Option<u64> {
    info(pid).filter(|info| !info.exited).map(|info| info.start)
}

/// Every running descendant of `root`, whose start must still be `root_start`; `None` when the
/// root is gone (or its pid now names another process) or a child list cannot be read.
pub(super) fn list(root: u32, root_start: u64) -> Option<Vec<Proc>> {
    if start_of(root) != Some(root_start) {
        return None;
    }
    let mut found = Vec::new();
    let mut next = vec![root];
    while let Some(parent) = next.pop() {
        for pid in children(parent)? {
            // A child that exits between the two calls, or that a race re-parented, is skipped.
            let Some(info) = info(pid).filter(|info| !info.exited && info.parent == parent) else {
                continue;
            };
            next.push(pid);
            found.push(Proc {
                pid,
                parent: Some(parent),
                start: info.start,
                leads_group: info.group == pid,
                exe: path(pid),
            });
        }
    }
    Some(found)
}

/// The pids whose parent is `parent`; `None` when the call answers -1 (its failure), or for a
/// pid out of range. A count of 0 reads as no children: xnu's `proc_listpids`, which it calls,
/// answers a failed system call with 0, so that failure reads as no children too.
fn children(parent: u32) -> Option<Vec<u32>> {
    let parent = c_int::try_from(parent).ok()?;
    let mut room = FIRST_CHILDREN;
    loop {
        let mut pids: Vec<c_int> = vec![0; room];
        let bytes = c_int::try_from(pids.len() * size_of::<c_int>()).ok()?;
        // SAFETY: `pids` is a live, zeroed buffer of exactly `bytes` bytes that this call may
        // fill with pids and nothing else; the kernel writes at most `bytes` bytes into it.
        // The return counts pids (xnu libsyscall/wrappers/libproc/libproc.c:88-97 divides the
        // byte count by `sizeof(int)`) or is -1 on failure, which `usize::try_from` below
        // refuses; a count is used only capped at the buffer's length, so no read passes it.
        let count = unsafe { libc::proc_listchildpids(parent, pids.as_mut_ptr().cast(), bytes) };
        let count = usize::try_from(count).ok()?;
        if count < room || room >= MAX_CHILDREN {
            pids.truncate(count.min(room));
            // Unused slots stay 0, which is never a child's pid.
            return Some(
                pids.into_iter()
                    .filter_map(|pid| u32::try_from(pid).ok().filter(|&pid| pid != 0))
                    .collect(),
            );
        }
        room *= 2;
    }
}

/// The starts of the running processes whose executable's file name is `name` (`Microsoft
/// Word` for `/Applications/Microsoft Word.app/Contents/MacOS/Microsoft Word`, wherever the app
/// sits), compared byte for byte; an empty list when none runs.
///
/// Each process is matched by `proc_pidpath`, which xnu answers for any user's process (it is
/// exempt from `proc_pidinfo`'s same-user check, unlike `PROC_PIDTBSDINFO`, which answers
/// EPERM for root's and other users' processes). A read that fails with ESRCH skips the process
/// at any step: it exited since the listing, and may wait as a zombie, which libproc's reads do
/// not see but `kill(pid, 0)` does (`name_match::starts_named`). A process whose path cannot be
/// read otherwise (its app bundle replaced by an update) is read by
/// `PROC_PIDT_SHORTBSDINFO`, also exempt: one that has exited is skipped, and one that runs
/// with `name` as its `comm`, or that runs and cannot be read at all, makes the answer `None`
/// (`name_match::kind`). Only a match is read further, by `PROC_PIDTBSDINFO`, for its start.
/// `None` when the listing fails, or a match that has not exited cannot be read (another
/// user's Word): the caller cannot tell.
pub(crate) fn named(name: &str) -> Option<Vec<SystemTime>> {
    starts_named(name, all_pids(), &Libproc)
}

/// The per-process reads of [`named`], through libproc.
struct Libproc;

impl Reader for Libproc {
    fn file(&self, pid: u32) -> Read<Vec<u8>> {
        match read_path(pid) {
            Read::Got(exe) => Read::Got(
                exe.file_name()
                    .map(OsStrExt::as_bytes)
                    .unwrap_or_default()
                    .to_vec(),
            ),
            Read::Gone => Read::Gone,
            Read::Failed => Read::Failed,
        }
    }

    fn short(&self, pid: u32) -> Read<Seen> {
        match short_info(pid) {
            Read::Got(short) => Read::Got(Seen {
                comm: c_bytes(&short.pbsi_comm),
                exited: short.pbsi_status == SZOMB,
            }),
            Read::Gone => Read::Gone,
            Read::Failed => Read::Failed,
        }
    }

    fn gone(&self, pid: u32) -> bool {
        gone(pid)
    }

    fn start(&self, pid: u32) -> Read<Start> {
        match read_bsd(pid) {
            Read::Got(bsd) if bsd.pbi_status == SZOMB => Read::Got(Start::Exited),
            Read::Got(bsd) => {
                start_time(&bsd).map_or(Read::Failed, |start| Read::Got(Start::Running(start)))
            }
            Read::Gone => Read::Gone,
            Read::Failed => Read::Failed,
        }
    }
}

/// How a libproc call that wrote nothing failed, from the `errno` it left: [`Read::Gone`] for
/// `ESRCH` (no process with the pid can be read), else [`Read::Failed`]. libproc's
/// `proc_pidinfo` and `proc_pidpath` answer a failed `__proc_info` system call with 0 and its
/// `errno` set (xnu libsyscall/wrappers/libproc/libproc.c); `errno` is cleared before each
/// call, so an answer of 0 that did not fail reads as failed, never as gone.
fn failure<T>() -> Read<T> {
    if Errno::last() == Errno::ESRCH {
        Read::Gone
    } else {
        Read::Failed
    }
}

/// A NUL-terminated `c_char` field, as its bytes before the first NUL (all of it when it has
/// none, as `pbsi_comm` may for a 16-byte name).
fn c_bytes(field: &[std::ffi::c_char]) -> Vec<u8> {
    field
        .iter()
        .map(|c| c.to_ne_bytes()[0])
        .take_while(|&byte| byte != 0)
        .collect()
}

/// `proc_pidinfo(PROC_PIDT_SHORTBSDINFO)` for `pid`; [`Read::Gone`] when it fails with
/// `ESRCH`, [`Read::Failed`] when it fails otherwise or answers short.
fn short_info(pid: u32) -> Read<libc::proc_bsdshortinfo> {
    let Ok(pid) = c_int::try_from(pid) else {
        return Read::Failed;
    };
    let mut short = std::mem::MaybeUninit::<libc::proc_bsdshortinfo>::zeroed();
    let Ok(size) = c_int::try_from(size_of::<libc::proc_bsdshortinfo>()) else {
        return Read::Failed;
    };
    Errno::clear();
    // SAFETY: `short` is a live buffer of exactly `size` bytes, the size of
    // `proc_bsdshortinfo`, the struct the `PROC_PIDT_SHORTBSDINFO` flavour fills; the call
    // writes at most `size` bytes. The return is the byte count written, and anything but
    // `size` is refused before the struct is read.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDT_SHORTBSDINFO,
            0,
            short.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if written == 0 {
        return failure();
    }
    if written != size {
        return Read::Failed;
    }
    // SAFETY: the call above wrote the whole struct (`written == size`), and it was zeroed
    // before, so every field is initialised; `proc_bsdshortinfo` is plain integers and arrays.
    Read::Got(unsafe { short.assume_init() })
}

/// Whether no process `pid` exists now: `kill(pid, 0)` answers `ESRCH`.
fn gone(pid: u32) -> bool {
    i32::try_from(pid).is_ok_and(|pid| kill(Pid::from_raw(pid), None) == Err(Errno::ESRCH))
}

/// A process's start as wall time: `pbi_start_tvsec` and `pbi_start_tvusec` since the epoch.
fn start_time(bsd: &libc::proc_bsdinfo) -> Option<SystemTime> {
    SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_secs(bsd.pbi_start_tvsec))?
        .checked_add(Duration::from_micros(bsd.pbi_start_tvusec))
}

/// Every pid on the system but 0 (the kernel, never a named process, and a pid `kill` reads as
/// this process's group); `None` when the call fails, or answers 0 (as xnu answers a failed
/// system call; a running system always has processes), or more than [`MAX_PIDS`] run.
fn all_pids() -> Option<Vec<u32>> {
    let mut room = FIRST_PIDS;
    loop {
        let mut pids: Vec<c_int> = vec![0; room];
        let bytes = c_int::try_from(pids.len() * size_of::<c_int>()).ok()?;
        // SAFETY: `pids` is a live, zeroed buffer of exactly `bytes` bytes that this call may
        // fill with pids and nothing else; the kernel writes at most `bytes` bytes into it. The
        // return counts pids (xnu libsyscall/wrappers/libproc/libproc.c divides the byte count
        // by `sizeof(int)`) or is -1 on failure, which `usize::try_from` below refuses; a count
        // is used only capped at the buffer's length, so no read passes it.
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        let count = usize::try_from(count).ok().filter(|&count| count > 0)?;
        if count < room {
            pids.truncate(count);
            return Some(
                pids.into_iter()
                    .filter_map(|pid| u32::try_from(pid).ok().filter(|&pid| pid != 0))
                    .collect(),
            );
        }
        if room >= MAX_PIDS {
            return None;
        }
        room *= 2;
    }
}

/// `proc_pidinfo(PROC_PIDTBSDINFO)` for `pid`; `None` when the call fails or answers short.
fn info(pid: u32) -> Option<Info> {
    let bsd = bsd_info(pid)?;
    Some(Info {
        parent: bsd.pbi_ppid,
        group: bsd.pbi_pgid,
        start: bsd
            .pbi_start_tvsec
            .saturating_mul(1_000_000)
            .saturating_add(bsd.pbi_start_tvusec),
        exited: bsd.pbi_status == SZOMB,
    })
}

/// The whole `proc_bsdinfo` of `pid`; `None` when the call fails or answers short.
fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    read_bsd(pid).got()
}

/// `proc_pidinfo(PROC_PIDTBSDINFO)` for `pid`; [`Read::Gone`] when it fails with `ESRCH`,
/// [`Read::Failed`] when it fails otherwise (`EPERM` for another user's process) or answers
/// short.
fn read_bsd(pid: u32) -> Read<libc::proc_bsdinfo> {
    let Ok(pid) = c_int::try_from(pid) else {
        return Read::Failed;
    };
    let mut bsd = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let Ok(size) = c_int::try_from(size_of::<libc::proc_bsdinfo>()) else {
        return Read::Failed;
    };
    Errno::clear();
    // SAFETY: `bsd` is a live buffer of exactly `size` bytes, the size of `proc_bsdinfo`, the
    // struct the `PROC_PIDTBSDINFO` flavour fills; the call writes at most `size` bytes. The
    // return is the byte count written, and anything but `size` (0 for a gone pid, a short
    // write) is refused before the struct is read.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            bsd.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if written == 0 {
        return failure();
    }
    if written != size {
        return Read::Failed;
    }
    // SAFETY: the call above wrote the whole struct (`written == size`), and it was zeroed
    // before, so every field is initialised; `proc_bsdinfo` is plain integers and arrays.
    Read::Got(unsafe { bsd.assume_init() })
}

/// The executable of `pid`; `None` when the call fails.
fn path(pid: u32) -> Option<PathBuf> {
    read_path(pid).got()
}

/// `proc_pidpath` for `pid`; [`Read::Gone`] when it fails with `ESRCH`, [`Read::Failed`]
/// when it fails otherwise (an app bundle replaced under it) or answers an empty path.
fn read_path(pid: u32) -> Read<PathBuf> {
    let (Ok(pid), Ok(room)) = (
        c_int::try_from(pid),
        usize::try_from(libc::PROC_PIDPATHINFO_MAXSIZE),
    ) else {
        return Read::Failed;
    };
    let mut buffer = vec![0_u8; room];
    let Ok(size) = u32::try_from(buffer.len()) else {
        return Read::Failed;
    };
    Errno::clear();
    // SAFETY: `buffer` is a live, zeroed buffer of exactly `size` bytes
    // (`PROC_PIDPATHINFO_MAXSIZE`, the largest size the call accepts); the kernel writes a
    // NUL-terminated path of at most `size` bytes into it, and the wrapper returns its
    // `strlen`, or 0 on failure, which is refused before the buffer is read.
    let length = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), size) };
    if length == 0 {
        return failure();
    }
    let Ok(length) = usize::try_from(length) else {
        return Read::Failed;
    };
    buffer.truncate(length.min(room));
    Read::Got(PathBuf::from(OsStr::from_bytes(&buffer)))
}
