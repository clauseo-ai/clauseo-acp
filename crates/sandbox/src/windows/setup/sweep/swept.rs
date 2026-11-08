//! What the task walk (step 4) did, counted ([`Swept`]), and the run's one log line
//! ([`log_line`]): counts only, never a path, task name or SID.

use std::collections::BTreeMap;

use super::{FOLDERS_CAP, Failed, JOBS_CAP, Jobs};
#[cfg(doc)]
use super::{running, unlisted};

/// What the task walk (step 4) did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Swept {
    /// Folders read, or tried.
    pub folders: usize,
    /// Tasks listed.
    pub tasks: usize,
    /// The pool's tasks deleted.
    pub deleted: usize,
    /// Tasks whose principal or owner couldn't be read, and whose task file, when read
    /// instead, didn't show them not the pool's.
    pub unreadable: usize,
    /// Tasks whose principal's name resolves to no SID and is another domain's: skipped.
    pub skipped: usize,
    /// Tasks whose principal's name resolves to no SID, isn't another domain's and isn't a pool
    /// user's.
    pub unresolved: usize,
    /// Tasks whose principal names neither a user nor a group.
    pub no_principal: usize,
    /// The pool's tasks whose delete failed.
    pub not_deleted: usize,
    /// Folders whose tasks or subfolders couldn't be listed, a subfolder that couldn't be
    /// read, or the root that couldn't be opened.
    pub unlisted: usize,
    /// More than [`FOLDERS_CAP`] folders: those past it weren't read.
    pub capped: bool,
    /// Each failure's text, with how often it came: never logged, for the tests' diagnostics.
    pub why: BTreeMap<Failed, usize>,
    /// Tasks the Task Scheduler wouldn't give whose task file, its owner and DACL leaving no
    /// pool user a way to change it, names a principal that isn't the pool's: skipped.
    pub file_skipped: usize,
    /// The pool's tasks' running instances stopped.
    pub instances_stopped: usize,
    /// Their instances not stopped, or that may be theirs ([`running`]).
    pub instances_not_stopped: usize,
    /// The pool's tasks found, deleted or not: each its path (`\A\name`, `Err` where its
    /// folder's path couldn't be read), for the instance stop after the BITS step
    /// ([`running`]); never logged.
    pub pool_paths: Vec<Result<String, Failed>>,
    /// Tasks a task file or the Task Scheduler's cache holds that the Task Scheduler's listing
    /// didn't give (one whose descriptor shuts this process out, say), and whose task file
    /// didn't show them not the pool's ([`unlisted`]).
    pub hidden: usize,
    /// The three listings disagreed (a task one held that the walk didn't list), or one
    /// couldn't be read or changed while read, so the cross-check read them all again and
    /// judged that second read alone ([`unlisted`]).
    pub relisted: bool,
    /// In the read judged, the tasks the walk didn't list that a task file and the cache both
    /// hold.
    pub unlisted_in_both: usize,
    /// In the read judged, the tasks the walk didn't list in a task file alone.
    pub unlisted_files: usize,
    /// In the read judged, the tasks the walk didn't list in the cache alone.
    pub unlisted_keys: usize,
    /// Of the task files alone, those ignored: the file's own owner isn't a pool SID.
    pub stray_files: usize,
    /// Of the cache keys alone, those ignored: their task can't run (no `TaskCache\Tasks` key for
    /// their `Id`), or their `SD` is readable and names an owner that isn't a pool SID.
    pub stale_keys: usize,
    /// Folders below `Tasks` or cache keys gone while they were listed, left out of it, and
    /// still gone when checked once more.
    pub vanished: usize,
    /// Each hidden task's path, where it was read: never logged, for the tests.
    pub hidden_paths: Vec<String>,
    /// Tasks the second walk listed that the first didn't, held to the first walk's check.
    pub arrived: usize,
    /// Of those, the pool's: 56, deleted or not.
    pub arrived_pool: usize,
    /// Folders or keys gone while listed that were there again when checked once more, or
    /// couldn't be checked: 56.
    pub reappeared: usize,
    /// Hidden tasks a task file and the cache hold whose trusted task file names a principal
    /// that isn't the pool's, skipped though the owner their cache key's `SD` names couldn't
    /// be read (unreadable, ownerless, over its cap).
    pub odd_sd_skipped: usize,
}

impl Swept {
    /// Whether a task of the pool's may be left: step 4's 56.
    pub fn left(&self) -> bool {
        self.unreadable > 0
            || self.no_principal > 0
            || self.unresolved > 0
            || self.not_deleted > 0
            || self.instances_not_stopped > 0
            || self.hidden > 0
            || self.arrived_pool > 0
            || self.reappeared > 0
            || self.unlisted > 0
            || self.capped
    }

    /// Counts `why`.
    pub(super) fn note(&mut self, why: Failed) {
        *self.why.entry(why).or_default() += 1;
    }
}

/// The run's log line: counts, and the BITS list's failure, which names no user.
pub(super) fn log_line(users: usize, swept: &Swept, jobs: &Result<Jobs, Failed>) -> String {
    let (cancelled, failed) = jobs
        .as_ref()
        .map_or((0, 0), |jobs| (jobs.cancelled, jobs.failed));
    let unlisted = if swept.unlisted > 0 {
        format!("; {} folders couldn't be listed", swept.unlisted)
    } else {
        String::new()
    };
    let capped = if swept.capped {
        format!("; there were more than {FOLDERS_CAP} folders")
    } else {
        String::new()
    };
    let listed = match jobs {
        Ok(jobs) if jobs.capped => format!("; the BITS job list was longer than {JOBS_CAP}"),
        Ok(_) => String::new(),
        Err(why) => format!("; {why}"),
    };
    let relisted = if swept.relisted {
        format!(
            "; the listings disagreed and were read again: {} tasks in a task file and the cache \
             only, {} task files with no cache key ({} ignored, not a pool user's), {} cache keys \
             with no task file ({} ignored, not a pool user's or can't run), {} folders or keys \
             gone while listed and still gone, {} there again, {} tasks new to the second walk \
             ({} the pool's)",
            swept.unlisted_in_both,
            swept.unlisted_files,
            swept.stray_files,
            swept.unlisted_keys,
            swept.stale_keys,
            swept.vanished,
            swept.reappeared,
            swept.arrived,
            swept.arrived_pool
        )
    } else {
        String::new()
    };
    format!(
        "the sandbox's task sweep: {users} pool users, {} folders and {} tasks read, {} of the \
         pool's tasks deleted, {} unreadable ({} more skipped, not the pool's by their task \
         file, and {} with an odd descriptor), {} with no user or group, {} unresolved (no SID, \
         not another domain's), {} skipped (another domain's, no SID), {} not deleted, {} \
         not listed (in a task file or the cache only), {} running instances stopped, {} not, \
         {cancelled} BITS jobs cancelled, {failed} not{unlisted}{capped}{listed}{relisted}",
        swept.folders,
        swept.tasks,
        swept.deleted,
        swept.unreadable,
        swept.file_skipped,
        swept.odd_sd_skipped,
        swept.no_principal,
        swept.unresolved,
        swept.skipped,
        swept.not_deleted,
        swept.hidden,
        swept.instances_stopped,
        swept.instances_not_stopped
    )
}
