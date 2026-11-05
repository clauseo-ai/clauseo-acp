//! The Linux listing of an agent's descendants: `/proc/<pid>/stat` read for every process, the
//! tree walked by parent pid. Safe std only: file reads and `read_link`. The parent-death
//! signal is in `linux/death.rs`.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::descendants::Proc;

mod death;

pub(super) use death::end_with_parent;

/// What one `/proc/<pid>/stat` line says about its process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Stat {
    pub(super) parent: u32,
    pub(super) group: u32,
    /// Field 22, `starttime`: clock ticks from boot to the process's start. With the pid it
    /// names one process: a reused pid has a later start.
    pub(super) start: u64,
    /// State `Z` (zombie) or `X` (dead): it has exited, and is only waiting to be reaped.
    pub(super) exited: bool,
}

/// Reads process `pid`'s stat line, or `None` when it is gone or unreadable.
pub(super) fn stat(pid: u32) -> Option<Stat> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_stat(&text)
}

/// Parses a stat line: `pid (comm) state ppid pgrp session ... starttime ...`. The command name
/// may hold spaces and parentheses, so the fields are read after its last `)`; the first field
/// after it is field 3 (`state`).
pub(super) fn parse_stat(text: &str) -> Option<Stat> {
    let (_, rest) = text.rsplit_once(')')?;
    let fields: Vec<&str> = rest.split_ascii_whitespace().collect();
    let state = *fields.first()?;
    Some(Stat {
        parent: fields.get(1)?.parse().ok()?,
        group: fields.get(2)?.parse().ok()?,
        start: fields.get(22 - 3)?.parse().ok()?,
        exited: matches!(state, "Z" | "X"),
    })
}

/// Now, as clock ticks since boot, rounded down: `CLOCK_BOOTTIME` (the clock a process's
/// `start_boottime` is read from) in `_SC_CLK_TCK` units, the unit of field 22 of
/// `/proc/<pid>/stat` (`nsec_to_clock_t` rounds a start down the same way). `None` when either
/// call fails.
pub(super) fn now() -> Option<u64> {
    let hz = nix::unistd::sysconf(nix::unistd::SysconfVar::CLK_TCK).ok()??;
    let hz = u64::try_from(hz).ok().filter(|&hz| hz > 0)?;
    let time = nix::time::clock_gettime(nix::time::ClockId::CLOCK_BOOTTIME).ok()?;
    let seconds = u64::try_from(time.tv_sec()).ok()?;
    let nanos = u64::try_from(time.tv_nsec()).ok()?;
    Some(seconds.checked_mul(hz)? + nanos * hz / 1_000_000_000)
}

/// The start of a running process `pid`: `None` when it is gone, unreadable, or has exited.
pub(super) fn start_of(pid: u32) -> Option<u64> {
    stat(pid).filter(|stat| !stat.exited).map(|stat| stat.start)
}

/// Every running descendant of `root`, whose start must still be `root_start`; `None` when the
/// root is gone (or its pid now names another process) or `/proc` cannot be read.
pub(super) fn list(root: u32, root_start: u64) -> Option<Vec<Proc>> {
    if start_of(root) != Some(root_start) {
        return None;
    }
    let mut children: HashMap<u32, Vec<(u32, Stat)>> = HashMap::new();
    for entry in fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        // A process that exits between the directory read and the stat read is skipped.
        if let Some(stat) = stat(pid).filter(|stat| !stat.exited) {
            children.entry(stat.parent).or_default().push((pid, stat));
        }
    }
    let mut found = Vec::new();
    let mut next = vec![root];
    while let Some(parent) = next.pop() {
        for (pid, stat) in children.remove(&parent).unwrap_or_default() {
            next.push(pid);
            found.push(Proc {
                pid,
                parent: Some(parent),
                start: stat.start,
                leads_group: stat.group == pid,
                exe: exe(pid),
            });
        }
    }
    Some(found)
}

/// The executable process `pid` runs, as the kernel resolved it; `None` when unreadable (a
/// process of another user, a kernel thread, or one that just exited).
fn exe(pid: u32) -> Option<PathBuf> {
    fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[cfg(test)]
mod tests;
