//! `--reclaim --parent <pid>` (S15 `PR4i`, D-B10): the elevated clean-up. It takes no path and no
//! data folder from its command line.
//!
//! 1. The owner is the verified parent's token user (`Owner::of_parent`, D-B2). The machine's
//!    record must name that user as its `OwnerSid`, or nothing is touched (`OtherOwner`); a
//!    record whose removal is unfinished is left alone too (`Failed`).
//! 2. The data folder is the record's `DataDir` (written by an administrator), resolved as the
//!    owner and owned by the owner, as `--set-up`'s is (`exe::owners_data_dir`).
//! 3. The ledger and `matters.json` are read as the owner, and [`targets`] picks the trees from
//!    them: an owed hand-back's matter, an item a launch refused at.
//! 4. Each pool user's profile folder is read from `ProfileList` (an administrator's key). The
//!    walk refuses a root outside every recorded matter root, at a volume's root, or holding or
//!    in a profile, all as final paths, and one neither a pool user's nor the owner's to list
//!    (`reclaim::walk`).
//! 5. `SeTakeOwnershipPrivilege`, `SeRestorePrivilege` and `SeBackupPrivilege` are enabled for
//!    the walks and disabled again after (`acl/privilege.rs`); a token that lacks one fails here,
//!    naming it.
//! 6. Each tree is walked ([`take_ownership`] over `acl/take.rs`), entering only what a pool
//!    user owns or the owner can list: what a pool SID of the record owns and the owner can't
//!    hand back (`Owner::granted`) loses its `OWNER RIGHTS` entries (`drop_owner_rights`, which
//!    would take the new owner's implied rights away) and gets the owner as its owner, and
//!    nothing else changes (K1).
//! 7. `setup-result.json` is written as the owner: 0 when nothing failed; else the counts, of
//!    what the walks reached only, with how many items were taken. Before it, as the owner too,
//!    `reclaim-taken.json` lists each item taken (`taken_file.rs`), so the app's hand-back strips
//!    the agent's denies of the lawyer on those items alone.
//!
//! The app then hands back what was taken, as the lawyer (`UserSetup::reclaim`), and retries the
//! forget or the launch.

use std::path::{Path, PathBuf};

use super::exe::owners_data_dir;
use super::outcome::SetupExit;
use super::owner::Owner;
use super::profile::profile_of;
use super::record_write;
use super::result_file::{self, SetupResult};
use super::taken_file;
use crate::windows::acl::privilege::{Privileges, RECLAIM};
use crate::windows::acl::take::NtfsOwners;
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::matters::MatterSids;
use crate::windows::pool::MachineRecord;
use crate::windows::reclaim::{
    Reclaimed, Scope, TakenItem, matter_roots, take_ownership, taken_items, targets,
};
use crate::windows::sids::{Reserved, Sid};

/// The result file's step for the clean-up.
const STEP: &str = "taking back what the sandbox users own";

/// `--reclaim` for `owner`, against the record under `HKLM\<key>`, as the module says.
pub(crate) fn run_reclaim(owner: &Owner, key: &str) -> SetupExit {
    let record = record_write::read(key).map_err(drop);
    run_with(owner, record)
}

/// [`run_reclaim`] once the record is read (`Err` when it couldn't be): steps 1 to 7.
pub(super) fn run_with(owner: &Owner, record: Result<Option<MachineRecord>, ()>) -> SetupExit {
    let record = match record {
        Ok(Some(record)) => record,
        Ok(None) => {
            tracing::error!("there is no pool to clean up after: nothing is set up");
            return SetupExit::Failed;
        }
        Err(()) => {
            tracing::error!("the machine's record can't be read");
            return SetupExit::Failed;
        }
    };
    if record.owner != *owner.sid() {
        tracing::error!("the pool belongs to another user; nothing is touched");
        return SetupExit::OtherOwner;
    }
    if record.removing {
        tracing::error!("the pool's removal is unfinished; nothing is touched");
        return SetupExit::Failed;
    }
    let data_dir = match owners_data_dir(owner, &record.data_dir) {
        Ok(data_dir) => data_dir,
        Err((exit, _)) => {
            // Its words name the folder and its owner: kept out of the log.
            tracing::error!(?exit, "the recorded data folder isn't the owner's own");
            return exit;
        }
    };
    let result = match reclaim(owner, &record, &data_dir) {
        Ok((done, taken)) => {
            let written = owner.run_as(|| taken_file::write(&data_dir, &taken));
            if !taken.is_empty() && !matches!(written, Ok(Ok(()))) {
                tracing::error!("reclaim-taken.json couldn't be written");
            }
            result_of(done)
        }
        Err(cause) => {
            // The cause goes to the result file, for the app; the log names nothing.
            tracing::error!("the clean-up failed");
            SetupResult::failed(SetupExit::Failed, STEP, cause)
        }
    };
    let exit = SetupExit::from_code(u32::from(result.exit)).unwrap_or(SetupExit::Failed);
    if !matches!(
        owner.run_as(|| result_file::write(&data_dir, &result)),
        Ok(Ok(()))
    ) {
        tracing::error!("setup-result.json couldn't be written");
    }
    exit
}

