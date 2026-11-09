//! Unix: the processes that run with a run's profile, for a Chrome record left saying `starting`
//! (decision record 0029, "Chrome records"). A guard killed between that record and the next
//! may have spawned Chrome without telling anyone its pid; Chrome and every helper of it carry
//! `--user-data-dir=<profile>` on their command line (`chrome.rs`), and the profile's name is
//! the run's own (`runs.rs`), so the processes whose command line holds that argument are this
//! run's Chrome and nothing else. The reaper and the next run's sweep end them here; the record
//! goes only once none is left, and stays when the listing cannot be trusted.
//!
//! Each listing is read fresh, and a pid is signalled only while its command line names the
//! profile, so no pid read from a file is acted on (AGENTS.md). Linux reads `/proc/<pid>/cmdline`;
//! macOS reads `sysctl KERN_PROCARGS2` (`scan/macos.rs`), only its arguments (`procargs.rs`).
//! Either first reads its own process's arguments: a listing that cannot read those (a jail,
//! `hidepid`) is not trusted.
//!
//! Another user's process: macOS refuses its arguments, so the listing skips it; Linux shows them
//! in `/proc/<pid>/cmdline`, so one naming this run's profile would be found, its kill would fail
//! (`EPERM`) and [`end_holders`] would answer `false` after [`GONE_WAIT`], keeping the record.
//! Only this run's guard puts the run's own profile on a command line (`runs.rs`).

#[cfg(target_vendor = "apple")]
pub(crate) mod macos;
#[cfg(any(target_vendor = "apple", test))]
mod procargs;

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::{Pid, getpgid};

use crate::browser_guard::pause;

/// How long [`end_holders`] waits for the processes it signalled to go.
const GONE_WAIT: Duration = Duration::from_secs(5);
/// How often it lists them again meanwhile.
const LOOK: Duration = Duration::from_millis(100);

/// Ends every process whose command line holds `--user-data-dir=<profile>`, and returns whether
/// none is left: `false` when a listing fails, or one is still there after [`GONE_WAIT`].
pub(crate) fn end_holders(profile: &Path) -> bool {
    let mut flag = OsString::from("--user-data-dir=");
    flag.push(profile);
    let flag = flag.into_encoded_bytes();
    for _ in 0..=GONE_WAIT.as_millis() / LOOK.as_millis() {
        let Some(pids) = holders(&flag) else {
            return false;
        };
        if pids.is_empty() {
            return true;
        }
        for pid in pids {
            end(pid);
        }
        pause(LOOK);
    }
    false
}

/// SIGKILL to `pid`, and to its whole group when it leads one (Chrome does, `chrome.rs`). One
/// gone already is not an error.
fn end(pid: i32) {
    let pid = Pid::from_raw(pid);
    let sent = if getpgid(Some(pid)) == Ok(pid) {
        killpg(pid, Signal::SIGKILL)
    } else {
        kill(pid, Signal::SIGKILL)
    };
    // Gone already is fine: nothing is left to kill.
    if sent.is_err() {}
}

/// The running processes with argument `flag`; `None` when the listing fails.
fn holders(flag: &[u8]) -> Option<Vec<i32>> {
    let own = i32::try_from(std::process::id()).ok()?;
    let mut found = Vec::new();
    let mut reader = Reader::new()?;
    // This process's arguments first: never empty, so a listing that cannot read them is blind.
    if reader.args_of(own)?.is_empty() {
        return None;
    }
    for pid in all_pids()? {
        // Gone or a zombie (no arguments), or on macOS another user's (refused): skipped. On
        // Linux another user's arguments are read like any other (see the top of this file).
        let Some(args) = reader.args_of(pid) else {
            continue;
        };
        if pid != own && args.split(|byte| *byte == 0).any(|arg| arg == flag) {
            found.push(pid);
        }
    }
    Some(found)
}

/// Linux: every pid in `/proc`.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn all_pids() -> Option<Vec<i32>> {
    let entries = std::fs::read_dir("/proc").ok()?;
    Some(
        entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
            .filter(|pid| *pid > 1)
            .collect(),
    )
}

/// Linux: reads each process's arguments from `/proc`; it keeps nothing between them.
#[cfg(any(target_os = "linux", target_os = "android"))]
struct Reader;

#[cfg(any(target_os = "linux", target_os = "android"))]
impl Reader {
    #[expect(
        clippy::unnecessary_wraps,
        reason = "one signature with macOS, whose reader can fail to start"
    )]
    fn new() -> Option<Reader> {
        Some(Reader)
    }

    /// `pid`'s arguments, each ended by a NUL; empty for a zombie.
    #[expect(
        clippy::unused_self,
        reason = "one signature with macOS, whose reader keeps a buffer"
    )]
    fn args_of(&mut self, pid: i32) -> Option<Vec<u8>> {
        std::fs::read(format!("/proc/{pid}/cmdline")).ok()
    }
}

#[cfg(target_vendor = "apple")]
use macos::{Reader, all_pids};

/// Elsewhere no listing is known: never trusted, so a `starting` record stays.
#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn all_pids() -> Option<Vec<i32>> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
struct Reader;

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
impl Reader {
    fn new() -> Option<Reader> {
        None
    }

    fn args_of(&mut self, _pid: i32) -> Option<Vec<u8>> {
        None
    }
}
