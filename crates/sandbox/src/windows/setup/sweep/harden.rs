//! Hardening the tasks that run as the lawyer (step 4, `sweep.rs`; set-up; the pre-logon
//! check). The owner rule catches a task a pool user registered, never one it modified: an
//! administrator's task whose DACL lets Users write, with the lawyer (or a group the lawyer may
//! be in) as its principal, could have its actions rewritten by a pool user and then run as the
//! lawyer. Whether Windows' defaults ever leave such a task is not measured; the sweep closes it
//! either way.
//!
//! A task is hardened when both hold:
//! - its descriptor's owner is SYSTEM, Administrators or `TrustedInstaller`
//!   ([`HARDENED_OWNERS`]). A task the lawyer or any other user owns is never touched: its owner
//!   may change its DACL back at will, and it isn't ours to change;
//! - its principal may be the lawyer (`Groups::runs_as_lawyer`): the record's `OwnerSid`, a
//!   group that may hold the lawyer (one outside `S-1-5-21`, or a local group with the lawyer,
//!   such a group, or a group or SID of unknown kind among its members, a failed lookup
//!   counting as yes), or a name that resolves to no SID.
//!
//! Hardening takes, from each allow in its DACL that gives one of [`TASK_WRITE`] to a SID a pool
//! user's token may hold (`Groups::pool_may_hold`: Users, Authenticated Users, Everyone,
//! INTERACTIVE, LOCAL, CONSOLE LOGON, `ClauseoSandboxUsers`, a pool SID, a local group with a
//! pool member), the rights [`STRIPPED`]; an entry left with no right goes. Every other entry
//! stays byte for byte and in order, and the owner, group, SACL and control stay as read
//! ([`tightened`]). Nothing is ever added; a NULL DACL, which only an added DACL could close, is
//! left and counted as not hardened. Whether a task needs it is told from its descriptor read
//! without the SACL; only a task that does is read whole, SACL included (no write when that
//! fails), and the new descriptor goes through the task's own interface
//! (`Sweeper::set_task_security`: `IRegisteredTask::SetSecurityDescriptor`, with no entry added
//! for the principal), then is read back to check its owner, group and SACL are as read. Each
//! task changed, with the SIDs that lost which rights and its DACL before as SDDL, and each that
//! couldn't be changed or read, go to the admin-only log ([`to_admin_log`]); the world-readable
//! line gets their counts. Hardening is never 56, a failure included. It undoes nothing a pool
//! user changed during a session: pre-logon hardening comes with the run-group PR.

use std::collections::BTreeMap;

use super::group::Groups;
use super::log_path::clean;
use super::{FOLDERS_CAP, Failed, Names, Sweeper, resolve, running};
use crate::windows::setup::pool_steps::Pool;
use crate::windows::sids::Sid;

#[cfg(any(windows, test))]
mod bytes;
#[cfg(any(windows, test))]
pub use bytes::tightened;
#[cfg(any(windows, test))]
use bytes::{kept_as_read, owner_of};

/// The owners whose tasks are hardened: SYSTEM, Administrators and `TrustedInstaller`.
pub const HARDENED_OWNERS: [&str; 3] = [
    "S-1-5-18",
    "S-1-5-32-544",
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464",
];

/// The rights that let their holder rewrite a task, or its DACL or owner and then the task:
/// `FILE_WRITE_DATA`, `FILE_APPEND_DATA`, `FILE_WRITE_EA`, `FILE_WRITE_ATTRIBUTES` (the Task
/// Scheduler's write bits), `WRITE_DAC`, `WRITE_OWNER` (an owner holds `WRITE_DAC`),
/// `GENERIC_ALL` and `GENERIC_WRITE`; `FILE_ALL_ACCESS` holds the first. An allow giving any to a
/// SID a pool user may hold is tightened.
pub const TASK_WRITE: u32 =
    0x2 | 0x4 | 0x10 | 0x100 | 0x4_0000 | 0x8_0000 | 0x1000_0000 | 0x4000_0000;

