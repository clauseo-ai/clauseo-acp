//! Windows: whether an agent's Job Object still holds a process that has not exited, for the
//! host's wait before it frees a session's ports and the install's before it removes the folder
//! npm ran in. The job never allows breakaway (`windows.rs` sets only
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), so every process the agent starts stays in it.
//!
//! The job's count (`JobObjectBasicAccountingInformation`'s `ActiveProcesses`) is not enough:
//! `TerminateJobObject` sets it to 0 as it is called, while each process is still ending and
//! still holds its handles (its working folder, its sockets). So the watch also holds a handle
//! to each process of the job, each confirmed in the job (`IsProcessInJob`): taken when the
//! watch is made, by the tree's kill (just before `TerminateJobObject`, and from the system's
//! process list after it when a process joined meanwhile), and from the job's list at each check
//! that finds the count at 0. The tree is gone only once the count is 0 and every held process
//! has exited (its handle signalled). A child of `windows.rs`, whose
//! `unsafe_code` expectation covers its Win32 calls.

use std::os::windows::io::OwnedHandle;
use std::sync::{Arc, PoisonError};

use windows::Win32::System::Threading::PROCESS_SYNCHRONIZE;

use super::job::accounting;
use super::{Held, Tree, alive, hold_listed, job_pids, open};

/// Asks whether the agent's job is empty. Holds the job open (see `TreeWatch`).
///
/// So if the handle's `TerminateJobObject` failed, the job's kill-on-close cannot fire while a
/// watch lives: the host's wait then runs its whole cap and holds the session, and the job
/// closes, ending what is left, only once the last watch drops.
#[derive(Debug, Clone)]
pub(crate) struct Watch {
    job: Arc<OwnedHandle>,
    /// The job's processes, held until each has exited; shared with the tree, whose kill adds
    /// the processes it ends.
    held: Held,
    /// The pids a kill spared (a browser, `spare.rs`): left running, not waited for.
    spared: Arc<std::sync::Mutex<Vec<u32>>>,
}

/// A [`Watch`] on `tree`'s job, holding the job's processes now, before any kill.
pub(crate) fn watch(tree: &Tree) -> Watch {
    let watch = Watch {
        job: Arc::clone(&tree.job),
        held: Arc::clone(&tree.held),
        spared: Arc::clone(&tree.spared),
    };
    hold_listed(&watch.job, &watch.held);
    watch
}

impl Watch {
    /// Whether every process of the job has exited: the job counts none running, and each
    /// process held (as the watch was made, by the kill, and from the job's list now, read only
    /// once the count is 0) has exited. False when the count cannot be read, so a caller's wait
    /// fails closed; never true while a held process of the job still holds its handles.
    ///
    /// After a kill that spared a browser, the browser and its children are left out: the tree
    /// is gone once every other process of the job has exited.
    pub(crate) fn gone(&self) -> bool {
        let spared = self
            .spared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if spared.is_empty() {
            if active(&self.job) != Some(0) {
                return false;
            }
        } else if !only_spared(&self.job, &spared) {
            return false;
        }
        hold_listed(&self.job, &self.held);
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|(pid, process)| !spared.contains(pid) && alive(process));
        held.is_empty()
    }
}

/// Whether every process `job` lists that still runs is one of `spared`; false when the list
/// cannot be read.
fn only_spared(job: &OwnedHandle, spared: &[u32]) -> bool {
    job_pids(job).is_some_and(|pids| {
        pids.iter().all(|pid| {
            spared.contains(pid)
                || !open(*pid, PROCESS_SYNCHRONIZE).is_some_and(|process| alive(&process))
        })
    })
}

/// The job's running processes, or `None` when the query fails.
fn active(job: &OwnedHandle) -> Option<u32> {
    accounting(job).map(|(active, _)| active)
}

#[cfg(test)]
mod tests {
    use windows::Win32::System::JobObjects::{
        JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        QueryInformationJobObject,
    };

    use super::super::{Tree, raw};

    /// The agent's job forbids breakaway, so no process of the tree leaves it and the watch
    /// sees them all; an empty job reads as gone.
    #[test]
    fn the_job_forbids_breakaway_and_an_empty_one_is_gone() {
        let tree = Tree::new().expect("create a job");
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap();
        // SAFETY: `limits` is a live struct of `size` bytes, the one this class writes, and
        // `tree.job` is the job handle `tree` owns.
        unsafe {
            QueryInformationJobObject(
                Some(raw(&tree.job)),
                JobObjectExtendedLimitInformation,
                (&raw mut limits).cast(),
                size,
                None,
            )
        }
        .expect("query the job's limits");
        let flags = limits.BasicLimitInformation.LimitFlags;
        assert_eq!((flags & JOB_OBJECT_LIMIT_BREAKAWAY_OK).0, 0);
        assert_eq!((flags & JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK).0, 0);
        assert!(super::watch(&tree).gone(), "an empty job is gone");
    }

    /// A job whose count of running processes is 0 but which holds a process that has not
    /// exited, as just after `TerminateJobObject` while that process is still ending, is not
    /// gone; once the process has exited, it is.
    #[test]
    fn a_held_process_that_has_not_exited_keeps_a_job_with_no_count() {
        use async_process::{Command, Stdio};

        use windows::Win32::System::Threading::{
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };

        let tree = Tree::new().expect("create a job");
        let watch = super::watch(&tree);
        assert!(watch.gone(), "an empty job is gone");
        // `cmd /k` waits on its stdin, which stays open until the kill. Outside the job, so
        // the job's count stays 0, and held as the kill holds a process it is ending.
        let mut child = Command::new(std::env::var_os("ComSpec").expect("ComSpec"))
            .args(["/d", "/k"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start cmd");
        let process = super::super::open(
            child.id(),
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
        )
        .expect("open cmd");
        watch
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((child.id(), process));
        assert_eq!(super::active(&watch.job), Some(0), "the job counts none");
        assert!(!watch.gone(), "a held process that runs keeps the job");
        child.kill().expect("end cmd");
        async_io::block_on(child.status()).expect("reap cmd");
        assert!(watch.gone(), "every held process has exited");
    }
}
