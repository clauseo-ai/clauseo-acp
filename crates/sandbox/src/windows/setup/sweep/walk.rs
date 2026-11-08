//! One task of step 4's walk (`sweep.rs`): its principal resolved and, when the pool's, the
//! task deleted ([`sweep_task`]); a task the Task Scheduler won't give held to its owner and
//! its file ([`file_trust::by_file`]); a group's task counted apart ([`group`]); a task that
//! runs as the lawyer hardened ([`harden`]).

use std::collections::BTreeMap;

#[cfg(doc)]
use super::group;
use super::group::Groups;
use super::harden::{self, HardenWith};
use super::{
    Failed, Names, Resolved, Sweeper, Swept, Task, file_trust, pool_owned, resolve, running,
    untrusted,
};
use crate::windows::sids::Sid;

/// Why a task whose owner hardening may have written, and that no principal or file names the
/// pool's, is 56.
const UNTRUSTED: &str = "hardening may have written this task's owner, and nothing else \
                         names it not the pool's";

/// What one task's sweep works with.
pub(super) struct Walk<'w> {
    /// The pool's SIDs.
    pub(super) pool: &'w [Sid],
    /// The record's `OwnerSid`, for hardening ([`harden`]); `None` hardens nothing.
    pub(super) lawyer: Option<&'w Sid>,
    /// The pool's names, for a name that resolves to no SID.
    pub(super) names: &'w Names,
    /// Each name's resolution, once a run.
    pub(super) resolved: &'w mut BTreeMap<String, Resolved>,
    /// The groups, each read once a run.
    pub(super) groups: &'w mut Groups,
    /// The counts.
    pub(super) swept: &'w mut Swept,
}

/// What one task's principal or owner makes of it.
pub(super) enum Found {
    /// The pool's: deleted.
    Pool,
    /// Not the pool's: left.
    NotPool,
    /// Another domain's name that resolves to no SID: skipped.
    Skipped(Failed),
    /// A name that resolves to no SID and may be the pool's: 56.
    Unresolved(Failed),
    /// No user or group: 56.
    NoPrincipal,
    /// The owner couldn't be read, or the task file read instead couldn't be read or trusted,
    /// or names the pool or no one known: 56.
    Unread(Failed),
    /// The Task Scheduler wouldn't give it, and its trusted task file names a principal that
    /// isn't the pool's: skipped.
    FileSkipped,
}

