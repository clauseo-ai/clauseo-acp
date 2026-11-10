//! The watch of the open matter's folder, as the actor keeps it (decision record 0055), on macOS
//! and Linux; Windows builds `no_watch.rs` instead. One matter at a time: the one last opened or
//! listed. Each start has its own generation, and what a start or a burst posts for another
//! generation (a watch since replaced) is dropped.
//!
//! - A start is `Starting` until its thread answers ([`on_started`]): `Live`, `Gone` (the folder
//!   is not there) or `Failed` (the OS refused). A failed start is not tried again until another
//!   folder or matter is followed, so a folder past inotify's limit is not walked at each listing.
//! - A folder gone (`Gone`) is watched again when a listing finds it there
//!   ([`super::follow::listed`]).
//! - A folder swapped for another at the same path gets a new watch.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use clauseo_protocol::MatterId;

use super::follow;
use super::watch::{self, FolderChanged, Look, Started, Watch, WatchStarted};
use crate::actor::Actor;

/// The next start's generation.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// The followed matter, its folder, and how its watch stands.
pub(crate) struct Follow {
    matter: MatterId,
    folder: std::path::PathBuf,
    generation: u64,
    state: State,
    /// The watch while `Live`; dropping it ends it.
    watch: Option<Watch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Starting,
    Live,
    Gone,
    Failed,
}

impl Follow {
    /// In tests: whether the watch is live.
    #[cfg(test)]
    pub(crate) fn is_live(&self) -> bool {
        self.state == State::Live
    }
}

/// Follows `matter`'s `folder`, in place of any other; one already followed stays as it is.
pub(super) fn follow(actor: &mut Actor, matter: MatterId, folder: &Path) {
    let same = actor
        .papers_follow
        .as_ref()
        .is_some_and(|follow| follow.matter == matter && follow.folder == folder);
    if !same {
        start(actor, matter, folder);
    }
}

/// `matter` now lives at `folder`: a new watch there if it is the one followed.
pub(super) fn relocated(actor: &mut Actor, matter: MatterId, folder: &Path) {
    if follows(actor, matter) {
        start(actor, matter, folder);
    }
}

/// `matter` left the list: its watch ends.
pub(super) fn forgotten(actor: &mut Actor, matter: MatterId) {
    if follows(actor, matter) {
        actor.papers_follow = None;
    }
}

/// A listing found `matter`'s `folder`: watched again if it had gone.
pub(super) fn found(actor: &mut Actor, matter: MatterId, folder: &Path) {
    let gone = actor.papers_follow.as_ref().is_some_and(|follow| {
        follow.matter == matter && follow.folder == folder && follow.state == State::Gone
    });
    if gone {
        start(actor, matter, folder);
    }
}

/// A start's thread answered.
pub(crate) fn on_started(actor: &mut Actor, started: WatchStarted) {
    let WatchStarted {
        generation,
        started,
    } = started;
    // One for a watch since replaced drops here, and its feed with it.
    let Some(follow) = current(actor, generation) else {
        return;
    };
    (follow.state, follow.watch) = match started {
        Started::Live(watch) => (State::Live, Some(watch)),
        Started::Missing => (State::Gone, None),
        Started::Failed => (State::Failed, None),
    };
}

/// The watch saw a quiet change: the app lists the papers again, and a folder that went reads
/// `missing`.
pub(crate) fn on_changed(actor: &mut Actor, changed: FolderChanged) {
    let FolderChanged {
        matter,
        generation,
        look,
    } = changed;
    let Some(follow) = current(actor, generation) else {
        return;
    };
    match look {
        Look::Found => {}
        Look::Missing => {
            follow.state = State::Gone;
            // The feed of a folder that went ends here.
            drop(follow.watch.take());
        }
        Look::Replaced => {
            let folder = follow.folder.clone();
            start(actor, matter, &folder);
        }
    }
    follow::changed(actor, matter, look != Look::Missing);
}

fn current(actor: &mut Actor, generation: u64) -> Option<&mut Follow> {
    actor
        .papers_follow
        .as_deref_mut()
        .filter(|follow| follow.generation == generation)
}

fn follows(actor: &Actor, matter: MatterId) -> bool {
    actor
        .papers_follow
        .as_ref()
        .is_some_and(|follow| follow.matter == matter)
}

/// A new watch of `folder`, in place of any other; the old one ends first.
fn start(actor: &mut Actor, matter: MatterId, folder: &Path) {
    actor.papers_follow = None;
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    let (post, clock) = (actor.post.clone(), std::sync::Arc::clone(&actor.clock));
    watch::start(matter, folder.to_path_buf(), generation, post, clock);
    actor.papers_follow = Some(Box::new(Follow {
        matter,
        folder: folder.to_path_buf(),
        generation,
        state: State::Starting,
        watch: None,
    }));
}
