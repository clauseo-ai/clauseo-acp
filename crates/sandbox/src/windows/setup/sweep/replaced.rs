//! A task replaced under its name after step 4's walk judged it (`sweep.rs`). The walk and the
//! cross-check (`unlisted.rs`) hold tasks by path, so a pool user holding `DELETE` on a task
//! could delete it once judged and register its own under the name, and the sweep end 0 with
//! the new one left. So each task's `TaskCache\Tree` `Id` is read before it is judged
//! ([`record`]), and the walk's paths are held by path and `Id` ([`recheck`]): the Task
//! Scheduler assigns the `Id`, the task's registrant can't set it, and a task re-created under a
//! name gets a new one (the elevated test `elevated_a_re_created_task_gets_a_new_cache_id`).
//!
//! A path whose `Id` reads another at the end is judged again in the same sweep
//! ([`again`]: the task now under the name, as the walk judges, deleted when the pool's). One
//! that can't be is 56: an `Id` that couldn't be read then or before, a folder or list that
//! couldn't be read, a task the listing hides that the cache still holds, or a path replaced
//! once more after it was judged again. A path gone is no task. Hardening's swap before its
//! write is judged again alike (`walk::sweep_task`); one across its write is 56 already, its
//! owner distrusted from the next sweep (`untrusted.rs`), and isn't judged again here, since
//! our own write may have made the pool's task look an administrator's.

use super::{Failed, Sweeper, Walk, running, sweep_task};

/// Why a path whose `Id` changed again once judged again is 56.
const AGAIN: &str = "another task took the name of a task once more after it was judged again";

/// Why a replaced task the listing hides, but the cache holds, is 56.
const HIDDEN: &str = "another task took the name of a task, and the listing hides it";

/// Records the `Id` of the task `name` in `folder` in `at`'s `swept`, before it is judged. A
/// folder whose path can't be read is 56 already (`sweep_tasks`).
pub(super) fn record<S: Sweeper>(fs: &mut S, folder: &S::Folder, name: &str, at: &mut Walk<'_>) {
    if let Ok(folder) = fs.folder_path(folder) {
        let path = running::task_path(&folder, name);
        let id = fs.task_cache_id(&path);
        at.swept.walked_ids.insert(path, id);
    }
}

/// Holds each path the walk judged to the `Id` it read then, as the module says, into `at`'s
/// `swept`.
pub(super) fn recheck<S: Sweeper>(fs: &mut S, at: Walk<'_>) {
    let mut at = at;
    let walked: Vec<_> = at.swept.walked_ids.clone().into_iter().collect();
    for (path, then) in walked {
        let swapped = at.swept.hardened.swapped_after.iter();
        if swapped.clone().any(|(across, _)| *across == path) {
            continue;
        }
        let now = fs.task_cache_id(&path);
        match (then, now) {
            (_, Ok(None)) => {}
            (Ok(Some(then)), Ok(Some(now))) if then == now => {}
            (Err(why), _) | (_, Err(why)) => unjudged(&mut at, path, why),
            (Ok(_), Ok(Some(_))) => again(fs, path, reborrow(&mut at)),
        }
    }
}

/// Judges the task now at `path` (`\A\name`) again, as the module says: once a run, 56 when
/// it can't be or was judged again already.
pub(super) fn again<S: Sweeper>(fs: &mut S, path: String, at: Walk<'_>) {
    let mut at = at;
    if at.swept.rejudged.contains(&path) {
        return unjudged(&mut at, path, AGAIN.to_owned());
    }
    at.swept.rejudged.push(path.clone());
    if let Err(why) = judge(fs, &path, reborrow(&mut at)) {
        unjudged(&mut at, path, why);
    }
}

/// The task at `path` found by its folder's list and judged ([`sweep_task`]); `Ok` too when it
/// is gone, by its listing and its cache key both.
fn judge<S: Sweeper>(fs: &mut S, path: &str, at: Walk<'_>) -> Result<(), Failed> {
    let (folder, name) = path
        .rsplit_once('\\')
        .ok_or_else(|| "the task's path doesn't start at the root".to_owned())?;
    let folder_path = if folder.is_empty() { "\\" } else { folder };
    let folder = fs.folder_at(folder_path)?;
    match fs
        .tasks(&folder)?
        .into_iter()
        .find(|task| task.name == name)
    {
        Some(task) => {
            sweep_task(fs, &folder, task, at);
            Ok(())
        }
        None if fs.task_cache_key_present(path)? => Err(HIDDEN.to_owned()),
        None => Ok(()),
    }
}

/// `path` counted as not judged again, for `why`: 56, its path in the admin-only log.
fn unjudged(at: &mut Walk<'_>, path: String, why: Failed) {
    at.swept.unjudged += 1;
    at.swept.left_paths.push(path);
    at.swept.note(why);
}

/// `at` again, for one call.
fn reborrow<'a>(at: &'a mut Walk<'_>) -> Walk<'a> {
    Walk {
        pool: at.pool,
        lawyer: at.lawyer,
        names: at.names,
        resolved: &mut *at.resolved,
        groups: &mut *at.groups,
        swept: &mut *at.swept,
    }
}
