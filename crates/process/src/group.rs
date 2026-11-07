//! A process group told apart from a later one with the same id, across a restart of this
//! process (Unix): what `host` records at each agent launch and checks at its next start, to
//! end an agent's group a crashed desktop left behind (decision record 0051). A pid is unique
//! only with its start, and a start only within one boot, so a record holds the group's id, its
//! leader's [`StartStamp`], its [`session_id`] and the [`boot_stamp`]. [`presence`] says
//! whether the leader exists and with which start, [`group_members`] lists who is in a group
//! now, each with its start and session; [`kill_group`] and [`kill_member`] end them.
//!
//! Windows has none of this: the agent's Job Object ends its tree when the desktop's process
//! dies, whatever way it dies (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`).

use std::fmt;

use crate::os;

/// When a process started, as the OS stamps it (Linux: `/proc/<pid>/stat` field 22,
/// `starttime`, in clock ticks since boot; macOS: `proc_pidinfo(PROC_PIDTBSDINFO)`'s
/// `pbi_start_tvsec` and `pbi_start_tvusec` as microseconds). With its pid it names one process
/// within one boot: a reused pid has a later start. Stamps compare only with stamps read on the
/// same machine in the same boot. [`StartStamp::encode`] and [`StartStamp::decode`] carry it
/// through a file as an opaque string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StartStamp(u64);

impl StartStamp {
    /// A stamp from the OS's raw start (the unit of [`StartStamp`]'s own doc).
    pub(crate) fn from_raw(start: u64) -> StartStamp {
        StartStamp(start)
    }

    /// The stamp as text, for a record; [`StartStamp::decode`] reads it back.
    pub fn encode(self) -> String {
        self.0.to_string()
    }

    /// A stamp [`StartStamp::encode`] wrote; `None` for any other text.
    pub fn decode(text: &str) -> Option<StartStamp> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        text.parse().ok().map(StartStamp)
    }
}

impl fmt::Display for StartStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One running member of a process group, with its start and session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    /// Its pid.
    pub pid: u32,
    /// Its start.
    pub start: StartStamp,
    /// Its session id.
    pub session: u32,
}

/// Whether a process with a given pid exists ([`presence`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// No process has the pid: `kill(pid, 0)` answers `ESRCH`.
    Gone,
    /// A process has it (running, a zombie, or another user's), with its start when that could
    /// be read (on Linux a zombie's too; never another user's on macOS).
    There(Option<StartStamp>),
}

/// Whether process `pid` exists, and its start when readable. The start is read first, then
/// existence, so a process that exits between the two reads as gone, never as there with
/// another process's start.
pub fn presence(pid: u32) -> Presence {
    let start = os::start_any(pid).map(StartStamp);
    match start {
        Some(start) => Presence::There(Some(start)),
        None if os::exists(pid) => Presence::There(None),
        None => Presence::Gone,
    }
}

/// The session id of process `pid` (Linux: `/proc/<pid>/stat` field 6; macOS: `getsid`);
/// `None` when it cannot be read, or on another Unix.
pub fn session_id(pid: u32) -> Option<u32> {
    os::session_of(pid)
}

/// The start of running process `pid`; `None` when it is gone, has exited (a zombie), cannot be
/// read, or on another Unix. One small read (Linux: one `/proc` file; macOS: one libproc call).
pub fn start_stamp(pid: u32) -> Option<StartStamp> {
    os::start_of(pid).map(StartStamp)
}

/// This boot's stamp, as opaque text that differs from one boot to the next (Linux:
/// `/proc/sys/kernel/random/boot_id`; macOS: `sysctl kern.boottime`). `None` when it cannot be
/// read, or on another Unix.
pub fn boot_stamp() -> Option<String> {
    os::boot()
}

/// The running members of process group `pgid` (a zombie is none), each with its start and
/// session; an empty list when none runs. `None` when the listing failed or may be incomplete,
/// or on another Unix. Blocking: on
/// Linux it reads `/proc/<pid>/stat` of every process (field 5, `pgrp`); on macOS it is
/// `proc_listpids(PROC_PGRP_ONLY, pgid)` and one read per member.
pub fn group_members(pgid: u32) -> Option<Vec<Member>> {
    Some(
        os::members(pgid)?
            .into_iter()
            .map(|(pid, start, session)| Member {
                pid,
                start: StartStamp(start),
                session,
            })
            .collect(),
    )
}

/// SIGKILL to every process in group `pgid` (`killpg`). The caller must have proved the group
/// is the one it means (its leader's start, or members that can only be its own). Refused,
/// with nothing sent, for group 0 or 1 and for this process's own group. Returns whether the
/// signal went: `false` for an empty group too.
pub fn kill_group(pgid: u32) -> bool {
    os::kill_group(pgid)
}

/// SIGKILL to `member` if its pid still runs with its start, read again just before; never pid
/// 0 or 1, this process, or the leader of this process's own group. Returns whether it went.
pub fn kill_member(member: &Member) -> bool {
    os::kill_started(member.pid, member.start.0)
}

#[cfg(test)]
mod tests;
