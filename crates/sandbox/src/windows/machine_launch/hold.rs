//! What keeps the launch value where COM reads it. DCOM's launcher service keeps
//! `MachineLaunchRestriction` only while `MachineAccessRestriction` is there too: without it, it
//! moves a new launch value to `MachineLaunchRestrictionOld` within about 10 ms (a VM run of row 4,
//! the absent case), and no deny holds.
//!
//! So before writing the launch value, a missing access value is written as exactly the access
//! descriptor COM uses now ([`LaunchStore::effective_access`]), which changes nothing in effect,
//! and its bytes go to the backup. When COM's descriptor can't be read, nothing is written and the
//! guard is degraded: no default is guessed. After the write, the values are read back after
//! [`SETTLE_MS`]: the guard counts only when the deny is still first at each read.
//!
//! The removal deletes the access value only while it is still exactly the bytes Clauseo wrote
//! and no launch value is left that DCOM would then move, and takes Clauseo's bytes out of
//! `MachineLaunchRestrictionOld` when they landed there. Pure; tested on every OS.

use super::seam::{Backup, LaunchStore, Raw, StoreError};
use super::{Guard, LaunchSd};
use crate::windows::sids::Sid;

/// The waits before each read-back: 250 ms, then up to 1 s after the write.
pub const SETTLE_MS: [u64; 2] = [250, 750];

/// The access value to write before the launch value: COM's effective descriptor when there is
/// no value; `None` when there is one (of any type), which is never touched.
pub(super) fn access_to_add(
    store: &mut impl LaunchStore,
) -> Result<Option<Vec<u8>>, (Guard, String)> {
    match store.access() {
        Ok(Raw::Absent) => store.effective_access().map(Some).map_err(|why| {
            let why = format!("COM's machine access can't be read, so nothing was written: {why}");
            (Guard::AccessUnknown, why)
        }),
        Ok(Raw::Binary(_) | Raw::OtherType) => Ok(None),
        Err(why) => Err((Guard::Failed, why)),
    }
}

/// After the launch value was written as `written`: `Ok` only when the deny is still first and
/// the access value still there at each read-back; otherwise [`Guard::Moved`], with why.
pub(super) fn verify(
    store: &mut impl LaunchStore,
    group: &Sid,
    written: &[u8],
) -> Result<(), (Guard, String)> {
    let failed = |why: String| (Guard::Failed, why);
    for millis in SETTLE_MS {
        store.settle(millis);
        let held = match store.read().map_err(failed)? {
            Raw::Binary(bytes) => LaunchSd::parse(&bytes).is_ok_and(|sd| sd.denies_first(group)),
            Raw::Absent | Raw::OtherType => false,
        };
        if !held {
            let moved = matches!(store.old(), Ok(Raw::Binary(old)) if old == written);
            let why = if moved {
                "dcom moved the value to MachineLaunchRestrictionOld"
            } else {
                "the value didn't hold the deny when read back"
            };
            return Err((Guard::Moved, why.to_owned()));
        }
        if store.access().map_err(failed)? == Raw::Absent {
            let why = "MachineAccessRestriction is gone, and dcom won't keep the value without it";
            return Err((Guard::Moved, why.to_owned()));
        }
    }
    Ok(())
}

/// The removal's last part, before the backup goes: Clauseo's bytes taken out of
/// `MachineLaunchRestrictionOld`, and the access value Clauseo added deleted, each only when
/// byte-identical to what Clauseo wrote; anything else is left and logged.
pub(super) fn remove_extras(
    store: &mut impl LaunchStore,
    backup: Option<&Backup>,
) -> Result<(), StoreError> {
    let Some(backup) = backup else {
        return Ok(());
    };
    if matches!(store.old()?, Raw::Binary(old) if old == backup.written) {
        store.delete_old()?;
        tracing::info!("Clauseo's value was taken out of MachineLaunchRestrictionOld");
    }
    let Some(added) = &backup.access else {
        return Ok(());
    };
    match store.access()? {
        Raw::Absent => {}
        Raw::Binary(now) if now == *added => {
            if store.read()? == Raw::Absent {
                store.delete_access()?;
                tracing::info!("the MachineAccessRestriction Clauseo added was deleted");
            } else {
                tracing::warn!(
                    "the MachineAccessRestriction Clauseo added was left: a launch value is \
                     still there, and dcom won't keep it without one"
                );
            }
        }
        Raw::Binary(_) | Raw::OtherType => {
            tracing::warn!(
                "MachineAccessRestriction changed since Clauseo added it, so it was left"
            );
        }
    }
    Ok(())
}
