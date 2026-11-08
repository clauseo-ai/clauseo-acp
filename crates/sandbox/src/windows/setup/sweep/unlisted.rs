//! Step 4's cross-check (`sweep.rs`): the Task Scheduler's `GetTasks` may list only the tasks
//! the caller may read, so a pool task whose descriptor shuts SYSTEM out could be missing from
//! the walk, and the run end 0 with it left. Two other listings are read: every task file below
//! `%SystemRoot%\System32\Tasks` (walked by handle, never through a link) and every task in the
//! Task Scheduler's cache (`HKLM\…\Schedule\TaskCache\Tree`, read only). Paths are compared
//! exactly, then as Windows compares names ([`Sweeper::same_task_path`]).
//!
//! When the three disagree (either listing holds a task the walk didn't list), or a listing
//! can't be read or had a folder or key vanish while it was read, all three are read again
//! once, the Task Scheduler's by a walk that lists ([`listed_again`]), and only that second
//! read is judged: a task made or deleted mid-run then agrees. A task the second walk lists
//! that the first didn't is no proof of safety: it is held to the first walk's principal check
//! ([`super::sweep_task`]), deleted when the pool's, and a pool task arrived mid-run is 56. A
//! walk that couldn't list a folder is 56 already and isn't repeated: the first read is judged.
//! In it, a task the walk didn't list is:
//!
//! - in a task file and the cache: hidden, 56, unless the owner of the descriptor its cache key
//!   holds is readable and isn't a pool SID, and its task file, under the fallback's trust
//!   rules ([`file_trust::trusted`]), names a principal that isn't the pool's: then it is
//!   skipped, counted with the tasks skipped by their file. An owner that can't be read
//!   (unreadable, ownerless, over its cap) is 56 whatever the file says: [`file_trust::trusted`]
//!   reads today's DACL, and a registrant with write and `WRITE_DAC` on its file can name SYSTEM
//!   there, then drop its own write. Security wins; the planned recycle restores availability;
//! - in a task file alone (no cache key, so the Task Scheduler holds no such task): the file's
//!   own owner, read through a handle that never follows a link
//!   ([`Sweeper::task_file_owner`]), decides. A pool SID, or an owner that can't be read, is
//!   56; any other (Administrators, SYSTEM, another user) leaves a stray file, ignored;
//! - in the cache alone (no task file): whether the task can run decides
//!   ([`Sweeper::task_cache_runnable`]). A key whose `Id` names no `TaskCache\Tasks\{Id}` key
//!   holds no task the Task Scheduler can run, a stale key, ignored; one that can run, or can't
//!   be told, is 56. Its `SD`'s owner is never trusted: the `SD` holds the descriptor the task's
//!   registrant gave, and a pool user may delete its own task's file. The principal in
//!   `Tasks\{Id}`'s binary values isn't read either: their format is undocumented.
//!
//! A folder or key that vanishes while listed is left out of its listing, never failing it,
//! and checked once more after the judged read: one there again is 56 ([`Swept::reappeared`]);
//! only one still gone is skipped and counted. A listing that can't be read on the judged read
//! is 56. The log line counts each kind; the paths of the tasks that keep it at 56 go to the
//! admin-only log alone ([`super::log_path`]). Nothing is deleted through a file or the
//! registry: a hidden task is left, and the run fails closed.
//!
//! [`Swept::reappeared`]: super::Swept::reappeared

use std::collections::BTreeSet;

use super::{FOLDERS_CAP, Failed, Sweeper, Walk, file_trust, running, sweep_task, task_file};
use crate::windows::sids::Sid;

/// One of the cross-check's listings ([`Sweeper::task_file_paths`],
/// [`Sweeper::task_cache_paths`]): every task's path (`\A\name`), and the folders or keys that
/// vanished while it was read, each left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// Every task's path.
    pub paths: Vec<String>,
    /// Each folder (below `Tasks`, as `\A\B`) or key (below `Tree`, as `\A\name`) gone while
    /// listed, by path: never logged.
    pub vanished: Vec<String>,
}

/// Where a task the walk didn't list was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    /// In a task file and the cache.
    Both,
    /// In a task file alone.
    File,
    /// In the cache alone.
    Key,
}

/// One read of the three listings, held against each other.
#[derive(Default)]
struct Views {
    /// Why each listing that couldn't be read wasn't.
    failed: Vec<Failed>,
    /// Folders below `Tasks`, and keys below `Tree`, gone while listed.
    vanished_folders: Vec<String>,
    vanished_keys: Vec<String>,
    /// Each task the walk didn't list, its path and where it was found; `Err` where a
    /// comparison failed, since it may be one.
    unlisted: Vec<Result<(String, Held), Failed>>,
}

