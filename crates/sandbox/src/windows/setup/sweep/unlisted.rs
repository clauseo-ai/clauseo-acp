//! Step 4's cross-check (`sweep.rs`): the Task Scheduler's `GetTasks` may list only the tasks
//! the caller may read, so a pool task whose descriptor shuts SYSTEM out could be missing from
//! the walk, and the run end 0 with it left. Two other listings are read: every task file below
//! `%SystemRoot%\System32\Tasks` (walked by handle, never through a link) and every task in the
//! Task Scheduler's cache (`HKLM\…\Schedule\TaskCache\Tree`, read only). A task either holds
//! that the walk didn't list (compared exactly, then as Windows compares names,
//! [`Sweeper::same_task_path`]) is hidden, 56, unless the owner of the descriptor its cache key
//! holds isn't a pool SID and its task file, under the fallback's trust rules
//! ([`file_trust::trusted`]), names a principal that isn't the pool's: then it is skipped,
//! counted with the other tasks skipped by their file. A listing that can't be read is 56 too.
//!
//! An orphan cache key isn't a task and can't run: Windows leaves them after feature updates and
//! uninstalls, a `Tree` key holding `SD`, `Id` and `Index` and nothing more. A path only the
//! cache holds is one, counted apart and not 56, when all of these hold: its `SD` gives an
//! owner that isn't a pool SID ([`Sweeper::task_cache_owner`]); no
//! `TaskCache\{Tasks,Plain,Logon,Boot}` key holds its `Id` ([`Sweeper::task_cache_entries`]);
//! and the file listing, read whole, has no task file at its path. Anything that can't be read
//! leaves it to the rule above, so a task hidden by stripping its `SD` (Tarrask) or owned by a
//! pool user is still 56. Nothing is deleted through a file or the registry: a hidden task or an
//! orphan is left, and the run fails closed.

use std::collections::BTreeSet;

use super::{Failed, Sweeper, Swept, Walk, file_trust};
use crate::windows::sids::Sid;

/// Holds every task the task files and the cache list against `listed`, the paths the walk
/// listed, counted in `at`'s `swept`.
pub(super) fn cross_check<S: Sweeper>(fs: &mut S, listed: &[String], at: Walk<'_>) {
    let Walk {
        pool,
        names,
        resolved,
        swept,
    } = at;
    let mut found = Vec::new();
    let mut lists = [fs.task_file_paths(), fs.task_cache_paths()].map(|list| match list {
        Ok(paths) => {
            found.extend(paths.iter().cloned());
            Some(paths)
        }
        Err(why) => {
            swept.unlisted += 1;
            swept.note(why);
            None
        }
    });
    let on_file = lists[0].take();
    let in_cache: BTreeSet<String> = lists[1].take().unwrap_or_default().into_iter().collect();
    let exact: BTreeSet<&str> = listed.iter().map(String::as_str).collect();
    let mut checked = BTreeSet::new();
    for path in found {
        if exact.contains(path.as_str()) || !checked.insert(path.clone()) {
            continue;
        }
        match listed_as(fs, listed, &path) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(why) => {
                hidden(swept, why);
                continue;
            }
        }
        if in_cache.contains(&path) && orphan(fs, &path, pool, on_file.as_deref()) {
            swept.orphans += 1;
            continue;
        }
        match file_trust::not_pool_by_path(fs, &path, pool, names, resolved) {
            Ok(()) => swept.file_skipped += 1,
            Err(why) => hidden(swept, why),
        }
    }
}

/// Whether the cache key at `path`, which the walk didn't list, is an orphan, as the module
/// says, given `on_file`, the file listing (`None` when it couldn't be read); `false` when
/// anything that would show it can't be read.
fn orphan<S: Sweeper>(fs: &mut S, path: &str, pool: &[Sid], on_file: Option<&[String]>) -> bool {
    let Some(on_file) = on_file else {
        return false;
    };
    let not_pool = matches!(fs.task_cache_owner(path), Ok(owner) if !pool.contains(&owner));
    not_pool
        && !on_file.iter().any(|file| file == path)
        && matches!(listed_as(fs, on_file, path), Ok(false))
        && matches!(fs.task_cache_entries(path), Ok(false))
}

/// Whether `path` is one of `listed` as Windows compares names; `Err` when a comparison
/// fails, since it may not be.
fn listed_as<S: Sweeper>(fs: &mut S, listed: &[String], path: &str) -> Result<bool, Failed> {
    for one in listed {
        if fs.same_task_path(one, path)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// One task the walk didn't list and its file didn't show not the pool's, because of `why`.
fn hidden(swept: &mut Swept, why: Failed) {
    swept.hidden += 1;
    swept.note(why);
}
