//! Windows: whether an agent's Job Object still holds a running process
//! (`JobObjectBasicAccountingInformation`'s `ActiveProcesses`), for the host's wait before it
//! frees a session's ports. The job never allows breakaway (`windows.rs` sets only
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), so every process the agent starts stays in it. A
//! child of `windows.rs`, whose `unsafe_code` expectation covers its one Win32 call.

use std::os::windows::io::OwnedHandle;
use std::sync::Arc;

use windows::Win32::System::JobObjects::{
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
    QueryInformationJobObject,
};

use super::{Tree, raw};

/// Asks whether the agent's job is empty. Holds the job open (see `TreeWatch`).
///
/// So if the handle's `TerminateJobObject` failed, the job's kill-on-close cannot fire while a
/// watch lives: the host's wait then runs its whole cap and holds the session, and the job
/// closes, ending what is left, only once the last watch drops.
#[derive(Debug, Clone)]
pub(crate) struct Watch {
    job: Arc<OwnedHandle>,
}

/// A [`Watch`] on `tree`'s job.
pub(crate) fn watch(tree: &Tree) -> Watch {
    Watch {
        job: Arc::clone(&tree.job),
    }
}

impl Watch {
    /// Whether the job holds no running process; false when the query fails, so a caller's
    /// wait fails closed.
    pub(crate) fn gone(&self) -> bool {
        active(&self.job) == Some(0)
    }
}

/// The job's running processes, or `None` when the query fails.
fn active(job: &OwnedHandle) -> Option<u32> {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    let size = u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>()).ok()?;
    // SAFETY: `info` is a live JOBOBJECT_BASIC_ACCOUNTING_INFORMATION of `size` bytes, the
    // struct this information class writes, and `job` is a live job handle.
    unsafe {
        QueryInformationJobObject(
            Some(raw(job)),
            JobObjectBasicAccountingInformation,
            (&raw mut info).cast(),
            size,
            None,
        )
    }
    .ok()?;
    Some(info.ActiveProcesses)
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
}