impl Views {
    /// Whether the listings agree: each read whole, and no task the walk didn't list.
    fn agree(&self) -> bool {
        self.failed.is_empty()
            && self.vanished_folders.is_empty()
            && self.vanished_keys.is_empty()
            && self.unlisted.is_empty()
    }
}

/// Holds every task the task files and the cache list against `listed`, the paths the walk
/// listed, as the module says, counted in `at`'s `swept`.
pub(super) fn cross_check<S: Sweeper>(fs: &mut S, listed: &[String], at: Walk<'_>) {
    let first = read_views(fs, listed);
    if first.agree() {
        return;
    }
    // A walk that couldn't list everything is 56 already, and a second walk would fail alike:
    // the first read is judged.
    if at.swept.unlisted > 0 {
        judge(fs, first, at);
        return;
    }
    at.swept.relisted = true;
    let mut at = at;
    match listed_again(fs, listed, &mut at) {
        Ok(again) => {
            let second = read_views(fs, &again);
            judge(fs, second, at);
        }
        Err(why) => {
            at.swept.unlisted += 1;
            at.swept.note(why);
        }
    }
}

/// Every task the Task Scheduler lists, by path, from a second walk; each task it lists that
/// isn't one of `listed`, the first walk's (or can't be compared), is held to the first walk's
/// check ([`super::sweep_task`]), and one found the pool's counts in
/// [`super::Swept::arrived_pool`]: 56. `Err` when a folder, its path, its tasks or its
/// subfolders can't be read, or past [`FOLDERS_CAP`].
fn listed_again<S: Sweeper>(
    fs: &mut S,
    listed: &[String],
    at: &mut Walk<'_>,
) -> Result<Vec<String>, Failed> {
    let first: BTreeSet<&str> = listed.iter().map(String::as_str).collect();
    let mut stack = vec![fs.root()?];
    let mut folders = 0_usize;
    let mut again = Vec::new();
    while let Some(folder) = stack.pop() {
        folders += 1;
        if folders > FOLDERS_CAP {
            return Err(format!("more than {FOLDERS_CAP} folders, read again"));
        }
        let path = fs.folder_path(&folder)?;
        for task in fs.tasks(&folder)? {
            let at_path = running::task_path(&path, &task.name);
            let known = first.contains(at_path.as_str()) || holds(fs, listed, &at_path) == Ok(true);
            again.push(at_path.clone());
            if known {
                continue;
            }
            at.swept.arrived += 1;
            let pool_before = at.swept.pool_paths.len();
            let one = Walk {
                pool: at.pool,
                names: at.names,
                resolved: &mut *at.resolved,
                swept: &mut *at.swept,
            };
            sweep_task(fs, &folder, task, one);
            if at.swept.pool_paths.len() > pool_before {
                at.swept.arrived_pool += 1;
                at.swept.left_paths.push(at_path);
            }
        }
        for sub in fs.subfolders(&folder)? {
            stack.push(sub?);
        }
    }
    Ok(again)
}

/// The task files and the cache, read and held against `listed`.
fn read_views<S: Sweeper>(fs: &mut S, listed: &[String]) -> Views {
    let mut views = Views::default();
    let files = match fs.task_file_paths() {
        Ok(list) => {
            views.vanished_folders = list.vanished;
            list.paths
        }
        Err(why) => {
            views.failed.push(why);
            Vec::new()
        }
    };
    let keys = match fs.task_cache_paths() {
        Ok(list) => {
            views.vanished_keys = list.vanished;
            list.paths
        }
        Err(why) => {
            views.failed.push(why);
            Vec::new()
        }
    };
    let walked: BTreeSet<&str> = listed.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let each = files
        .iter()
        .map(|path| (path, Held::File, &keys))
        .chain(keys.iter().map(|path| (path, Held::Key, &files)));
    for (path, alone, other) in each {
        if walked.contains(path.as_str()) || !seen.insert(path.as_str()) {
            continue;
        }
        let held = match unlisted_as(fs, listed, path, other) {
            Ok(None) => continue,
            Ok(Some(true)) if alone == Held::Key => continue, // Its file's turn counted it.
            Ok(Some(true)) => Ok((path.clone(), Held::Both)),
            Ok(Some(false)) => Ok((path.clone(), alone)),
            Err(why) => Err(why),
        };
        views.unlisted.push(held);
    }
    views
}

