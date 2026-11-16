//! A matter on Clauseo's list inside the session's own matter: Clauseo runs no conversation
//! there (`matter.holds_other_matter`). Every way through Clauseo refuses that layout (New
//! matter, import, Locate, scan); it only comes from folders moved outside Clauseo, and then an
//! agent in the outer matter could rename a folder on the way down to the inner one, so the
//! fence's paths, made from the folders as they were, no longer name it.
//!
//! - Checked at every launch (a resume too) on the fence as resolved ([`held`]), and before
//!   every prompt to a live agent with each folder resolved again ([`recheck`]), so a layout
//!   that appears while a conversation is open stops its next turn.
//! - The same paths as the fence's: each spelling of the other matter's folder (through links,
//!   as `super::spellings` walks it) against each of the own matter's (as stored, as the agent
//!   works in it, and canonical), names compared through [`super::key_as`], case folded where
//!   the file system folds it. A matter whose folder merely starts with the same letters
//!   (`Acme2` beside `Acme`) is not inside.
//! - Invited or not. A forgotten matter does not count, and neither does the conversation's own;
//!   a conversation in the inner matter runs, since the outer one is not inside it.
//! - The fence's nested handling (`MatterFence::own_shut`, `NoticeKind::ShellLimited`) stays as
//!   the second layer.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, HostError};

use super::{MatterFence, Other, key_as, spellings};
use crate::error::Error;
use crate::network_paths::resolve::Disk;
use crate::network_paths::{FOLD_CASE, NetEnv};

/// The first matter on the list (by id) inside the own matter, as the launch resolved them.
pub(crate) fn held(fence: &MatterFence) -> Option<&Other> {
    held_as(&fence.others, &fence.own, FOLD_CASE)
}

/// The first matter of `others` on the list with a spelling inside one of `own`, names folded
/// when `insensitive`.
pub(crate) fn held_as<'a>(
    others: &'a [Other],
    own: &[PathBuf],
    insensitive: bool,
) -> Option<&'a Other> {
    let inside = |path: &Path, place: &Path| {
        let (path, place) = (key_as(path, insensitive), key_as(place, insensitive));
        path.len() > place.len() && path.starts_with(&place)
    };
    others.iter().filter(|other| other.listed).find(|other| {
        (other.spellings.iter()).any(|spelling| own.iter().any(|place| inside(spelling, place)))
    })
}

/// The own matter folder `stored`, each way the launch spells it (`launch_checks::folders`): as
/// the agent works in it, as stored, and canonical; never canonical on a network location.
fn own_places(stored: &Path, net: &NetEnv) -> Vec<PathBuf> {
    let cwd = crate::agent::session_folder(stored);
    let local = !net.is_network(&cwd) && !net.is_network(stored);
    let real = local.then(|| dunce::canonicalize(&cwd).ok()).flatten();
    let mut places = vec![cwd, stored.to_owned()];
    places.extend(real);
    places
}

/// [`held`] with every folder resolved again now: `fence`'s other matters (as the launch asked
/// them) and the own matter folder `stored`. Reads the file system: off the actor, waited on
/// for `codex::prepare::PREPARE_WAIT` (5 s) before each prompt; one that does not answer in
/// time lets the prompt through (`actor::turn::prompt`).
///
/// What it touches: the mount table once (`NetEnv::current`), then for each other matter
/// `touchable`'s walk: an `lstat` of each name on the way, and a `readlink` of each link,
/// each checked as text against the network locations first, so a folder on a share (or a
/// link into one) is never touched and counts as stored. No folder is listed or opened for
/// reading, so a cloud folder (File Provider, `OneDrive`) is not downloaded; on Windows the
/// walk's canonical form opens a handle to the path, as the launch's does. About ten calls
/// per matter: well under a millisecond each on a local disk, for 50 matters a few ms.
pub(crate) fn recheck(fence: &MatterFence, stored: &Path) -> Result<(), Error> {
    let net = NetEnv::current();
    let disk = Disk::resolved(&net);
    let mut others = fence.others.clone();
    for other in &mut others {
        other.spellings = spellings(&other.folder, &disk);
    }
    match held_as(&others, &own_places(stored, &net), FOLD_CASE) {
        Some(other) => Err(refusal(other)),
        None => Ok(()),
    }
}

/// `matter.holds_other_matter`, naming `other`: its name is for the notice, never the log.
pub(crate) fn refusal(other: &Other) -> Error {
    tracing::warn!(matter = %other.id, "a matter on the list lies inside this one; no turn runs");
    let error = HostError::new(
        ErrorCode::MatterHoldsOtherMatter,
        "another matter on Clauseo's list lies inside this matter's folder",
    );
    Error::Refused(error.with_matter(other.id))
}

#[cfg(test)]
mod tests;
