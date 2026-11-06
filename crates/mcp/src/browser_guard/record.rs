//! Each run's Chrome record, `<cwd>/chrome-profile-<id>.chrome`, so the host can tell whether a
//! run's Chrome may still live (decision record 0029, "Chrome records"). The guard writes it
//! (always to a temporary name, then renamed over the record, so a reader sees one whole record):
//!
//! - `none` right after it claims the run, and again once it has killed and reaped Chrome at a
//!   stop (idle, crash, a failed start);
//! - `starting` just before it spawns Chrome;
//! - `running <pgid>` (Unix: Chrome leads its own group) or `running <pid> <creation time>`
//!   (Windows, in 100 ns units) right after the spawn, before it tells the reaper;
//!
//! and deletes it at its exit, after Chrome is reaped and before its lock is released. A reaper
//! that cleans up for a killed guard, and the next run's sweep, delete it only when it holds no
//! Chrome that may live ([`forget`]): `none`; a `running` group that is gone; `starting` (the
//! guard died between that record and the next, maybe after the spawn, so Chrome may run with
//! no pid told) once every process with the run's profile on its command line is ended
//! (`scan.rs`, Unix) or, on Windows, at once: the run's lock is free, so the guard has exited,
//! and its Job Object ended every process in it. A `running` group still there stays, and so
//! does `starting` when the listing fails.
//!
//! [`chrome_groups`] reads the records read-only. AGENTS.md warns against acting on a pid read
//! from a file, as it can name a reused process; nothing here signals or kills one. It only
//! probes, and every doubt reads as alive: a reused pgid or pid makes [`ChromeProcess::gone`]
//! say `false`, never `true`.

#[cfg(unix)]
mod scan;
#[cfg(windows)]
pub(crate) mod windows;

use std::io;
use std::path::Path;

use super::runs::{self, LOCK_SUFFIX, PROFILE_PREFIX, RECORD_SUFFIX};
use crate::write;

/// A run's Chrome, as its record says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChromeState {
    /// No Chrome now: before the first browser call, or after a stop.
    None,
    /// The guard is about to spawn Chrome, or has spawned it without recording it yet; also a
    /// record or a run whose state cannot be told. Count it as alive.
    Starting,
    /// Chrome was spawned as this process.
    Running(ChromeProcess),
}

/// The Chrome a run spawned: on Unix its process group, on Windows its process and creation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeProcess {
    /// Unix: the group's id, Chrome's pid. Windows: Chrome's pid.
    pub id: u32,
    /// Windows: the process's creation time, in 100 ns units since 1601. 0 on Unix.
    pub created: u64,
}

impl ChromeProcess {
    /// Whether this Chrome is surely gone. Unix: no process is left in its group (`kill(-pgid,
    /// 0)` fails with `ESRCH`; a group's id is not reused while any member lives, so a reused one
    /// reads as alive). Windows: no process has its pid, or the one that has started at another
    /// time; access denied, or any other failure, reads as alive.
    pub fn gone(&self) -> bool {
        #[cfg(unix)]
        {
            use nix::errno::Errno;
            use nix::sys::signal::killpg;
            use nix::unistd::Pid;
            i32::try_from(self.id)
                .is_ok_and(|raw| raw > 1 && killpg(Pid::from_raw(raw), None) == Err(Errno::ESRCH))
        }
        #[cfg(windows)]
        {
            windows::gone(self.id, self.created)
        }
    }
}

/// Each run under `cwd` whose Chrome may matter: every run that is not settled
/// (`runs::settled`'s rule: its profile is there or its lock is held), and every other run whose
/// record was left naming something but `none`. Read-only: it takes no lock but the probe
/// `runs::held` makes, and signals nothing.
pub(crate) fn chrome_groups(cwd: &Path) -> Vec<ChromeState> {
    let Ok(entries) = std::fs::read_dir(cwd) else {
        return Vec::new();
    };
    let mut ids = std::collections::BTreeSet::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(PROFILE_PREFIX)) else {
            continue;
        };
        let id = rest
            .strip_suffix(LOCK_SUFFIX)
            .or_else(|| rest.strip_suffix(RECORD_SUFFIX))
            .unwrap_or(rest);
        if runs::is_id(id) {
            ids.insert(id.to_owned());
        }
    }
    ids.iter().filter_map(|id| run_state(cwd, id)).collect()
}