/// One task of `folder`: deleted when its principal is the pool's (a pool SID, or a pool
/// user's name that resolves to no SID) or its owner is a pool SID, counted in `swept` either
/// way. A task the Task Scheduler won't give is held to its owner and its file
/// ([`file_trust::by_file`]). A group's task is judged alike (a pool owner deleted, an owner
/// that can't be read 56, any other ignored) and counted apart. A task left whose principal may
/// be the lawyer is hardened ([`harden::harden_task`]), never 56 unless its `Id` changed across
/// the write. A task in the record's `OwnerUntrusted` has its owner ignored ([`untrusted`]).
pub(super) fn sweep_task<S: Sweeper>(fs: &mut S, folder: &S::Folder, task: Task, at: Walk<'_>) {
    let Walk {
        pool,
        lawyer,
        names,
        resolved,
        groups,
        swept,
    } = at;
    swept.tasks += 1;
    let left_before = swept.left_tasks();
    let deleted_before = swept.deleted;
    let mut author = String::new();
    let mut lawyer_task = false;
    let (found, group) = match task.principal {
        Ok(principal) => {
            let found = match principal.name() {
                Some(name) => by_name(fs, pool, names, resolved, name),
                None => Found::NoPrincipal,
            };
            let group = groups.group_task(fs, &principal, resolved, pool);
            lawyer_task = lawyer.is_some_and(|lawyer| {
                groups.runs_as_lawyer(fs, &principal, resolved, pool, lawyer)
            });
            // The registrant writes `Author`: logged for a group task left at 56, never
            // trusted, never part of the judgment.
            author = principal.author;
            let found = if matches!(found, Found::Pool) {
                found
            } else {
                match pool_owned(fs, folder, &task.name, pool) {
                    Ok(true) => Found::Pool,
                    Ok(false) => found,
                    Err(why) => Found::Unread(why),
                }
            };
            (found, group)
        }
        Err(why) => {
            let (found, group) =
                file_trust::by_file(fs, folder, &task.name, pool, names, resolved, groups);
            if matches!(found, Found::Unread(_)) {
                swept.note(why);
            }
            (found, group)
        }
    };
    let found = distrusted(fs, folder, &task.name, found, swept);
    // Only a task left and read: a pool task is deleted, and one whose owner can't be read
    // can't be hardened either.
    let harden = lawyer_task
        && matches!(
            found,
            Found::NotPool | Found::Skipped(_) | Found::Unresolved(_)
        );
    match found {
        Found::Pool => {
            // Deleted first, so no trigger starts another instance; deleting the task ends
            // none, so its running instances are stopped once the walk and the BITS step are
            // done (`running`).
            let path = fs
                .folder_path(folder)
                .map(|folder| running::task_path(&folder, &task.name));
            match fs.delete_task(folder, &task.name) {
                Ok(()) => swept.deleted += 1,
                Err(why) => {
                    swept.not_deleted += 1;
                    swept.note(why);
                }
            }
            swept.pool_paths.push(path);
        }
        Found::NotPool => {}
        Found::Skipped(why) => {
            swept.skipped += 1;
            swept.note(why);
        }
        Found::Unresolved(why) => {
            swept.unresolved += 1;
            swept.note(why);
        }
        Found::NoPrincipal => swept.no_principal += 1,
        Found::Unread(why) => {
            swept.unreadable += 1;
            swept.note(why);
        }
        Found::FileSkipped => swept.file_skipped += 1,
    }
    if harden && let Some(lawyer) = lawyer {
        let at = HardenWith {
            pool,
            lawyer,
            groups: &mut *groups,
            hardened: &mut swept.hardened,
        };
        harden::harden_task(fs, folder, &task.name, at);
    }
    let left = swept.left_tasks() > left_before;
    if let Some(may_hold_pool) = group {
        if may_hold_pool {
            swept.group_tasks_open += 1;
        }
        if left {
            swept.group_tasks_56 += 1;
            if !author.is_empty() {
                swept.group_authors.push(author);
            }
        } else if swept.deleted > deleted_before {
            swept.group_tasks_deleted += 1;
        } else {
            swept.group_tasks_ignored += 1;
        }
    }
    if left && let Ok(folder) = fs.folder_path(folder) {
        swept
            .left_paths
            .push(running::task_path(&folder, &task.name));
    }
}

/// `found` for the task `name` in `folder`, unless hardening may have written its owner
/// ([`untrusted`]): then only its principal or its file naming the pool counts, and anything
/// else, which only an owner could pass, is 56.
fn distrusted<S: Sweeper>(
    fs: &mut S,
    folder: &S::Folder,
    name: &str,
    found: Found,
    swept: &Swept,
) -> Found {
    if matches!(found, Found::Pool) || swept.owner_untrusted.is_empty() {
        return found;
    }
    // A folder whose path can't be read is 56 already (`sweep_tasks`).
    let path = fs
        .folder_path(folder)
        .map(|at| running::task_path(&at, name));
    match path {
        Ok(path) if untrusted::distrusts(fs, &swept.owner_untrusted, &path) => {
            Found::Unread(UNTRUSTED.to_owned())
        }
        _ => found,
    }
}

/// What the principal `name` makes of a task, against `pool`; resolved once a run
/// ([`resolve`]), kept in `resolved`.
pub(super) fn by_name(
    fs: &mut impl Sweeper,
    pool: &[Sid],
    names: &Names,
    resolved: &mut BTreeMap<String, Resolved>,
    name: &str,
) -> Found {
    let found = resolved
        .entry(name.to_owned())
        .or_insert_with(|| resolve(fs, names, name))
        .clone();
    match found {
        Resolved::Sid(sid) if pool.contains(&sid) => Found::Pool,
        Resolved::Sid(_) => Found::NotPool,
        Resolved::PoolName => Found::Pool,
        Resolved::Skipped(why) => Found::Skipped(why),
        Resolved::Unresolved(why) => Found::Unresolved(why),
    }
}
