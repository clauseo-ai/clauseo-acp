//! The launch check: the clean-close record read and removed, then each condition in turn, and
//! the journal read from the cursor. Every doubt is the full walk.

use std::collections::BTreeSet;
use std::path::Path;

use super::changes::{Live, most_touched, read_volume, reduce, resolve};
use super::journal::Journal;
use super::record::{self, CleanRecord, Unusable};
#[cfg(windows)]
use super::wsl_heal::WindowsDenyTask;
use super::wsl_heal::{self, DenyTask, LaunchProgress};
use super::{FullReason, LaunchCheck, WalkInputs};
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, in_folder};
use crate::windows::admin_copy::{self, AdminCopyFs};
use crate::windows::ledger::{EntryState, Ledger};
use crate::windows::set_aside;
use crate::windows::wsl::{self, WslRead};

/// The launch check, on the host at the lawyer's token, before the set-aside's launch pass and
/// `apply`. First the pool is this user's and the set-up program's admin copy passes
/// `admin_copy::check_launchable`, or the launch is refused with [`Error::Blocked`]
/// (`OtherAccount`, `AdminCopyMissing`, `AdminCopyChanged`, `AdminCopyUnsafe`, or `ToolMissing`
/// for a copy gone after Windows security blocked it) and nothing
/// is read or removed: without a copy only administrators can
/// change, tier B can't top up, reclaim or be removed safely, so it doesn't run. Next, with WSL
/// on the machine, its classes' launch permissions must deny the pool group first
/// (`wsl::check`), or the launch is refused with `WslDenyMissing` or `WslDenyUnverifiable`,
/// and no WSL upgrade may be pending (`WslUpgradePending`): a pool user who can start WSL gets a
/// Linux the network filters don't see. Both are healed first (`wsl_heal`): the WSL deny task
/// is asked to run, as the lawyer, and the check waits for it, up to 10 s for a missing deny and
/// up to 120 s for a pending upgrade, which its SYSTEM run installs before it re-applies the
/// deny; `progress` hears [`LaunchProgress::UpdatingWindowsComponent`] when that wait begins, so
/// the host can say so. A pending upgrade refuses nothing once the deny is verified; what the
/// healing did is [`LaunchCheck::wsl_healed`], for the host's log. Then, under the
/// matter's set-aside lock, it reads the matter's clean-close record and removes it, whatever
/// comes next: a failed removal refuses the launch
/// ([`Error::CleanClose`]), and a lock that can't be taken too ([`Error::SetAside`]). Then it
/// decides ([`LaunchCheck::path`]): see the module's docs and 0049. `ledger` is read as the
/// caller opened it, just before.
#[cfg(windows)]
pub fn check_launch(
    data_dir: &Path,
    ledger: &Ledger,
    plan: &AdmittedPlan,
    journal: &dyn Journal,
    progress: &dyn Fn(LaunchProgress),
) -> Result<LaunchCheck, Error> {
    let seams = Seams {
        admin: &admin_copy::os::WindowsAdminCopy,
        wsl: &wsl::WindowsWsl::reader(),
        task: &WindowsDenyTask::new(),
        progress,
    };
    check_launch_in(&seams, data_dir, ledger, plan, journal)
}

/// What the launch check reads through: the machine on Windows, fakes in the tests.
pub(crate) struct Seams<'a> {
    /// The admin copy.
    pub(crate) admin: &'a dyn AdminCopyFs,
    /// WSL's registration.
    pub(crate) wsl: &'a dyn WslRead,
    /// The WSL deny task, and the clock its waits use.
    pub(crate) task: &'a dyn DenyTask,
    /// Told when the wait for a WSL upgrade begins.
    pub(crate) progress: &'a dyn Fn(LaunchProgress),
}

/// [`check_launch`] with no WSL deny task (a request fails, as where it isn't registered).
#[cfg(test)]
pub(crate) fn check_launch_with(
    admin: &dyn AdminCopyFs,
    wsl: &dyn WslRead,
    data_dir: &Path,
    ledger: &Ledger,
    plan: &AdmittedPlan,
    journal: &dyn Journal,
) -> Result<LaunchCheck, Error> {
    let task = super::wsl_heal::fake::FakeTask::absent();
    let seams = Seams {
        admin,
        wsl,
        task: &task,
        progress: &|_| {},
    };
    check_launch_in(&seams, data_dir, ledger, plan, journal)
}

/// [`check_launch`] through `seams`.
pub(crate) fn check_launch_in(
    seams: &Seams<'_>,
    data_dir: &Path,
    ledger: &Ledger,
    plan: &AdmittedPlan,
    journal: &dyn Journal,
) -> Result<LaunchCheck, Error> {
    admin_copy::check_launchable(seams.admin).map_err(Error::Blocked)?;
    let healed = wsl_heal::heal(&mut || wsl::inspect(seams.wsl), seams.task, seams.progress);
    let (found, healed) = healed.map_err(|refused| {
        let (blocked, why) = (refused.blocked, refused.why);
        tracing::warn!(?blocked, %why, "WSL's check refused the launch");
        Error::Blocked(blocked)
    })?;
    tracing::info!(?found, ?healed, "WSL's deny for the pool");
    let check = record_check(data_dir, ledger, plan, journal)?;
    Ok(check.with_wsl(healed))
}

