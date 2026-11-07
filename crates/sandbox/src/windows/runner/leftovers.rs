//! What the pool user could leave to start at its next logon, removed (M1): its scheduled tasks,
//! its BITS jobs, the values under its `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` and
//! `RunOnce`, and every entry of its Startup folder. Run as the pool user, by the inner runner
//! right after its first-act sweep and by the detached end sweeper (`--sweep`): one code for
//! both, [`remove_all`].
//!
//! Each kind is listed, then each item deleted; one kind failing (its list, or an item) is logged
//! and the rest still run. The log carries counts and OS codes only, never a name or a content.
//! The calls are behind [`Leftovers`]; Windows' are `os.rs`.

use std::fmt;
use std::io;

use crate::windows::sids::Sid;

#[cfg(windows)]
pub(super) mod os;

/// A kind of leftover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Scheduled tasks whose principal or author is the pool user, in every folder it can read.
    Tasks,
    /// BITS jobs the pool user owns (`EnumJobs(0)`), cancelled.
    Bits,
    /// The values of `HKCU\...\Run` and `RunOnce`.
    RunKeys,
    /// The entries of the pool user's Startup folder (`FOLDERID_Startup`).
    Startup,
}

impl Kind {
    /// Every kind, in the order they are removed.
    pub(crate) const ALL: [Kind; 4] = [Kind::Tasks, Kind::Bits, Kind::RunKeys, Kind::Startup];
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Tasks => "scheduled tasks",
            Kind::Bits => "BITS jobs",
            Kind::RunKeys => "Run and RunOnce values",
            Kind::Startup => "Startup folder entries",
        })
    }
}

/// The calls a removal makes.
pub(crate) trait Leftovers {
    /// One item found; what it holds is never logged.
    type Item;
    /// Every item of `kind`; none when its place doesn't exist.
    fn list(&mut self, kind: Kind) -> io::Result<Vec<Self::Item>>;
    /// Deletes `item`.
    fn delete(&mut self, kind: Kind, item: &Self::Item) -> io::Result<()>;
}

/// Removes every kind's items, logging each kind's count, at `who` (the inner's first act, or
/// the end sweeper). Whether every kind was listed and every item deleted.
pub(crate) fn remove_all<L: Leftovers>(
    calls: &mut L,
    who: &str,
    log: &mut dyn FnMut(&str),
) -> bool {
    let mut clean = true;
    for kind in Kind::ALL {
        let items = match calls.list(kind) {
            Ok(items) => items,
            Err(error) => {
                log(&format!(
                    "clauseo-sandbox-run: cleanup ({who}): the {kind} can't be listed ({})",
                    code(&error)
                ));
                clean = false;
                continue;
            }
        };
        let mut removed = 0usize;
        let mut failed: Option<(usize, String)> = None;
        for item in &items {
            match calls.delete(kind, item) {
                Ok(()) => removed += 1,
                Err(error) => {
                    let (count, _) = failed.get_or_insert((0, code(&error)));
                    *count += 1;
                }
            }
        }
        match failed {
            None if removed == 0 => {}
            None => log(&format!(
                "clauseo-sandbox-run: cleanup ({who}): removed {removed} {kind}"
            )),
            Some((count, first)) => {
                clean = false;
                log(&format!(
                    "clauseo-sandbox-run: cleanup ({who}): removed {removed} {kind}, {count} \
                     couldn't be ({first})"
                ));
            }
        }
    }
    clean
}

/// Whether `named`, a task's principal or author as Task Scheduler gives it, is the pool user:
/// its SID, or its account name alone or after a domain or computer and a backslash, in any case.
pub(crate) fn names_user(named: &str, sid: &Sid, account: &str) -> bool {
    let named = named.trim();
    if named.eq_ignore_ascii_case(sid.as_str()) {
        return true;
    }
    let name = named.rsplit_once('\\').map_or(named, |(_, name)| name);
    !account.is_empty() && name.eq_ignore_ascii_case(account)
}

/// The names of `path` below `base`, both full local paths, compared without case: `None`
/// when `path` isn't below `base` or a name is empty, `.` or `..`.
pub(crate) fn below<'a>(path: &'a str, base: &str) -> Option<Vec<&'a str>> {
    let base = base.trim_end_matches('\\');
    let head = path.get(..base.len())?;
    let rest = path.get(base.len()..)?.strip_prefix('\\')?;
    if base.is_empty() || !head.eq_ignore_ascii_case(base) {
        return None;
    }
    let parts: Vec<&str> = rest.split('\\').collect();
    let bad = |part: &&str| part.is_empty() || *part == "." || *part == ".." || part.contains('/');
    (!parts.iter().any(bad)).then_some(parts)
}

/// An error's OS code, or its kind.
fn code(error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => code.to_string(),
        None => error.kind().to_string(),
    }
}

#[cfg(test)]
mod tests;
