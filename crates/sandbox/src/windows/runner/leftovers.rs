//! What the pool user could leave to start at its next logon, or to run in a later runner,
//! removed (M1): its per-user COM classes, the values under its
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` and `RunOnce`, every entry of its Startup
//! folder and its scheduled tasks. Run as the pool user, by the inner runner right
//! after its first-act sweep ([`Kind::LOCAL`]: no COM in the inner) and by the detached end sweeper
//! (`--sweep`, [`Kind::ALL`]): one code for both, [`remove_all`].
//!
//! The per-user classes (`HKCU\Software\Classes\{CLSID,Interface,AppID,TypeLib}` and their
//! `Wow6432Node` twins) go first: a server registered there for a class a runner makes would run
//! for that runner with the lawyer's logon SID, and a sandbox account has no per-user class of its
//! own to keep. The tasks, the one kind reached over COM, are skipped when any class key is left.
//! Task Scheduler's class object comes from System32's `taskschd.dll` itself, with no class lookup
//! (`os/com.rs`).
//!
//! No BITS job is touched, and no BITS object made: BITS is an out-of-process server, whose
//! creation (`CoCreateInstance`) and proxies could consult per-user class, `AppID` and
//! proxy/stub entries registered after the classes' removal, and load what they name into the
//! sweeper. A pool user's BITS job can't run anyway (BITS refuses it a file, 0x800704dd), and
//! only the `SYSTEM` stop task cancels it (0049).
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
    /// The pool user's per-user COM class keys, deleted whole.
    Classes,
    /// Scheduled tasks whose principal or author is the pool user, in every folder it can read.
    Tasks,
    /// The values of `HKCU\...\Run` and `RunOnce`.
    RunKeys,
    /// The entries of the pool user's Startup folder (`FOLDERID_Startup`).
    Startup,
}

impl Kind {
    /// The end sweeper's kinds, in the order they are removed: the classes before COM starts.
    pub(crate) const ALL: [Kind; 4] = [Kind::Classes, Kind::RunKeys, Kind::Startup, Kind::Tasks];
    /// The inner runner's: none reached over COM.
    pub(crate) const LOCAL: [Kind; 3] = [Kind::Classes, Kind::RunKeys, Kind::Startup];

    /// Whether the kind is reached over COM.
    pub(crate) fn over_com(self) -> bool {
        self == Kind::Tasks
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Classes => "per-user COM class keys",
            Kind::Tasks => "scheduled tasks",
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

/// Removes the items of each of `kinds`, in order, logging each kind's count, at `who` (the
/// inner's first act, or the end sweeper). A kind reached over COM is skipped once the classes
/// weren't all removed. Whether every kind was listed and every item deleted.
pub(crate) fn remove_all<L: Leftovers>(
    calls: &mut L,
    kinds: &[Kind],
    who: &str,
    log: &mut dyn FnMut(&str),
) -> bool {
    let mut clean = true;
    let mut classes_left = false;
    for &kind in kinds {
        if kind.over_com() && classes_left {
            log(&format!(
                "clauseo-sandbox-run: cleanup ({who}): the {kind} weren't listed: per-user COM \
                 class keys are left"
            ));
            clean = false;
            continue;
        }
        let removed = remove_kind(calls, kind, who, log);
        if kind == Kind::Classes && !removed {
            classes_left = true;
        }
        clean &= removed;
    }
    clean
}

/// Removes one kind's items; whether it was listed and every item deleted.
fn remove_kind<L: Leftovers>(
    calls: &mut L,
    kind: Kind,
    who: &str,
    log: &mut dyn FnMut(&str),
) -> bool {
    let items = match calls.list(kind) {
        Ok(items) => items,
        Err(error) => {
            log(&format!(
                "clauseo-sandbox-run: cleanup ({who}): the {kind} can't be listed ({})",
                code(&error)
            ));
            return false;
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
        None if removed == 0 => true,
        None => {
            log(&format!(
                "clauseo-sandbox-run: cleanup ({who}): removed {removed} {kind}"
            ));
            true
        }
        Some((count, first)) => {
            log(&format!(
                "clauseo-sandbox-run: cleanup ({who}): removed {removed} {kind}, {count} \
                 couldn't be ({first})"
            ));
            false
        }
    }
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