/// What such an allow loses: [`TASK_WRITE`] and `DELETE`. Its other rights (read, execute,
/// `READ_CONTROL`, `SYNCHRONIZE`) stay.
pub const STRIPPED: u32 = TASK_WRITE | 0x1_0000;

/// The most tasks one run names in the admin-only log; past it, their count.
pub const HARDENED_LOGGED_CAP: usize = 100;

/// What hardening did in one run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hardened {
    /// Tasks whose descriptor was rewritten.
    pub tasks: usize,
    /// Tasks that run as the lawyer that needed it and couldn't have it: a full descriptor
    /// (with its SACL) that couldn't be read, a DACL that couldn't be parsed or is NULL, a write
    /// refused, or a read-back whose owner, group or SACL changed.
    pub failed: usize,
    /// Tasks that run as the lawyer whose descriptor (without its SACL) couldn't be read, so
    /// whether they need it can't be told.
    pub unread: usize,
    /// For [`harden_loose_tasks`] alone: tasks whose principal couldn't be read, so whether
    /// they run as the lawyer can't be told.
    pub principal_unreadable: usize,
    /// For [`harden_loose_tasks`] alone: folders whose tasks or subfolders couldn't be listed,
    /// or past [`FOLDERS_CAP`]; their tasks weren't checked.
    pub unlisted: usize,
    /// One line per task for the admin-only log: its path, cleaned, and what changed or why not.
    pub notes: Vec<String>,
}

/// One allow tightened: the SID it names (`None` where that can't be read), the rights it lost,
/// and whether it went for having none left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loosened {
    /// The SID the entry names.
    pub sid: Option<Sid>,
    /// The rights it lost.
    pub lost: u32,
    /// Whether it went.
    pub removed: bool,
}

/// What [`tightened`] gives: the new descriptor and each allow tightened, `None` when no allow
/// needed it.
pub type Tightened = Option<(Vec<u8>, Vec<Loosened>)>;

/// A task's security descriptor as `Sweeper::task_security` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSecurity {
    /// Self-relative.
    pub bytes: Vec<u8>,
    /// The SDDL the Task Scheduler gave, for the admin-only log.
    pub sddl: String,
}

/// What [`harden_task`] works with.
pub(super) struct HardenWith<'h> {
    /// The pool's SIDs.
    pub(super) pool: &'h [Sid],
    /// The record's `OwnerSid`.
    pub(super) lawyer: &'h Sid,
    /// The groups, each read once a run.
    pub(super) groups: &'h mut Groups,
    /// The counts.
    pub(super) hardened: &'h mut Hardened,
}

/// The owner of the self-relative descriptor `descriptor`; off Windows none, so nothing is
/// hardened there.
#[cfg(not(any(windows, test)))]
fn owner_of(_descriptor: &[u8]) -> Option<Sid> {
    None
}

/// `bytes::kept_as_read`'s stand-in off Windows, where no descriptor is read.
#[cfg(not(any(windows, test)))]
fn kept_as_read(_read: &[u8], _back: &[u8]) -> Result<(), String> {
    Ok(())
}

/// `bytes::tightened`'s stand-in off Windows, where no descriptor is read.
#[cfg(not(any(windows, test)))]
fn tightened(
    _descriptor: &[u8],
    _loose: &mut dyn FnMut(Option<&Sid>) -> bool,
) -> Result<Tightened, Failed> {
    Err("hardening reads a task's descriptor only on Windows".to_owned())
}