/// Run `id`'s Chrome, or `None` when the run is settled and its record names nothing.
fn run_state(cwd: &Path, id: &str) -> Option<ChromeState> {
    let held = runs::held(&runs::lock_of(cwd, id));
    let profile = std::fs::symlink_metadata(runs::profile_of(cwd, id)).is_ok();
    let record = read(cwd, id);
    match record {
        // No record: a live run has not written its first yet (no Chrome before it), or its
        // guard deleted it after reaping Chrome; a free run with a profile left cannot be told.
        Err(error) if error.kind() == io::ErrorKind::NotFound => match (held, profile) {
            (true, _) => Some(ChromeState::None),
            (false, true) => Some(ChromeState::Starting),
            (false, false) => None,
        },
        Err(_) => Some(ChromeState::Starting),
        Ok(ChromeState::None) if !held && !profile => None,
        Ok(state) => Some(state),
    }
}

/// Run `id`'s record, parsed. Text that is no record reads as [`ChromeState::Starting`].
fn read(cwd: &Path, id: &str) -> io::Result<ChromeState> {
    let text = std::fs::read_to_string(runs::record_of(cwd, id))?;
    Ok(parse(&text))
}

/// `none`, `starting`, `running <id>` or `running <id> <created>`; anything else is
/// [`ChromeState::Starting`] (unknown, so alive).
pub(crate) fn parse(text: &str) -> ChromeState {
    let words: Vec<&str> = text.split_whitespace().collect();
    let number = |word: Option<&&str>| word.and_then(|word| word.parse::<u64>().ok());
    match words.as_slice() {
        ["none"] => ChromeState::None,
        ["running", ..] if (2..=3).contains(&words.len()) => {
            let id = number(words.get(1)).and_then(|id| u32::try_from(id).ok());
            let created = if words.len() == 3 {
                number(words.get(2))
            } else {
                Some(0)
            };
            match (id, created) {
                (Some(id), Some(created)) => ChromeState::Running(ChromeProcess { id, created }),
                _ => ChromeState::Starting,
            }
        }
        _ => ChromeState::Starting,
    }
}

/// The record's text for `state`.
fn text(state: &ChromeState) -> String {
    match state {
        ChromeState::None => "none".to_owned(),
        ChromeState::Starting => "starting".to_owned(),
        ChromeState::Running(process) if cfg!(windows) => {
            format!("running {} {}", process.id, process.created)
        }
        ChromeState::Running(process) => format!("running {}", process.id),
    }
}

/// Writes run `id`'s record: to `<record>.tmp`, then renamed over it. A failure is noted; the
/// host then reads the record before it, which errs towards alive only while the guard runs.
pub(crate) fn write_state(cwd: &Path, id: &str, state: &ChromeState) {
    let record = runs::record_of(cwd, id);
    let mut temporary = record.clone().into_os_string();
    temporary.push(".tmp");
    let temporary = std::path::PathBuf::from(temporary);
    let written =
        write::write(&temporary, text(state)).and_then(|()| write::replace(&temporary, &record));
    if let Err(error) = written {
        drop(write::remove_file(&temporary));
        super::note(&format!("could not write the Chrome record: {error}"));
    }
}

/// Deletes run `id`'s record: the guard's last step before its lock goes, once Chrome is reaped.
pub(crate) fn clear(cwd: &Path, id: &str) {
    match write::remove_file(&runs::record_of(cwd, id)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            super::note(&format!("could not delete the Chrome record: {error}"));
        }
        _ => {}
    }
}

/// Deletes run `id`'s record unless it may name a live Chrome: kept when it cannot be read, or
/// names a process that is not [`ChromeProcess::gone`]. One that says `starting` goes once no
/// process with the run's profile is left: on Unix after ending every one (`scan.rs`), kept
/// when the listing fails; on Windows at once. The reaper's and the sweep's step, called only
/// once the run's guard is gone (its lock free, or the reaper's pipe ended): a live guard's
/// `starting` Chrome is its own.
pub(crate) fn forget(cwd: &Path, id: &str) {
    let safe = match read(cwd, id) {
        Ok(ChromeState::None) => true,
        Ok(ChromeState::Running(process)) => process.gone(),
        Ok(ChromeState::Starting) => no_chrome_left(&runs::profile_of(cwd, id)),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    };
    if safe {
        clear(cwd, id);
    }
}

/// Unix: [`scan::end_holders`], whether no process with the run's profile is left after ending
/// each one, for a dead guard's `starting` record.
#[cfg(unix)]
use scan::end_holders as no_chrome_left;

/// Windows: no process of a dead guard's run is left, for its `starting` record: its Job Object
/// ended them all with the guard.
#[cfg(windows)]
fn no_chrome_left(_profile: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests;
