//! One item moved into a pass's folder, written to the manifest as an intent before the move
//! and finished after it, and each load of the manifest settling an intent a crash left (N10),
//! so every item in a pass's folder has a finished entry. The matter's lock is held throughout.
//! Each item moved is then handed to the desktop user (S15 `PR4a2`).

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use super::files::{Manifest, Place, io_failure};
use super::run::{Refused, Volume, one_part};
use super::time::{rfc3339, timestamp};
use super::{Entry, Reason, SetAsideFailure};

/// How many names an item tries in its folder: its own, then `~2` up to `~9`.
const NAMES_TRIED: u32 = 9;

/// Moves `from` into the folder `within` in the pass's folder for `now`, made as needed, under
/// its own name or the first free `~n` one, and adds its manifest entry: an intent first, then,
/// once the move is done, the finished entry. Names are kept as they are, text or not. A
/// failure's `at` is `from`.
pub(super) fn set_aside(
    volume: &impl Volume,
    place: &Place,
    from: &Path,
    within: &[OsString],
    reason: Reason,
    now: SystemTime,
) -> Result<Entry, Refused> {
    let refuse = |failure| Refused::at(failure, from);
    let own = from
        .file_name()
        .filter(|name| one_part(name))
        .ok_or_else(|| refuse(SetAsideFailure::Io(io::ErrorKind::InvalidInput)))?;
    if !within.iter().all(|part| one_part(part)) {
        return Err(refuse(SetAsideFailure::Io(io::ErrorKind::InvalidInput)));
    }
    let owner = volume.owner(from).ok();
    let folder = timestamp(now);
    let parts = place.parts(&folder, within);
    let mut manifest = load(volume, place).map_err(|refused| refuse(refused.failure))?;
    let id = manifest.next;
    manifest.next += 1;
    for n in 1..=NAMES_TRIED {
        let mut name = own.to_owned();
        if n > 1 {
            name.push(format!("~{n}"));
        }
        let to = place.to(&folder, within, &name);
        match volume.file_id(&to) {
            Ok(None) => {}
            Ok(Some(_)) => continue,
            Err(error) => return Err(refuse(io_failure(&error))),
        }
        let entry = Entry {
            id,
            matter: place.matter(),
            from: from.to_owned(),
            to,
            folder: folder.clone(),
            within: within.to_vec(),
            name,
            reason,
            owner: owner.clone(),
            at: rfc3339(now),
        };
        manifest.intents.push(entry.clone());
        place.write_manifest(&manifest).map_err(refuse)?;
        let result = volume.move_into(from, &place.sandbox, &parts, &entry.name);
        manifest.intents.retain(|intent| intent.id != id);
        if result.is_ok() {
            manifest.entries.push(entry.clone());
        }
        place.write_manifest(&manifest).map_err(refuse)?;
        match result {
            Ok(()) => {
                tracing::info!(?reason, "a protected name was set aside");
                hand_back(volume, &entry.to);
                return Ok(entry);
            }
            Err(SetAsideFailure::NameTaken) => {}
            Err(other) => return Err(refuse(other)),
        }
    }
    Err(refuse(SetAsideFailure::NameTaken))
}

/// The item moved to `to` handed to the desktop user when another user owns it (S15 `PR4a2`):
/// an item the agent made would otherwise stay its pool user's, and once that user serves
/// another matter its agent would keep the owner's `WRITE_DAC` on it, and could open it by its
/// full path. The entry keeps the owner it had, for the lawyer to see who made it. It runs once
/// the move is done and its entry finished: a change of owner moves nothing, so a crash here
/// leaves the item set aside and listed. One that fails is logged; the item stays set aside.
fn hand_back(volume: &impl Volume, to: &Path) {
    if let Err(error) = volume.own(to) {
        let error = error.kind();
        tracing::warn!(%error, "a set-aside item wasn't handed to the desktop user");
    }
}

/// The manifest, each intent a crash left settled against the pass's folder and the result
/// saved: the item at its new place finishes the entry; the item still at its old place means
/// the move never happened, and the intent is dropped; neither drops it with a warning.
pub(super) fn load(volume: &impl Volume, place: &Place) -> Result<Manifest, Refused> {
    let mut manifest = place.read_manifest()?;
    if manifest.intents.is_empty() {
        return Ok(manifest);
    }
    for intent in std::mem::take(&mut manifest.intents) {
        if present(volume, &intent.to)? {
            tracing::info!("a set-aside move a crash interrupted had finished; it is listed");
            manifest.entries.push(intent);
        } else if present(volume, &intent.from)? {
            tracing::info!("a set-aside move a crash interrupted never happened; dropped");
        } else {
            tracing::warn!(
                id = intent.id,
                "a set-aside move a crash interrupted left the item in neither place; dropped"
            );
        }
    }
    manifest.entries.sort_by_key(|entry| entry.id);
    place.write_manifest(&manifest)?;
    Ok(manifest)
}

/// Whether something is at `path`: one whose ID the lawyer can't read is there too.
fn present(volume: &impl Volume, path: &Path) -> Result<bool, Refused> {
    match volume.file_id(path) {
        Ok(id) => Ok(id.is_some()),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Ok(true),
        Err(error) => Err(Refused::at(io_failure(&error), path)),
    }
}
