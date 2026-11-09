//! The outer runner's failure: the kind it is said with on stderr (`stderr::refuse`), whose
//! [`RunExit`] the run fails with, and its detail.

use super::super::RunExit;
use super::super::stderr::{self, Detail, RunnerKind};

/// A runner failure: its kind, whose [`RunExit`] the run fails with, and its detail.
pub(super) struct Failure {
    pub(super) kind: RunnerKind,
    pub(super) detail: Detail,
}

impl Failure {
    /// A failure of `exit`'s own kind.
    pub(super) fn new(exit: RunExit, detail: impl Into<Detail>) -> Failure {
        let kind = RunnerKind::for_exit(exit).unwrap_or(RunnerKind::Internal);
        Failure::of(kind, detail)
    }

    /// A failure of `kind`, a finer one than its exit's own.
    pub(super) fn of(kind: RunnerKind, detail: impl Into<Detail>) -> Failure {
        Failure {
            kind,
            detail: detail.into(),
        }
    }

    /// The exit the run fails with.
    pub(super) fn exit(&self) -> RunExit {
        self.kind.exit().unwrap_or(RunExit::Spawn)
    }

    /// Says it on stderr ([`stderr::refuse`]) and gives its exit code.
    pub(super) fn refuse(self) -> u32 {
        let code = self.exit().code();
        stderr::refuse(self.kind, self.detail);
        code
    }
}
