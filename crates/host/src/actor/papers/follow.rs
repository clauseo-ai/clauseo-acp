//! Which matter's folder the host watches (decision record 0055): the one the lawyer last
//! opened (`OpenMatter`), one at a time. Opening another matter, forgetting the matter, and the
//! host's end drop the watch; a relocate moves it to the new folder. A folder that cannot be
//! watched (gone, or the OS refused the watch) is logged and left unwatched: the app lists the
//! papers again when its window comes back to the front, and a listing that finds the folder
//! there again starts the watch.
//!
//! Each quiet change the watch posts ([`on_changed`]) sends `PapersChanged` to the app, which
//! lists the papers again; a folder that went also reads `missing` in the matter's next row, so
//! the room shows the moved folder's line with Locate folder, and the watch stops (on Windows its
//! handle would hold the folder's delete open). The `tracked` marks follow, as each listing
//! scans them; the Find index keeps the names of the last checkpoint, as before.

use std::path::PathBuf;

use clauseo_protocol::{FolderState, HostEvent, MatterId};

use super::watch::{FolderChanged, Watch};
use crate::actor::Actor;

/// The watched matter, and its watch while there is one.
pub(crate) struct Follow {
    matter: MatterId,
    watch: Option<Watch>,
}

/// `OpenMatter` of `matter`: watch its folder, in place of any other.
pub(crate) fn opened(actor: &mut Actor, matter: MatterId) {
    let Some(folder) = folder_of(actor, matter) else {
        return;
    };
    if let Some(follow) = &actor.papers_follow
        && follow.matter == matter
        && follow
            .watch
            .as_ref()
            .is_some_and(|watch| watch.folder == folder)
    {
        return;
    }
    // The old watch ends before the new one starts.
    actor.papers_follow = None;
    let watch = start(actor, matter, folder);
    actor.papers_follow = Some(Box::new(Follow { matter, watch }));
}

/// `matter` was relocated: watch its new folder if it is the one followed, and have the app
/// list its papers there.
pub(crate) fn relocated(actor: &mut Actor, matter: MatterId) {
    if follows(actor, matter) {
        restart(actor, matter);
    }
    crate::actor::app::publish(actor, &HostEvent::PapersChanged { matter });
}

/// `matter` left the list: stop watching it.
pub(crate) fn forgotten(actor: &mut Actor, matter: MatterId) {
    if follows(actor, matter) {
        actor.papers_follow = None;
    }
}

/// A listing of `matter` answered: what it found of the folder goes in the matter's row, and a
/// followed folder found again is watched again.
pub(crate) fn listed(actor: &mut Actor, matter: MatterId, found: bool) {
    mark(actor, matter, found);
    let unwatched = actor
        .papers_follow
        .as_ref()
        .is_some_and(|follow| follow.matter == matter && follow.watch.is_none());
    if found && unwatched {
        restart(actor, matter);
    }
}

/// The watch saw a quiet change. One from a watch since replaced is dropped.
pub(crate) fn on_changed(actor: &mut Actor, changed: FolderChanged) {
    let FolderChanged {
        matter,
        folder,
        found,
    } = changed;
    let current = actor.papers_follow.as_ref().is_some_and(|follow| {
        follow.matter == matter
            && follow
                .watch
                .as_ref()
                .is_some_and(|watch| watch.folder == folder)
    });
    if !current || folder_of(actor, matter).as_ref() != Some(&folder) {
        return;
    }
    if !found && let Some(follow) = actor.papers_follow.as_mut() {
        follow.watch = None;
    }
    mark(actor, matter, found);
    crate::actor::app::publish(actor, &HostEvent::PapersChanged { matter });
}

fn follows(actor: &Actor, matter: MatterId) -> bool {
    actor
        .papers_follow
        .as_ref()
        .is_some_and(|follow| follow.matter == matter)
}

fn restart(actor: &mut Actor, matter: MatterId) {
    if let Some(follow) = actor.papers_follow.as_mut() {
        follow.watch = None;
    }
    let Some(folder) = folder_of(actor, matter) else {
        return;
    };
    let watch = start(actor, matter, folder);
    if let Some(follow) = actor.papers_follow.as_mut() {
        follow.watch = watch;
    }
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

fn start(actor: &Actor, matter: MatterId, folder: PathBuf) -> Option<Watch> {
    match Watch::start(matter, folder, actor.post.clone()) {
        Ok(watch) => Some(watch),
        Err(error) => {
            tracing::info!(
                %matter,
                // The kind alone: the error's paths would put the folder in the log.
                kind = ?error.kind,
                "the matter's folder is not watched; its papers are listed again on focus"
            );
            None
        }
    }
}