/// `None` when `path` is one of `listed` as Windows compares names; otherwise whether the
/// other listing, `other`, holds it too. `Err` when a comparison fails, since it may be.
fn unlisted_as<S: Sweeper>(
    fs: &mut S,
    listed: &[String],
    path: &str,
    other: &[String],
) -> Result<Option<bool>, Failed> {
    if holds(fs, listed, path)? {
        return Ok(None);
    }
    let exact = other.iter().any(|one| one == path);
    Ok(Some(exact || holds(fs, other, path)?))
}

/// Whether `path` is one of `list` as Windows compares names.
fn holds<S: Sweeper>(fs: &mut S, list: &[String], path: &str) -> Result<bool, Failed> {
    for one in list {
        if fs.same_task_path(one, path)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Judges `views`, the second read, as the module says, into `at`'s `swept`.
fn judge<S: Sweeper>(fs: &mut S, views: Views, at: Walk<'_>) {
    let Walk {
        pool,
        names,
        resolved,
        swept,
    } = at;
    for why in views.failed {
        swept.unlisted += 1;
        swept.note(why);
    }
    // Each folder or key gone while listed, checked once more: one there again may hold a
    // task the listing missed.
    let folders = views.vanished_folders.into_iter().map(|path| (path, true));
    let keys = views.vanished_keys.into_iter().map(|path| (path, false));
    for (path, folder) in folders.chain(keys) {
        let present = if folder {
            fs.task_folder_present(&path)
        } else {
            fs.task_cache_key_present(&path)
        };
        match present {
            Ok(false) => swept.vanished += 1,
            Ok(true) => {
                swept.reappeared += 1;
                swept.left_paths.push(path);
                swept.note(REAPPEARED.to_owned());
            }
            Err(why) => {
                swept.reappeared += 1;
                swept.left_paths.push(path);
                swept.note(why);
            }
        }
    }
    for one in views.unlisted {
        let (path, held) = match one {
            Ok(one) => one,
            Err(why) => {
                swept.hidden += 1;
                swept.note(why);
                continue;
            }
        };
        let judged = match held {
            Held::Both => {
                swept.unlisted_in_both += 1;
                match file_trust::not_pool_by_path(fs, &path, pool, names, resolved) {
                    Ok(()) => {
                        swept.file_skipped += 1;
                        Ok(())
                    }
                    Err(why) => Err(why),
                }
            }
            Held::File => {
                swept.unlisted_files += 1;
                match file_owner(fs, &path) {
                    Ok(owner) if pool.contains(&owner) => Err(POOL_FILE.to_owned()),
                    Ok(_) => {
                        swept.stray_files += 1;
                        Ok(())
                    }
                    Err(why) => Err(why),
                }
            }
            Held::Key => {
                swept.unlisted_keys += 1;
                match key_alone(fs, &path) {
                    Ok(()) => {
                        swept.stale_keys += 1;
                        Ok(())
                    }
                    Err(why) => Err(why),
                }
            }
        };
        if let Err(why) = judged {
            swept.hidden += 1;
            swept.left_paths.push(path.clone());
            swept.hidden_paths.push(path);
            swept.note(why);
        }
    }
}

/// Why a folder or key gone while listed and there again is 56.
const REAPPEARED: &str = "a folder or key gone while listed was there again";

/// Whether the cache key at `path`, with no task file, may be ignored, as the module says:
/// `Ok` only when its task can't run; `Err` with why it is 56. Its `SD`'s owner is never read:
/// the registrant gave it.
fn key_alone<S: Sweeper>(fs: &mut S, path: &str) -> Result<(), Failed> {
    if fs.task_cache_runnable(path)? {
        return Err(RUNNABLE_KEY.to_owned());
    }
    Ok(())
}

/// Why a task file with no cache key is 56.
const POOL_FILE: &str = "a task file with no cache key is owned by a pool user";

/// Why a cache key with no task file is 56.
const RUNNABLE_KEY: &str = "a cache key with no task file holds a task that can run";

/// The owner of the task file of the task at `path` (`\A\name`), as the module says.
fn file_owner<S: Sweeper>(fs: &mut S, path: &str) -> Result<Sid, Failed> {
    let (folder, name) = path
        .rsplit_once('\\')
        .ok_or_else(|| "the task's path doesn't start at the root".to_owned())?;
    let folder = if folder.is_empty() { "\\" } else { folder };
    let parts = task_file::parts(folder, name)?;
    fs.task_file_owner(&parts)
}
