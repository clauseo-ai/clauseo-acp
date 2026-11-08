//! The group's deny healed with no prompt ("Admin copy" in 0049). [`super::check`] requires the
//! deny once the record names the pool's group, but two upgrades leave a copy without it: a
//! machine set up before the deny existed, and a per-user update, whose refresh runs from the
//! old copy and secures the new one with the old DACL. Both leave the folder and the copy locked
//! exactly as before ([`locked`] with no group), which [`owed`] tells apart from any other drift.
//!
//! - **SYSTEM.** Each refresh run of this build, with the set-up lock held and the record read,
//!   gives the folder and the copy the deny when [`owed`] says each is locked one way or the
//!   other and one lacks it (`refresh/deny.rs`). Nothing else is changed.
//! - **The lawyer's side.** Before each check that refuses a launch or a set-up, or shows the
//!   state ([`heal_owed`]): when the copy lacks only the deny (every other rule holds and
//!   [`owed`] says [`Owed::Lacking`]), it asks the refresh task to run and looks again every
//!   [`HEAL_PAUSE`], at most [`HEAL_LOOKS`] times, asking again every [`ASK_EVERY`] looks (the
//!   run it started may be the old copy's, which swaps in this one but can't heal; the task
//!   never runs twice at once, so an ask during a run is ignored). Then the check decides, as
//!   ever. Once a wait has run out in a process, later ones ask and look once, so a heal that
//!   can't happen never stalls each state query.
//!
//! Anything but the exact old form, or the deny on one and drift on the other, stays
//! `AdminCopyUnsafe`, for the installer's repair.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::{AdminCopyFs, admin_dir, admin_exe, checked, locked};
use crate::windows::sids::Sid;

/// How long the lawyer's side waits between looks at a copy being healed.
pub(crate) const HEAL_PAUSE: Duration = Duration::from_millis(500);
/// How many looks it takes at most: 20 seconds, time for an old copy's refresh run to swap in
/// this build and for this build's run to heal it.
pub(crate) const HEAL_LOOKS: u32 = 40;
/// It asks the task again every this many looks: every 3 seconds.
pub(crate) const ASK_EVERY: u32 = 6;

/// Whether a wait has run out in this process.
static WAITED_OUT: AtomicBool = AtomicBool::new(false);

/// How the admin folder and the copy stand against the recorded group's deny.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owed {
    /// Both are locked with the deny.
    Locked,
    /// Both are locked exactly, with or without the deny, and these lack it.
    Lacking {
        /// The folder lacks it.
        folder: bool,
        /// The copy lacks it.
        copy: bool,
    },
    /// Either is anything else: left for the installer's repair.
    Drift,
}

/// How the folder's descriptor `folder` and the copy's `copy` stand for `group`, as [`Owed`]
/// says: each must be exactly [`locked`] for `group`, or exactly locked with no group.
pub fn owed(folder: &[u8], copy: &[u8], group: &Sid) -> Owed {
    let lacks = |bytes: &[u8], is_folder: bool| {
        if locked(bytes, is_folder, Some(group)) {
            Some(false)
        } else if locked(bytes, is_folder, None) {
            Some(true)
        } else {
            None
        }
    };
    match (lacks(folder, true), lacks(copy, false)) {
        (Some(false), Some(false)) => Owed::Locked,
        (Some(folder), Some(copy)) => Owed::Lacking { folder, copy },
        _ => Owed::Drift,
    }
}

/// The lawyer's side, as the module says: asks the refresh task to heal a copy that lacks only
/// the group's deny, and waits for it, bounded. Changes nothing itself; the check that follows
/// decides.
pub(super) fn heal_owed(fs: &dyn AdminCopyFs) {
    heal_with(fs, &WAITED_OUT);
}

/// [`heal_owed`], with `waited_out` as the process's flag; whether the deny is there now.
pub(super) fn heal_with(fs: &dyn AdminCopyFs, waited_out: &AtomicBool) -> bool {
    if !matches!(standing(fs), Some(Owed::Lacking { .. })) {
        return false;
    }
    let rest = checked(fs, false, |exe| fs.sha256(exe).map(|hash| (hash, ())));
    if rest.is_err() {
        return false;
    }
    tracing::info!("the admin copy lacks the sandbox group's deny; asking the refresh task");
    if let Err(error) = fs.request_refresh() {
        tracing::warn!(%error, "the refresh task couldn't be started to heal the admin copy");
        return false;
    }
    let looks = if waited_out.load(Ordering::Relaxed) {
        1
    } else {
        HEAL_LOOKS
    };
    for look in 1..=looks {
        fs.pause(HEAL_PAUSE);
        // Nothing to read is the old copy's swap between its two renames: look again.
        match standing(fs) {
            Some(Owed::Lacking { .. }) | None => {}
            Some(Owed::Locked) => {
                tracing::info!("the refresh task gave the admin copy the sandbox group's deny");
                return true;
            }
            Some(Owed::Drift) => return false,
        }
        if look % ASK_EVERY == 0
            && let Err(error) = fs.request_refresh()
        {
            tracing::warn!(%error, "the refresh task couldn't be started again");
        }
    }
    waited_out.store(true, Ordering::Relaxed);
    tracing::warn!("the admin copy still lacks the sandbox group's deny");
    false
}

/// [`owed`] for the machine's folder and copy and its recorded group; `None` with no group or
/// when any of it can't be read.
fn standing(fs: &dyn AdminCopyFs) -> Option<Owed> {
    let group = fs.recorded_group().ok()??;
    let program_files = fs.program_files().ok()?;
    let folder = fs.security(&admin_dir(&program_files)).ok()?;
    let copy = fs.security(&admin_exe(&program_files)).ok()?;
    Some(owed(&folder, &copy, &group))
}