/// Hardens the task `name` in `folder`, whose principal may be the lawyer, as the module says,
/// into `at`'s counts. Whether it needs it is told from its descriptor without the SACL; only
/// then is the whole one read (no write when that fails), tightened again, written, and read
/// back to check its owner, group and SACL are as read.
pub(super) fn harden_task<S: Sweeper>(
    fs: &mut S,
    folder: &S::Folder,
    name: &str,
    at: HardenWith<'_>,
) {
    let HardenWith {
        pool,
        lawyer,
        groups,
        hardened,
    } = at;
    let path = fs.folder_path(folder).map_or_else(
        |_| format!("<a folder that can't be read>\\{name}"),
        |folder| running::task_path(&folder, name),
    );
    let path = clean(&path);
    let not = |hardened: &mut Hardened, why: &str| {
        hardened.failed += 1;
        let note = format!("couldn't harden a task that runs as the lawyer, {path}: {why}");
        hardened.notes.push(note);
    };
    let first = match fs.task_security(folder, name, false) {
        Ok(first) => first,
        Err(why) => {
            hardened.unread += 1;
            let note = format!("couldn't read a task that runs as the lawyer, {path}: {why}");
            hardened.notes.push(note);
            return;
        }
    };
    match needs(fs, groups, pool, lawyer, &first.bytes) {
        Ok(Some(_)) => {}
        Ok(None) => return,
        Err(why) => return not(hardened, &why),
    }
    let read = match fs.task_security(folder, name, true) {
        Ok(read) => read,
        Err(why) => {
            let why = format!("its SACL couldn't be read ({why}), so nothing was written");
            return not(hardened, &why);
        }
    };
    let (tightened, changes) = match needs(fs, groups, pool, lawyer, &read.bytes) {
        Ok(Some(tightened)) => tightened,
        Ok(None) => return,
        Err(why) => return not(hardened, &why),
    };
    let lost = describe(&changes);
    let before = dacl_part(&read.sddl);
    if let Err(why) = fs.set_task_security(folder, name, &tightened) {
        return not(hardened, &format!("{why}; it would have lost: {lost}"));
    }
    let checked = fs
        .task_security(folder, name, true)
        .map_err(|why| format!("it couldn't be read back ({why})"))
        .and_then(|back| kept_as_read(&read.bytes, &back.bytes));
    match checked {
        Ok(()) => {
            hardened.tasks += 1;
            let note = format!(
                "hardened a task that runs as the lawyer, {path}: {lost}; its DACL before: \
                 {before}"
            );
            hardened.notes.push(note);
        }
        Err(why) => not(
            hardened,
            &format!("written ({lost}; its DACL before: {before}), but {why}"),
        ),
    }
}

/// `descriptor` [`tightened`] when its owner is one of [`HARDENED_OWNERS`] (`Ok(None)` for
/// any other owner, or none), for the pool `pool` and the lawyer `lawyer`.
fn needs<S: Sweeper>(
    fs: &mut S,
    groups: &mut Groups,
    pool: &[Sid],
    lawyer: &Sid,
    descriptor: &[u8],
) -> Result<Tightened, Failed> {
    let owner = owner_of(descriptor);
    if !owner.is_some_and(|owner| HARDENED_OWNERS.contains(&owner.as_str())) {
        return Ok(None);
    }
    let mut loose = |sid: Option<&Sid>| groups.pool_may_hold(fs, sid, pool, lawyer);
    tightened(descriptor, &mut loose)
}

/// The DACL part of the SDDL `sddl` (`D:` up to `S:` or the end), for undoing a change by
/// hand; `sddl` whole when it has none.
pub fn dacl_part(sddl: &str) -> &str {
    let Some(start) = sddl.find("D:") else {
        return sddl;
    };
    let rest = sddl.get(start..).unwrap_or(sddl);
    let end = rest.find("S:").unwrap_or(rest.len());
    rest.get(..end).unwrap_or(rest)
}

/// `changes` for the admin-only log: `S-1-5-32-545 lost 0x40000000; S-1-1-0 lost 0x001f01ff
/// (its entry removed)`.
fn describe(changes: &[Loosened]) -> String {
    let each = changes.iter().map(|change| {
        let who = change
            .sid
            .as_ref()
            .map_or("an entry whose SID can't be read", Sid::as_str);
        let removed = if change.removed {
            " (its entry removed)"
        } else {
            ""
        };
        format!("{who} lost {:#010x}{removed}", change.lost)
    });
    each.collect::<Vec<_>>().join("; ")
}

