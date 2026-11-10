//! Which matter's folder the host follows (decision record 0055): the one the lawyer last opened
//! (`OpenMatter`) or the app last listed (`ListPapers`: the matter the app reopens at launch is
//! listed, never opened), one at a time. Opening or listing another matter, forgetting the
//! matter, and the host's end drop the watch; a relocate moves it to the new folder. How the
//! watch starts, fails and restarts is `live.rs`'s.
//!
//! Not on Windows ([`live_watch`]): there the watch's handle would stop the lawyer renaming or
//! moving the folder that holds the matter's folder, so nothing is watched, and the app lists
//! the papers again when its window comes back to the front and when each answer ends
//! ([`on_turn_ended`]).
//!
//! Each quiet change the watch posts sends `PapersChanged` to the app, which lists the papers
//! again; a folder that went also reads `missing` in the matter's next row, so the room shows the
//! moved folder's line with Locate folder. The watch knows only that something changed, not
//! which papers, so it calls no `scan::rescan`: the listing the app makes next hands each new or
//! changed `.docx` to the background tracked-changes scan (`scan::listed`), which sends
//! `PapersChanged` again if it finds marks. The Find index keeps the names of the last
//! checkpoint, as before.

use std::path::{Path, PathBuf};

use clauseo_protocol::{FolderState, HostEvent, MatterId};

use super::live;
use crate::actor::Actor;

/// Whether the open matter's folder is watched on this OS: not on Windows, where the watch's
/// handle on the folder would stop the lawyer renaming or moving the folder that holds it. The
/// actor keeps it as `papers_live`, which tests set.
pub(crate) const fn live_watch() -> bool {
    cfg!(not(windows))
}

/// `OpenMatter` or `ListPapers` of `matter`: follow its folder, in place of any other.
pub(crate) fn opened(actor: &mut Actor, matter: MatterId) {
    if !actor.papers_live {
        return;
    }
    if let Some(folder) = folder_of(actor, matter) {
        live::follow(actor, matter, &folder);
    }
}

/// `matter` was relocated: watch its new folder if it is the one followed, and have the app
/// list its papers there.
pub(crate) fn relocated(actor: &mut Actor, matter: MatterId) {
    if actor.papers_live
        && let Some(folder) = folder_of(actor, matter)
    {
        live::relocated(actor, matter, &folder);
    }
    crate::actor::app::publish(actor, &HostEvent::PapersChanged { matter });
}

/// `matter` left the list: stop watching it.
pub(crate) fn forgotten(actor: &mut Actor, matter: MatterId) {
    live::forgotten(actor, matter);
}

/// A listing of `matter`'s `folder` answered: what it found goes in the matter's row, and a
/// followed folder found again is watched again. A listing of a folder the row no longer names
/// (a relocate in between) says nothing.
pub(crate) fn listed(actor: &mut Actor, matter: MatterId, folder: &Path, found: bool) {
    if folder_of(actor, matter).as_deref() != Some(folder) {
        return;
    }
    mark(actor, matter, found);
    if found && actor.papers_live {
        live::found(actor, matter, folder);
    }
}

/// A turn in `matter` ended, however it ended. With no watch (Windows) the app lists the papers
/// again, since the answer may have added or changed some; with one, the watch tells it.
pub(crate) fn on_turn_ended(actor: &mut Actor, matter: MatterId) {
    if !actor.papers_live && actor.matters.contains_key(&matter) {
        crate::actor::app::publish(actor, &HostEvent::PapersChanged { matter });
    }
}

/// The watch saw `matter`'s folder change: `found` or not goes in its row, and the app lists the
/// papers again.
#[cfg(not(windows))]
pub(super) fn changed(actor: &mut Actor, matter: MatterId, found: bool) {
    mark(actor, matter, found);
    crate::actor::app::publish(actor, &HostEvent::PapersChanged { matter });
}

/// `matter`'s folder state as found: `missing`, or `found` again; the rows go out at the next
/// flush when it changed.
fn mark(actor: &mut Actor, matter: MatterId, found: bool) {
    if !actor.matters.contains_key(&matter) {
        return;
    }
    let before = actor
        .folder_states
        .get(&matter)
        .copied()
        .unwrap_or(FolderState::Found);
    let now = if found {
        FolderState::Found
    } else {
        FolderState::Missing
    };
    if before == now {
        return;
    }
    if found {
        actor.folder_states.remove(&matter);
    } else {
        actor.folder_states.insert(matter, now);
    }
    actor.lists.touch_all();
}

fn folder_of(actor: &Actor, matter: MatterId) -> Option<PathBuf> {
    actor
        .matters
        .get(&matter)
        .map(|entry| PathBuf::from(&entry.row.folder_path))
}
