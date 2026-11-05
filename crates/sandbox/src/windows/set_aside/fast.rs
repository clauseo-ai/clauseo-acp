//! The set-aside's side of a fast later launch (S15 `PR3d`): the lock the clean-close record is
//! read, removed and written under, and the launch pass that keeps the last record when the
//! journal says nothing in the matter changed since a clean close, or re-probes only the
//! folders it says changed.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use clauseo_protocol::MatterId;

use super::check::snapshot;
use super::files::{Place, Record, io_failure};
use super::run::{Refused, Volume};
use super::scan::{Scan, scan_part};
use super::{Checked, SetAsideFailure};
use crate::windows::clean_close;

/// Runs `f` under `matter`'s set-aside lock, telling it whether any session of the matter is
/// open: launched with no after-session pass yet. A record that can't be read counts as open.
pub(crate) fn under_lock<R>(
    data_dir: &Path,
    matter: &MatterId,
    f: impl FnOnce(bool) -> R,
) -> Result<R, SetAsideFailure> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    let open = match place.read_record() {
        Ok(Some(record)) => !record.sessions.is_empty(),
        Ok(None) => false,
        Err(_unreadable) => true,
    };
    Ok(f(open))
}

/// Removes `matter`'s clean-close record; the caller holds the matter's lock. Every launch
/// does, before it adds its session, so no record is ever left while an agent runs.
pub(super) fn spend_clean_close(data_dir: &Path, matter: &MatterId) -> Result<(), SetAsideFailure> {
    clean_close::remove_record(data_dir, matter).map_err(|e| io_failure(&e))
}

/// Records a session of `matter` as open, with nothing else, for the tests.
#[cfg(test)]
pub(crate) fn open_session(data_dir: &Path, matter: &MatterId, session: &str) {
    let place = Place::new(data_dir, matter);
    let _held = place.lock().unwrap();
    let sessions = BTreeSet::from([session.to_owned()]);
    let seen = super::files::Snapshot::default();
    place
        .write_record(&Record::new(*matter, sessions, seen))
        .unwrap();
}

/// A folder the journal says changed, below the matter's root: its parts, and whether
/// everything below it is scanned again (`true`) or only its own names.
pub(crate) type Touched = (Vec<OsString>, bool);

/// The launch pass when the launch check found no change in the matter since a clean close
/// (`touched` empty), or changes in only these folders. Each touched folder is scanned again
/// and its part of the record replaced; the rest of the record is kept. `Ok(None)` when it
/// can't: no record, a session open (so the full pass checks it), or a touched folder that
/// can't be opened. The caller then runs the full pass.
pub(super) fn before_launch_fast(
    volume: &impl Volume,
    root: &Path,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    touched: &[Touched],
) -> Result<Option<Checked>, Refused> {
    let mut scans: Vec<(&Touched, Option<Scan>)> = Vec::new();
    for each in touched {
        match scan_part(volume, root, &each.0, each.1) {
            Ok(scan) => scans.push((each, scan)),
            Err(error) => {
                tracing::debug!(kind = %error.kind(), "a touched folder couldn't be scanned");
                return Ok(None);
            }
        }
    }
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    spend_clean_close(data_dir, matter)?;
    let Some(record) = place.read_record()? else {
        return Ok(None);
    };
    if !record.sessions.is_empty() {
        return Ok(None);
    }
    let mut seen = super::files::Snapshot {
        names: record.names,
        links: record.links,
    };
    let mut unscanned = Vec::new();
    for ((parts, below), scan) in &scans {
        // A folder gone takes everything below it from the record. A re-probe of a folder's own
        // names replaces them, and drops what the record holds under a folder in it that is no
        // longer there (renamed or removed), so a name moved back later is new again.
        let replaced = |key: &Vec<OsString>| {
            if !key.starts_with(parts) || key.len() <= parts.len() {
                return false;
            }
            match scan {
                None => true,
                Some(_) if *below => true,
                Some(_) if key.len() == parts.len() + 1 => true,
                Some(scan) => key
                    .get(..=parts.len())
                    .is_none_or(|within| !scan.folders.iter().any(|f| f.as_slice() == within)),
            }
        };
        seen.names.retain(|key, _| !replaced(key));
        seen.links.retain(|key, _| !replaced(key));
        if let Some(scan) = scan {
            let fresh = snapshot(scan, &[])?;
            seen.names.extend(fresh.names);
            seen.links.extend(fresh.links);
            unscanned.extend(scan.unscanned.iter().cloned());
        }
    }
    let sessions = BTreeSet::from([session.to_owned()]);
    place.write_record(&Record::new(*matter, sessions, seen))?;
    Ok(Some(Checked {
        moved: Vec::new(),
        unscanned,
    }))
}