/// Hardens every task that runs as the lawyer of `pool` (its `owner`), as the module says:
/// every folder from `\`, at most [`FOLDERS_CAP`], each task whose principal can be read. It
/// deletes nothing and is never 56; a folder that can't be listed is counted and passed over.
/// The sweep hardens the same way inside its own walk (`walk::sweep_task`), at every session's
/// end; set-up calls this once the record names the pool (`late_steps::harden_tasks`).
///
/// The pre-logon SYSTEM check (a later PR) must call it at every session's start, before the
/// pool user's logon: a task loosened since the last sweep is then closed before a session can
/// use it.
pub(crate) fn harden_loose_tasks<S: Sweeper>(fs: &mut S, pool: &Pool) -> Hardened {
    let names = Names::of(fs, &pool.users);
    let mut groups = Groups::of(fs);
    let mut resolved = BTreeMap::new();
    let mut hardened = Hardened::default();
    let Ok(root) = fs.root() else {
        hardened.unlisted += 1;
        return hardened;
    };
    let mut stack = vec![root];
    let mut folders = 0;
    while let Some(folder) = stack.pop() {
        if folders >= FOLDERS_CAP {
            hardened.unlisted += 1;
            break;
        }
        folders += 1;
        match fs.tasks(&folder) {
            Ok(tasks) => {
                for task in tasks {
                    let Ok(principal) = task.principal else {
                        hardened.principal_unreadable += 1;
                        continue;
                    };
                    if let Some(name) = principal.name() {
                        let found = resolved.get(name).cloned();
                        let found = found.unwrap_or_else(|| resolve(fs, &names, name));
                        resolved.insert(name.to_owned(), found);
                    }
                    let lawyer = &pool.owner;
                    if groups.runs_as_lawyer(fs, &principal, &resolved, &pool.users, lawyer) {
                        let at = HardenWith {
                            pool: &pool.users,
                            lawyer,
                            groups: &mut groups,
                            hardened: &mut hardened,
                        };
                        harden_task(fs, &folder, &task.name, at);
                    }
                }
            }
            Err(_) => hardened.unlisted += 1,
        }
        match fs.subfolders(&folder) {
            Ok(subs) => {
                for sub in subs {
                    match sub {
                        Ok(sub) => stack.push(sub),
                        Err(_) => hardened.unlisted += 1,
                    }
                }
            }
            Err(_) => hardened.unlisted += 1,
        }
    }
    hardened
}

/// Writes `hardened`'s notes to `fs`'s admin-only log, each a line starting `by`, at most
/// [`HARDENED_LOGGED_CAP`] and then their count, and returns what the world-readable line says
/// of them: `""` when nothing was hardened, failed or unread, else the counts alone, never a
/// path.
pub(crate) fn to_admin_log<S: Sweeper>(fs: &mut S, hardened: &Hardened, by: &str) -> String {
    let Hardened {
        tasks,
        failed,
        unread,
        principal_unreadable,
        ..
    } = *hardened;
    if tasks == 0 && failed == 0 && unread == 0 && principal_unreadable == 0 {
        return String::new();
    }
    let mut written = Ok(());
    for note in hardened.notes.iter().take(HARDENED_LOGGED_CAP) {
        written = written.and_then(|()| fs.admin_log(&format!("{by} {note}")));
    }
    let more = hardened.notes.len().saturating_sub(HARDENED_LOGGED_CAP);
    if more > 0 {
        let line = format!("{by}: {more} more tasks that run as the lawyer, not named");
        written = written.and_then(|()| fs.admin_log(&line));
    }
    let mut counts = format!(
        "; {tasks} tasks that run as the lawyer had standard users' write removed, {failed} \
         couldn't be, {unread} couldn't be read"
    );
    if principal_unreadable > 0 {
        counts = format!("{counts}, {principal_unreadable} tasks whose principal couldn't be read");
    }
    match written {
        Ok(()) => format!("{counts} (named in the admin-only log)"),
        Err(why) => format!("{counts} (not named: the admin-only log couldn't be written: {why})"),
    }
}