/// The clean-close record read and removed under the matter's lock, then the decision.
fn record_check(
    data_dir: &Path,
    ledger: &Ledger,
    plan: &AdmittedPlan,
    journal: &dyn Journal,
) -> Result<LaunchCheck, Error> {
    let matter = plan.matter();
    let held = set_aside::under_lock(data_dir, &matter, |open| {
        let found = record::read(data_dir, &matter);
        let removed = record::remove(data_dir, &matter);
        (open, found, removed)
    });
    let (open, found, removed) = held.map_err(|failure| Error::SetAside { failure, at: None })?;
    if let Err(error) = removed {
        tracing::warn!(kind = %error.kind(), "the clean-close record couldn't be removed");
        return Err(Error::CleanClose(error.kind()));
    }
    let record = match found {
        Ok(Some(Ok(record))) => record,
        Ok(None) => return Ok(full(FullReason::NoRecord)),
        Ok(Some(Err(Unusable::Version))) => return Ok(full(FullReason::Version)),
        Ok(Some(Err(Unusable::Unreadable))) | Err(_) => return Ok(full(FullReason::Unreadable)),
    };
    let inputs = WalkInputs::of_plan(plan);
    let check = match decide(&record, open, &inputs, ledger, journal) {
        Ok(targets) => LaunchCheck::fast(inputs, targets),
        Err(reason) => LaunchCheck::full(reason),
    };
    tracing::info!(path = ?check.path(), "tier B's launch check");
    Ok(check)
}

fn full(reason: FullReason) -> LaunchCheck {
    tracing::info!(?reason, "tier B's launch check: the full walk");
    LaunchCheck::full(reason)
}

/// The conditions after the record was read, in order; the targets when all hold.
fn decide(
    record: &CleanRecord,
    open: bool,
    inputs: &WalkInputs,
    ledger: &Ledger,
    journal: &dyn Journal,
) -> Result<Vec<super::Target>, FullReason> {
    let header = &record.header;
    if open {
        return Err(FullReason::SessionOpen);
    }
    if header.inputs != *inputs {
        return Err(FullReason::PlanChanged);
    }
    match busy(ledger, header.matter, inputs) {
        Some(Busy::Ledger) => return Err(FullReason::LedgerBusy),
        Some(Busy::Handback) => return Err(FullReason::HandbackOwed),
        None => {}
    }
    let mut live = Vec::new();
    for (path, kind) in &inputs.roots {
        let mark = header
            .roots
            .iter()
            .find(|r| r.path == *path && r.kind == *kind)
            .ok_or(FullReason::PlanChanged)?;
        let facts = journal
            .root(path)
            .map_err(|_unread| FullReason::RootUnreadable)?;
        if facts.removable {
            return Err(FullReason::Removable);
        }
        if !facts.ntfs || facts.id.is_none() {
            return Err(FullReason::NotNtfs);
        }
        if facts.volume != mark.volume || facts.id != Some(mark.id) {
            return Err(FullReason::RootChanged);
        }
        live.push(Live {
            path: path.clone(),
            kind: *kind,
            volume: facts.volume,
            found: facts.path,
        });
    }
    let items: usize = record.known.iter().map(super::record::Known::len).sum();
    let most = most_touched(items);
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for (mark, known) in header.volumes.iter().zip(&record.known) {
        let root = live
            .iter()
            .find(|l| l.volume == mark.volume)
            .ok_or(FullReason::RootChanged)?;
        let state = journal.query(&root.path).map_err(|error| match error {
            super::JournalError::NotNtfs => FullReason::NotNtfs,
            super::JournalError::NoJournal => FullReason::NoJournal,
            super::JournalError::Unsupported => FullReason::Unsupported,
            super::JournalError::Lost => FullReason::Wrapped,
            super::JournalError::Failed(_) => FullReason::ReadFailed,
        })?;
        if state.id != mark.journal {
            return Err(FullReason::JournalChanged);
        }
        if mark.cursor < state.first {
            return Err(FullReason::Wrapped);
        }
        if mark.cursor > state.next {
            return Err(FullReason::RolledBack);
        }
        let touched = read_volume(journal, &root.path, mark, state, known, &mut seen, most)?;
        targets.extend(resolve(journal, &live, mark.volume, &touched)?);
    }
    // Every root's volume must have been read.
    if live
        .iter()
        .any(|l| !header.volumes.iter().any(|v| v.volume == l.volume))
    {
        return Err(FullReason::RootChanged);
    }
    Ok(reduce(targets))
}

/// What the ledger owes the own trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Busy {
    /// An entry on, in or above an own root is `pending` or `removing`: its walk is owed.
    Ledger,
    /// A hand-back is owed for the matter.
    Handback,
}

/// What the ledger owes `matter`'s own trees, if anything.
pub(super) fn busy(
    ledger: &Ledger,
    matter: clauseo_protocol::MatterId,
    inputs: &WalkInputs,
) -> Option<Busy> {
    let near = |path: &Path| {
        inputs
            .roots
            .iter()
            .any(|(root, _)| in_folder(path, root) || in_folder(root, path))
    };
    if ledger
        .entries()
        .any(|e| e.state != EntryState::Applied && near(&e.path))
    {
        return Some(Busy::Ledger);
    }
    let owed = ledger.handbacks_owed().iter().any(|o| o.matter == matter);
    owed.then_some(Busy::Handback)
}
