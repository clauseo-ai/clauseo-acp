//! Windows: the kill of a tree that spares a browser (`LaunchSpec::spare_browsers`). Codex's
//! login opens the default browser as its own child (`webbrowser` 1.2.4 reads the http
//! handler with `AssocQueryStringW`, then spawns it), so a browser that was not running starts
//! in the login's job, and `TerminateJobObject`, or the job's kill-on-close when its handle
//! drops, would end the lawyer's whole browser with it.
//!
//! So such a tree is ended process by process: every process of the job but the browsers
//! (`crate::browser`) and their children is ended with `TerminateProcess`, the job listed again
//! until nothing but those is left, and only then is the job's kill-on-close cleared, so its
//! handle closes without ending them. A job holding no browser ends as any job does. When a
//! step fails (a listing, the limit change) or the job keeps starting processes, the whole job
//! is ended: a leaked login is worse than a closed browser. A crash of Clauseo before the stop
//! still ends the browser by the kill-on-close; only a stop clears it. A child of `windows.rs`,
//! whose `unsafe_code` expectation covers its Win32 calls.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};

use windows::Win32::System::JobObjects::JOB_OBJECT_LIMIT;
use windows::Win32::UI::Shell::{ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, AssocQueryStringW};
use windows::core::{PCWSTR, PWSTR, w};

use super::{Root, Tree, set_limits, terminate};
use crate::browser::{is_browser, spared};
use crate::descendants::Proc;

/// How many times the job is listed and its processes ended before the kill gives up sparing:
/// each pass ends what the last one found, and a process can start a child before it ends.
const PASSES: usize = 8;

/// The room, in UTF-16 units, for the http handler's path: the longest Windows path.
const PATH_ROOM: usize = 32_768;

/// Ends every process of `tree`'s job but its browsers and their children, then lets the job's
/// handle close without ending those. `Some(true)` once only they are left (or nothing is);
/// `None` when the job holds no browser or a step failed: the caller then ends the whole job.
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
    let mut ended: Vec<(u32, u64)> = Vec::new();
    for _ in 0..PASSES {
        let listed = root.list()?;
        let kept = spared(&listed, browser);
        if kept.is_empty() && ended.is_empty() {
            return None;
        }
        let rest: Vec<&Proc> = listed
            .iter()
            .filter(|proc| !kept.contains(&proc.pid))
            .filter(|proc| !ended.contains(&(proc.pid, proc.start)))
            .collect();
        if rest.is_empty() {
            if !kept.is_empty() {
                set_limits(&tree.job, JOB_OBJECT_LIMIT(0)).ok()?;
            }
            *tree.spared.lock().unwrap_or_else(PoisonError::into_inner) = kept;
            return Some(true);
        }
        for proc in rest {
            // A process that is gone, or whose pid now names another, needs no end.
            drop(terminate(proc));
            ended.push((proc.pid, proc.start));
        }
    }
    None
}

/// The default http handler's executable, as `webbrowser` finds the browser it opens:
/// `AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, "http")`. `None` when Windows
/// names none.
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
    Some(PathBuf::from(OsString::from_wide(&buffer))).filter(|path| path.is_absolute())
}
