//! Windows: the kill of a tree that spares a browser (`LaunchSpec::spare_browsers`). Codex's
//! login opens the default browser as its own child (`webbrowser` 1.2.4 reads the http
//! handler with `AssocQueryStringW`, then spawns it), so a browser that was not running starts
//! in the login's job, and `TerminateJobObject`, or the job's kill-on-close when its handle
//! drops, would end the lawyer's whole browser with it.
//!
//! So such a tree is ended process by process. Every process of the job but the browsers
//! (`crate::browser`: never the run's own process) and their children is ended with
//! `TerminateProcess`, by a handle that is kept; the job is listed again until nothing else is
//! left; each ended process is awaited, with a bound, so none can start a child afterwards; the
//! job is listed once more; and only then is the job's kill-on-close cleared, so its handle
//! closes without ending the browsers. The job's count of every process it has held is read
//! before that last listing and after the limit change: a process that joined in between puts
//! kill-on-close back and the whole job is ended. So is the job when any step fails: a process
//! that runs but cannot be opened, an end not seen inside the bound, a failed listing or limit
//! change, or a job that keeps starting processes. A leaked login is worse than a closed
//! browser. A job holding no browser ends as any job does. A crash of Clauseo before the stop
//! still ends the browser by the kill-on-close; only a stop clears it. A child of `windows.rs`,
//! whose `unsafe_code` expectation covers its Win32 calls.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::OwnedHandle;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};

use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::System::JobObjects::{JOB_OBJECT_LIMIT, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};
use windows::Win32::System::Threading::{
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, AssocQueryStringW};
use windows::core::{PCWSTR, PWSTR, w};

use super::{Root, Tree, alive, created, now, open, raw, set_limits, terminate, total};
use crate::browser::{Key, is_browser, spared, usable_default};
use crate::descendants::Proc;

/// How many times the job is listed and its processes ended before the kill gives up sparing:
/// each pass ends what the last one found, and a process can start a child before it ends.
const PASSES: usize = 8;

/// The room, in UTF-16 units, for the http handler's path: the longest Windows path.
const PATH_ROOM: usize = 32_768;

/// How long the ended processes may take, all together, to exit before the kill gives up
/// sparing, in milliseconds: well inside the lawyer's 2 s grace and the quit's 3 s bound.
const END_WAIT_MS: u32 = 1_500;

