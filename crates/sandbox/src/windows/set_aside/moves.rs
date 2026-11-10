//! One item moved into a pass's folder, written to the manifest as an intent before the move
//! and finished after it, and each load of the manifest settling an intent a crash left (N10),
//! so every item in a pass's folder has a finished entry. The matter's lock is held throughout.
//! Each item moved is then handed to the desktop user (S15 `PR4a2`).

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use super::files::{Manifest, Place, io_failure};
use super::run::{Need, Refused, Volume, items_root, one_part};
use super::time::{rfc3339, timestamp};
use super::{Entry, Reason, SetAsideFailure};

/// How many names an item tries in its folder: its own, then `~2` up to `~9`.
const NAMES_TRIED: u32 = 9;

/// Moves `from` into the folder `within` in the pass's folder for `now`, made as needed, under
/// its own name or the first free `~n` one, and adds its manifest entry: an intent first, then,
/// once the move is done, the finished entry. Names are kept as they are, text or not. The
/// chain starts at the item's folder's set-aside folder ([`items_root`]: the sandbox folder, or
/// the one on the item's volume, made or checked first). A failure's `at` is `from`. The entry's
/// owner is the one read from the handle the move renamed ([`Volume::move_into`], which refuses
/// an item `need` doesn't allow); the intent's, read by path before, is for a crash alone.
pub(super) fn set_aside(
    volume: &impl Volume,
    place: &Place,
    from: &Path,
    within: &[OsString],
    reason: Reason,
    now: SystemTime,
    need: Need<'_>,
) -> Result<Entry, Refused> {
    let owner = volume.owner(from).ok();
    set_aside_owned(volume, place, (from, within, owner), reason, now, need)
}

/// [`set_aside`], with the intent's owner read by the caller: the live watch reads it through
/// handles from the watched folder (`Volume::owner_at`), never by the item's full path, which
/// would follow a link partway (`live.rs`).
pub(super) fn set_aside_owned(
    volume: &impl Volume,
    place: &Place,
    (from, within, owner): (&Path, &[OsString], Option<String>),
    reason: Reason,
    now: SystemTime,
    need: Need<'_>,
) -> Result<Entry, Refused> {
    let refuse = |failure| Refused::at(failure, from);
    let own = from
        .file_name()
        .filter(|name| one_part(name))
        .ok_or_else(|| refuse(SetAsideFailure::Io(io::ErrorKind::InvalidInput)))?;
    if !within.iter().all(|part| one_part(part)) {
        return Err(refuse(SetAsideFailure::Io(io::ErrorKind::InvalidInput)));
    }
    let source = from
        .parent()
        .ok_or_else(|| refuse(SetAsideFailure::Io(io::ErrorKind::InvalidInput)))?;
    let root = items_root(volume, place, source).map_err(|refused| refuse(refused.failure))?;
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
        let to = place.to(&root, &folder, within, &name);
        match volume.file_id(&to) {
            Ok(None) => {}
            Ok(Some(_)) => continue,
            Err(error) => return Err(refuse(io_failure(&error))),
        }
        let mut entry = Entry {
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
        let result = volume.move_into(from, &root, &parts, &entry.name, need);
        manifest.intents.retain(|intent| intent.id != id);
        if let Ok(owned) = &result {
            entry.owner.clone_from(owned);
            manifest.entries.push(entry.clone());
        }
        place.write_manifest(&manifest).map_err(refuse)?;
        match result {
            Ok(_) => {
                tracing::info!(?reason, "a protected name was set aside");
                hand_over(volume, &entry.to, "a set-aside item");
                return Ok(entry);
            }
            Err(SetAsideFailure::NameTaken) => {}
            Err(other) => return Err(refuse(other)),
        }
    }
    Err(refuse(SetAsideFailure::NameTaken))
}

/// The item at `to` (`what`, for the log) handed to the desktop user, its whole tree, wherever
/// a pool user owns a part of it ([`Volume::own`], S15 `PR4a2`): what the agent made would
/// otherwise stay its pool user's, even inside a lawyer's folder it renamed to `.git`, and once
/// that user serves another matter its agent would keep the owner's `WRITE_DAC` there and could
/// open it by its full path. The entry keeps the owner the top had, for the lawyer to see who
/// made it. It runs once the move is done and its entry finished: a change of owner moves
/// nothing, so a crash here leaves the item set aside and listed.
///
/// Only an item a pool SID the machine's record names ([`Volume::pool`]) owns is handed over,
/// never one anyone else owns: the agent can hard-link a colleague's file into an item to be set
/// aside, and that file must keep its owner and DACL. With the record unreadable or empty
/// nothing is handed over and that is logged; the item stays listed, and the reuse hold (`PR4c`)
/// keeps its owner from serving another matter. A hand-over that fails is logged the same way.
pub(super) fn hand_over(volume: &impl Volume, to: &Path, what: &str) {
    let pool = match volume.pool() {
        Ok(pool) if !pool.is_empty() => pool,
        Ok(_) => {
            tracing::warn!(
                what,
                "no pool users recorded; nothing handed to the desktop user"
            );
            return;
        }
        Err(error) => {
            let error = error.kind();
            tracing::warn!(%error, what, "the pool record is unreadable; nothing handed over");
            return;
        }
    };
    if let Err(error) = volume.own(to, &pool) {
        let error = error.kind();
        tracing::warn!(%error, what, "an item wasn't handed to the desktop user");
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
