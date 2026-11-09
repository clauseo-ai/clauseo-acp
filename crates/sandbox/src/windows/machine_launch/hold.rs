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
//! The removal deletes the access value only while it still means what Clauseo wrote (DCOM
//! re-serialises it with padding, so bytes don't match)
//! and no launch value is left that DCOM would then move, and takes any value Clauseo wrote (the
//! last [`HISTORY_MAX`]) out of `MachineLaunchRestrictionOld`. A Group Policy access value set
//! means nothing is copied: COM's descriptor may then be the policy's. Pure; tested on every OS.

use super::seam::{Backup, LaunchStore, Raw, StoreError};
use super::{Guard, LaunchSd, Meaning};
use crate::windows::sids::Sid;

/// The waits before each read-back: 250 ms, then up to 1 s after the write.
pub const SETTLE_MS: [u64; 2] = [250, 750];

/// How many of the values Clauseo wrote the backup keeps ([`Backup::history`]).
pub const HISTORY_MAX: usize = 8;

/// The backup's history once `bytes` is written over `before`: `bytes` last, each value once,
/// the oldest dropped past [`HISTORY_MAX`].
pub(super) fn history(before: Option<&Backup>, bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut history: Vec<Vec<u8>> = before.map(|kept| kept.history.clone()).unwrap_or_default();
    history.retain(|old| old != bytes);
    history.push(bytes.to_vec());
    let excess = history.len().saturating_sub(HISTORY_MAX);
    history.drain(..excess);
    history
}

/// The access value to write before the launch value: COM's effective descriptor when there is
/// no value; `None` when there is one (of any type), which is never touched.
pub(super) fn access_to_add(
    store: &mut impl LaunchStore,
) -> Result<Option<Vec<u8>>, (Guard, String)> {
    match store.access() {
        // COM's descriptor may then be the policy's, which must not become the local value.
        Ok(Raw::Absent) if store.access_policy().map_err(|why| (Guard::Failed, why))? => {
            let why = "a Group Policy MachineAccessRestriction is set, so nothing was copied";
            Err((Guard::PolicyOverrides, why.to_owned()))
        }
        Ok(Raw::Absent) => store.effective_access().map(Some).map_err(|why| {
            let why = format!("COM's machine access can't be read, so nothing was written: {why}");
            (Guard::AccessUnknown, why)
        }),
        Ok(Raw::Binary(_) | Raw::OtherType) => Ok(None),
        Err(why) => Err((Guard::Failed, why)),
    }
}

/// What DCOM made of the write, read back last: the launch value and the access value.
pub(super) type ReadBack = (Vec<u8>, Raw);

/// After the launch value was written as `written`: what the last read-back found, only when
/// the deny is still first and the access value still there at each; otherwise
/// [`Guard::Moved`], with why.
pub(super) fn verify(
    store: &mut impl LaunchStore,
    group: &Sid,
    written: &[u8],
) -> Result<ReadBack, (Guard, String)> {
    let failed = |why: String| (Guard::Failed, why);
    let mut last = (written.to_vec(), Raw::Absent);
    for millis in SETTLE_MS {
        store.settle(millis);
        let read = store.read().map_err(failed)?;
        let held = match &read {
            Raw::Binary(bytes) => LaunchSd::parse(bytes).is_ok_and(|sd| sd.denies_first(group)),
            Raw::Absent | Raw::OtherType => false,
        };
        if !held {
            let moved = matches!(store.old(), Ok(Raw::Binary(old)) if Meaning::same(&old, written));
            let why = if moved {
                "dcom moved the value to MachineLaunchRestrictionOld"
            } else {
                "the value didn't hold the deny when read back"
            };
            return Err((Guard::Moved, why.to_owned()));
        }
        let access = store.access().map_err(failed)?;
        if access == Raw::Absent {
            let why = "MachineAccessRestriction is gone, and dcom won't keep the value without it";
            return Err((Guard::Moved, why.to_owned()));
        }
        if let Raw::Binary(bytes) = read {
            last = (bytes, access);
        }
    }
    Ok(last)
}

/// `backup` with what DCOM made of the write (`read`) as Clauseo's own: DCOM merges Windows'
/// default entries into the launch value and re-serialises the access value, and nothing else
/// writes them within the settle. The access value is adopted only when this run wrote it
/// (`wrote_access`). `None` when the read-back is what was written.
pub(super) fn adopt(backup: &Backup, read: ReadBack, wrote_access: bool) -> Option<Backup> {
    let (launch, access) = read;
    let mut adopted = backup.clone();
    if launch != backup.written {
        adopted.history = history(Some(backup), &launch);
        adopted.written = launch;
    }
    if let (true, Raw::Binary(now)) = (wrote_access, access) {
        adopted.access = Some(now);
    }
    (adopted != *backup).then_some(adopted)
}

/// The removal's last part, before the backup goes: any value Clauseo wrote taken out of
/// `MachineLaunchRestrictionOld`, and the access value Clauseo added deleted, each only when
/// byte-identical to what Clauseo wrote; anything else is left and logged.
pub(super) fn remove_extras(
    store: &mut impl LaunchStore,
    backup: Option<&Backup>,
) -> Result<(), StoreError> {
    let Some(backup) = backup else {
        return Ok(());
    };
    let ours = |old: &[u8]| {
        Meaning::same(&backup.written, old) || backup.history.iter().any(|w| Meaning::same(w, old))
    };
    if matches!(store.old()?, Raw::Binary(old) if ours(&old)) {
        store.delete_old()?;
        tracing::info!("Clauseo's value was taken out of MachineLaunchRestrictionOld");
    }
    let Some(added) = &backup.access else {
        return Ok(());
    };
    match store.access()? {
        Raw::Absent => {}
        Raw::Binary(now) if Meaning::same(&now, added) => {
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

/// `History`'s bytes: each value as its length (`u32`, little-endian), then its bytes.
#[cfg(any(windows, test))]
pub(super) fn pack(values: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        out.extend_from_slice(&u32::try_from(value.len()).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(value);
    }
    out
}

/// The values in `History`'s bytes; a cut-short tail is dropped (it only means less of `…Old`
/// is recognised as Clauseo's).
#[cfg(any(windows, test))]
pub(super) fn unpack(mut bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut values = Vec::new();
    while let Some((len, rest)) = bytes.split_first_chunk::<4>() {
        let len = usize::try_from(u32::from_le_bytes(*len)).unwrap_or(usize::MAX);
        let Some((value, rest)) = rest.split_at_checked(len) else {
            break;
        };
        values.push(value.to_vec());
        bytes = rest;
    }
    values
}