#[cfg(test)]
thread_local! {
    /// Tests: no process can be ended, as when one refuses `PROCESS_TERMINATE`.
    pub(super) static REFUSE_END: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Tests: runs after the last listing, before the limit change (a process joining then).
    pub(super) static BEFORE_LIMITS: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Ends every process of `tree`'s job but its browsers and their children, then lets the job's
/// handle close without ending those. `Some(true)` once only they are left; `None` when the job
/// holds no browser or a step failed: the caller then ends the whole job.
pub(super) fn kill_sparing(tree: &Tree) -> Option<bool> {
    // Pid 0 is never in a job, so the listing leaves out no process of it.
    let root = Root {
        job: Arc::downgrade(&tree.job),
        pid: 0,
    };
    let default = default_browser();
    let browser = |proc: &Proc| {
        proc.exe
            .as_deref()
            .is_some_and(|exe| is_browser(exe, default.as_deref()))
    };
    let mut ended: Vec<(Key, OwnedHandle)> = Vec::new();
    for _ in 0..PASSES {
        let listed = root.list()?;
        let kept = spared(&listed, tree.root, browser);
        if kept.is_empty() && ended.is_empty() {
            return None;
        }
        let rest: Vec<&Proc> = listed
            .iter()
            .filter(|proc| !kept.contains(&(proc.pid, proc.start)))
            .filter(|proc| !ended.iter().any(|(key, _)| *key == (proc.pid, proc.start)))
            .collect();
        if rest.is_empty() {
            return settle(tree, &root, &ended, browser);
        }
        for proc in rest {
            match end(proc) {
                Ended::Ending(handle) => ended.push(((proc.pid, proc.start), handle)),
                Ended::Gone => {}
                Ended::Refused => return None,
            }
        }
    }
    None
}

/// How [`end`] left a process.
enum Ended {
    /// Its end was asked: the handle, which can wait for it.
    Ending(OwnedHandle),
    /// It had already exited (or its pid names another process now).
    Gone,
    /// It runs and could not be opened to end it.
    Refused,
}

/// Asks `proc`'s end through a handle that can also wait for it.
fn end(proc: &Proc) -> Ended {
    #[cfg(test)]
    if REFUSE_END.with(std::cell::Cell::get) {
        return Ended::Refused;
    }
    if let Some(handle) = terminate(proc) {
        return Ended::Ending(handle);
    }
    let still = open(
        proc.pid,
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
    )
    .is_some_and(|handle| alive(&handle) && created(&handle) == Some(proc.start));
    if still { Ended::Refused } else { Ended::Gone }
}

/// The last steps once only browsers are listed: every ended process awaited (bounded), one
/// more listing, the limit cleared, and the job's total checked across both.
fn settle(
    tree: &Tree,
    root: &Root,
    ended: &[(Key, OwnedHandle)],
    browser: impl Fn(&Proc) -> bool,
) -> Option<bool> {
    if !all_exit(ended) {
        return None;
    }
    let before = total(&tree.job)?;
    let listed = root.list()?;
    let kept = spared(&listed, tree.root, browser);
    if kept.is_empty() || listed.len() != kept.len() {
        // Something to end appeared, or no browser is left: the whole job ends as any does.
        return None;
    }
    #[cfg(test)]
    if let Some(hook) = BEFORE_LIMITS.with(|hook| hook.borrow_mut().take()) {
        hook();
    }
    set_limits(&tree.job, JOB_OBJECT_LIMIT(0)).ok()?;
    if total(&tree.job) != Some(before) {
        // A process joined after the listing: the job kills on close again, and ends whole.
        drop(set_limits(&tree.job, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE));
        return None;
    }
    *tree.spared.lock().unwrap_or_else(PoisonError::into_inner) = kept;
    Some(true)
}

/// Whether every process in `ended` exits inside [`END_WAIT_MS`], all together.
fn all_exit(ended: &[(Key, OwnedHandle)]) -> bool {
    let Some(start) = now() else {
        return false;
    };
    ended.iter().all(|(_, handle)| {
        // 100 ns units since 1601, so 10 000 make a millisecond.
        let spent = now().map_or(u64::MAX, |at| at.saturating_sub(start) / 10_000);
        let left = u64::from(END_WAIT_MS).saturating_sub(spent);
        let left = u32::try_from(left).unwrap_or(0);
        // SAFETY: `handle` is a live process handle opened with PROCESS_SYNCHRONIZE.
        let waited = unsafe { WaitForSingleObject(raw(handle), left) };
        waited == WAIT_OBJECT_0
    })
}

/// The default http handler's executable, as `webbrowser` finds the browser it opens:
/// `AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, "http")`. `None` when Windows
/// names none, or names no browser ([`usable_default`]): then only the browser names count.
fn default_browser() -> Option<PathBuf> {
    let mut buffer = vec![0_u16; PATH_ROOM];
    let mut size = u32::try_from(buffer.len()).ok()?;
    // SAFETY: `buffer` is a live buffer of `size` UTF-16 units; the call writes at most that
    // many, NUL included, and sets `size`. The two strings are static and NUL-terminated (or
    // null, which the call takes for "no extra").
    let found = unsafe {
        AssocQueryStringW(
            ASSOCF_IS_PROTOCOL,
            ASSOCSTR_EXECUTABLE,
            w!("http"),
            PCWSTR::null(),
            Some(PWSTR(buffer.as_mut_ptr())),
            &raw mut size,
        )
    };
    if found.is_err() {
        return None;
    }
    let length = buffer.iter().position(|&unit| unit == 0)?;
    buffer.truncate(length);
    Some(PathBuf::from(OsString::from_wide(&buffer)))
        .filter(|path| path.is_absolute() && usable_default(path))
}

#[cfg(test)]
mod tests;
