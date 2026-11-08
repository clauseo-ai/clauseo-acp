//! Step 4's cross-check (`sweep.rs`): the Task Scheduler's `GetTasks` may list only the tasks
//! the caller may read, so a pool task whose descriptor shuts SYSTEM out could be missing from
//! the walk, and the run end 0 with it left. Two other listings are read: every task file below
//! `%SystemRoot%\System32\Tasks` (walked by handle, never through a link) and every task in the
//! Task Scheduler's cache (`HKLM\…\Schedule\TaskCache\Tree`, read only). A task either holds
//! that the walk didn't list (compared exactly, then as Windows compares names,
//! [`Sweeper::same_task_path`]) is hidden, 56, unless its task file, under the fallback's trust
//! rules ([`file_trust::trusted`]), names a principal that isn't the pool's: then it is
//! skipped, counted with the other tasks skipped by their file. A listing that can't be read is
//! 56 too. Nothing is deleted through a file or the registry: a hidden task is left, and the
//! run fails closed.

use std::collections::BTreeSet;

use super::{Failed, Sweeper, Swept, Walk, file_trust};

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
    for list in [fs.task_file_paths(), fs.task_cache_paths()] {
        match list {
            Ok(paths) => found.extend(paths),
            Err(why) => {
                swept.unlisted += 1;
                swept.note(why);
            }
        }
    }
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
        match file_trust::not_pool_by_path(fs, &path, pool, names, resolved) {
            Ok(()) => swept.file_skipped += 1,
            Err(why) => hidden(swept, why),
        }
    }
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
