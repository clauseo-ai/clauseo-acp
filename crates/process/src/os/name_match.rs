//! Which listed processes are the ones asked for by name, for macOS's `named` (`macos.rs`), as
//! plain functions of what libproc answered (the [`Reader`] seam), so every row is tested on
//! every OS.

use std::time::SystemTime;

/// `MAXCOMLEN`: how many bytes of the executable's name the kernel keeps as `p_comm`, which
/// `PROC_PIDT_SHORTBSDINFO`'s `pbsi_comm` carries (`Microsoft Word`, 14 bytes, fits).
pub(crate) const MAXCOMLEN: usize = 16;

/// What one listed process is, for a lookup by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Its executable's file name is the name: read its start.
    Named,
    /// It is not the process asked for, or it has exited.
    Other,
    /// It runs and may be the one, but which it is cannot be read: the whole answer is unknown.
    Unknown,
}

/// What `PROC_PIDT_SHORTBSDINFO` said of a process: its `pbsi_comm` bytes before any NUL, and
/// whether it has exited and waits to be reaped (`SZOMB`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Short<'a> {
    pub(crate) comm: &'a [u8],
    pub(crate) exited: bool,
}

/// The kind of one listed process, looked up as `name`:
/// - its path read (`file`, the executable's file name): [`Kind::Named`] when it is `name`,
///   byte for byte, else [`Kind::Other`];
/// - no path (an app bundle replaced under a running process, a zombie, a race with its exit):
///   by `short`: exited is [`Kind::Other`]; a `comm` that is `name` cut to [`MAXCOMLEN`]
///   bytes is [`Kind::Unknown`] (a running Word whose path cannot be read); any other `comm` is
///   [`Kind::Other`];
/// - neither read: [`Kind::Other`] when `gone` says it has exited, else [`Kind::Unknown`].
///
/// `gone` is asked only in that last case.
pub(crate) fn kind(
    name: &str,
    file: Option<&[u8]>,
    short: Option<Short<'_>>,
    gone: impl FnOnce() -> bool,
) -> Kind {
    if let Some(file) = file {
        return if file == name.as_bytes() {
            Kind::Named
        } else {
            Kind::Other
        };
    }
    match short {
        Some(Short { exited: true, .. }) => Kind::Other,
        Some(Short { comm, .. }) => {
            let cut = name.as_bytes().get(..MAXCOMLEN).unwrap_or(name.as_bytes());
            if comm == cut {
                Kind::Unknown
            } else {
                Kind::Other
            }
        }
        None if gone() => Kind::Other,
        None => Kind::Unknown,
    }
}

/// What one per-process libproc read answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Read<T> {
    /// The call answered in full.
    Got(T),
    /// It failed with `ESRCH`: no process with the pid can be read now. It exited since the
    /// listing, whether it is gone or waits to be reaped (`proc_pidinfo` and `proc_pidpath`
    /// look a pid up with `proc_find`, which skips zombies and exiting processes, while
    /// `kill(pid, 0)` still answers a zombie), so it is skipped.
    Gone,
    /// It failed another way (`EPERM`, a short answer, an unreadable path): the process may run.
    Failed,
}

impl<T> Read<T> {
    /// The answer, if the call gave one.
    #[cfg(target_os = "macos")]
    pub(crate) fn got(self) -> Option<T> {
        match self {
            Self::Got(value) => Some(value),
            Self::Gone | Self::Failed => None,
        }
    }
}

/// What `PROC_PIDT_SHORTBSDINFO` said of a process, owned: [`Short`] borrows from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) comm: Vec<u8>,
    pub(crate) exited: bool,
}

/// What `PROC_PIDTBSDINFO` said of a match: its start, or that it has exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Start {
    Running(SystemTime),
    Exited,
}

/// The per-process reads [`starts_named`] makes: libproc on macOS, a script in the tests.
pub(crate) trait Reader {
    /// The file name of the executable of `pid` (`proc_pidpath`), empty when the path has none.
    fn file(&self, pid: u32) -> Read<Vec<u8>>;
    /// `PROC_PIDT_SHORTBSDINFO` of `pid`.
    fn short(&self, pid: u32) -> Read<Seen>;
    /// Whether no process `pid` exists now (`kill(pid, 0)` answers `ESRCH`).
    fn gone(&self, pid: u32) -> bool;
    /// `PROC_PIDTBSDINFO` of `pid`, as its start.
    fn start(&self, pid: u32) -> Read<Start>;
}

/// The starts of the processes among `pids` (`None`: the listing failed) whose executable's file
/// name is `name`, read through `reader`. Each pid is read by path; one with no path by its
/// short info; each is decided by [`kind`], and a match is read for its start:
/// - a read that answers [`Read::Gone`] skips the pid, at any step: it exited since the listing;
/// - a match whose start read fails another way skips the pid if `gone` says it is gone, else
///   makes the answer `None`, as does a [`Kind::Unknown`]: a process that may be the one runs
///   and cannot be read, so the caller cannot tell.
pub(crate) fn starts_named(
    name: &str,
    pids: Option<Vec<u32>>,
    reader: &impl Reader,
) -> Option<Vec<SystemTime>> {
    let mut starts = Vec::new();
    for pid in pids? {
        let file = match reader.file(pid) {
            Read::Got(file) => Some(file),
            Read::Gone => continue,
            Read::Failed => None,
        };
        let seen = if file.is_none() {
            match reader.short(pid) {
                Read::Got(seen) => Some(seen),
                Read::Gone => continue,
                Read::Failed => None,
            }
        } else {
            None
        };
        let short = seen.as_ref().map(|seen| Short {
            comm: &seen.comm,
            exited: seen.exited,
        });
        match kind(name, file.as_deref(), short, || reader.gone(pid)) {
            Kind::Named => {}
            Kind::Other => continue,
            Kind::Unknown => return None,
        }
        match reader.start(pid) {
            Read::Got(Start::Running(start)) => starts.push(start),
            Read::Got(Start::Exited) | Read::Gone => {}
            Read::Failed if reader.gone(pid) => {}
            Read::Failed => return None,
        }
    }
    Some(starts)
}

#[cfg(test)]
mod tests;
