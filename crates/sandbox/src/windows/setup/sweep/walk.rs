//! One task of step 4's walk (`sweep.rs`): its principal resolved and, when the pool's, the
//! task deleted ([`sweep_task`]); a task the Task Scheduler won't give held to its owner and
//! its file ([`file_trust::by_file`]); a group's task counted apart ([`group`]).

use std::collections::BTreeMap;

#[cfg(doc)]
use super::group;
use super::group::Groups;
use super::{
    Failed, Names, Resolved, Sweeper, Swept, Task, file_trust, pool_owned, resolve, running,
};
use crate::windows::sids::Sid;

/// What one task's sweep works with.
pub(super) struct Walk<'w> {
    /// The pool's SIDs.
    pub(super) pool: &'w [Sid],
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
/// that can't be read 56, any other ignored) and counted apart.
pub(super) fn sweep_task<S: Sweeper>(fs: &mut S, folder: &S::Folder, task: Task, at: Walk<'_>) {
    let Walk {
        pool,
        names,
        resolved,
        groups,
        swept,
    } = at;
    swept.tasks += 1;
    let left_before = swept.left_tasks();
    let deleted_before = swept.deleted;
    let mut author = String::new();
    let (found, group) = match task.principal {
        Ok(principal) => {
            let found = match principal.name() {
                Some(name) => by_name(fs, pool, names, resolved, name),
                None => Found::NoPrincipal,
            };
            let group = groups.group_task(fs, &principal, resolved, pool);
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