/// What the walks did, as the result file says it, with how many items were taken: done when
/// nothing failed (roots refused and objects left shut named in the cause), else failed with
/// the counts. Every count is of what the walks reached.
pub(super) fn result_of(done: Reclaimed) -> SetupResult {
    let mut notes = Vec::new();
    if done.roots_refused > 0 {
        notes.push(format!(
            "{} recorded items were refused: outside every matter, or neither a sandbox user's \
             nor yours to open",
            done.roots_refused
        ));
    }
    if done.left > 0 {
        notes.push(format!(
            "{} items no sandbox user owns stay shut to you; they were left as they are",
            done.left
        ));
    }
    let mut result = if done.failed > 0 {
        let failed = format!(
            "{} items couldn't be read or given back to you ({} were)",
            done.failed, done.taken
        );
        notes.insert(0, failed);
        SetupResult::failed(SetupExit::Failed, STEP, notes.join("; "))
    } else {
        SetupResult {
            exit: SetupExit::Done.as_code(),
            step: None,
            cause: (!notes.is_empty()).then(|| notes.join("; ")),
            blocked: None,
            taken: None,
        }
    };
    result.taken = Some(done.taken);
    result
}

/// Steps 3 to 6: the targets, read as the owner, walked with the privileges enabled; with each
/// item taken, under the matter whose target found it.
fn reclaim(
    owner: &Owner,
    record: &MachineRecord,
    data_dir: &Path,
) -> Result<(Reclaimed, Vec<TakenItem>), String> {
    let mut trusted: Vec<Sid> = record.pool.clone();
    trusted.push(record.group.clone());
    let (found, bounds) = owner
        .run_as(|| -> Result<_, String> {
            // As the owner, so this machine's reservations name the owner, not the admin.
            let reserved = Reserved::of_this_machine().map_err(|e| e.to_string())?;
            let ledger = Ledger::open(ledger_path(data_dir), &trusted, &reserved)
                .map_err(|e| format!("the permission ledger: {e}"))?;
            let matters = MatterSids::load(data_dir).map_err(|e| e.to_string())?;
            Ok((targets(&ledger, matters.rows()), matter_roots(&ledger)))
        })
        .map_err(|e| e.to_string())??;
    let mut done = Reclaimed::default();
    let mut taken = Vec::new();
    if found.is_empty() {
        tracing::info!("nothing is owed a clean-up");
        return Ok((done, taken));
    }
    let profiles = profiles(&record.pool)?;
    let _privileges = Privileges::enable(&RECLAIM).map_err(|e| e.to_string())?;
    let granted = |descriptor: &[u8]| {
        owner
            .granted(descriptor)
            .map_err(|e| std::io::Error::other(e.to_string()))
    };
    let mut tree = NtfsOwners::new(&granted);
    let scope = Scope {
        pool: &record.pool,
        owner: owner.sid(),
        bounds: &bounds,
        profiles: &profiles,
    };
    for target in &found {
        done.add(take_ownership(
            &mut tree,
            &target.root,
            target.whole,
            &scope,
        ));
        taken.extend(taken_items(target.matter, tree.drain_taken()));
    }
    Ok((done, taken))
}

/// Each pool user's profile folder, from `ProfileList`; one that can't be read fails the
/// clean-up, since its walks couldn't keep out of it.
fn profiles(pool: &[Sid]) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for sid in pool {
        match profile_of(sid) {
            Ok(Some(profile)) => found.push(profile.path),
            Ok(None) => {}
            Err(error) => return Err(format!("a sandbox user's profile: {error}")),
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
