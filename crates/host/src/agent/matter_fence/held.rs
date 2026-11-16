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
use std::sync::Arc;

use clauseo_protocol::{ErrorCode, HostError, MatterId};

use super::{MatterFence, Other, key_as, spellings};
use crate::clock::Clock;
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
/// the agent works in it, as stored, and canonical; never canonical where [`walks`] says no.
fn own_places(stored: &Path, net: &NetEnv) -> Vec<PathBuf> {
    let cwd = crate::agent::session_folder(stored);
    let local = walks(&cwd, net) && walks(stored, net);
    let real = local.then(|| dunce::canonicalize(&cwd).ok()).flatten();
    let mut places = vec![cwd, stored.to_owned()];
    places.extend(real);
    places
}

/// [`held`] with every folder resolved again now: `fence`'s other matters on the list (as the
/// launch asked them) and the own matter folder `stored`. Reads the file system: off the actor.
///
/// What it touches: the mount table once (`NetEnv::current`), and nothing for a folder whose
/// text lies on a network location (a share, a remote mount, a mapped drive) or a cloud folder
/// ([`cloud`]: File Provider's `Library/CloudStorage`, iCloud's `Library/Mobile Documents`,
/// `OneDrive`): those count as stored, decided from the text alone. Any other folder gets
/// `touchable`'s walk: an `lstat` of each name on the way and a `readlink` of each link (each
/// target checked as text first), never a listing or a read; on Windows the walk's canonical
/// form opens a handle to the path, as the launch does. About ten calls per matter on a local
/// disk: for 50 matters a few ms.
pub(crate) fn recheck(fence: &MatterFence, stored: &Path) -> Result<(), Error> {
    let net = NetEnv::current();
    let disk = Disk::resolved(&net);
    let mut others: Vec<Other> = fence.others.iter().filter(|o| o.listed).cloned().collect();
    for other in &mut others {
        other.spellings = if walks(&other.folder, &net) {
            spellings(&other.folder, &disk)
        } else {
            vec![other.folder.clone()]
        };
    }
    match held_as(&others, &own_places(stored, &net), FOLD_CASE) {
        Some(other) => Err(refusal(other)),
        None => Ok(()),
    }
}

/// Whether the re-check may walk `folder` on the disk: not on a network location, nor in a cloud
/// folder ([`cloud`]), both judged from the text and the mount table only.
pub(crate) fn walks(folder: &Path, net: &NetEnv) -> bool {
    folder.is_absolute() && !net.is_network(folder) && !cloud(folder)
}

/// Whether `folder`'s text lies in a cloud folder whose names a walk could make its provider
/// fetch: under `Library/CloudStorage` or `Library/Mobile Documents` (macOS File Provider,
/// iCloud Drive), or under a `OneDrive` folder the environment names (Windows). Text only.
pub(crate) fn cloud(folder: &Path) -> bool {
    let names = key_as(folder, true);
    let library = |second: &str| {
        names.windows(2).any(|pair| {
            pair.first().is_some_and(|n| n == "library") && pair.get(1).is_some_and(|n| n == second)
        })
    };
    if library("cloudstorage") || library("mobile documents") {
        return true;
    }
    ["OneDrive", "OneDriveCommercial", "OneDriveConsumer"]
        .iter()
        .any(|var| {
            std::env::var_os(var).is_some_and(|root| {
                let root = key_as(Path::new(&root), true);
                !root.is_empty() && names.starts_with(&root)
            })
        })
}

/// The re-check before a prompt to a live agent, waited on for `codex::prepare::PREPARE_WAIT`
/// (5 s) off the actor: a refusal, or, failing closed, `agent.launch_failed` (retryable, the
/// question kept) when the check does not answer in time or its thread ends without an answer
/// (a panic). No thread when no matter on the list could be inside. The wait is on the system
/// clock, not the host's: a test's manual clock moving past it would fail a turn the disk
/// answered in time.
pub(crate) async fn recheck_off_actor(
    matter: Option<MatterId>,
    fence: MatterFence,
    stored: PathBuf,
) -> Result<(), Error> {
    if !fence.others.iter().any(|other| other.listed) {
        return Ok(());
    }
    let clock: Arc<dyn Clock> = Arc::new(crate::clock::SystemClock);
    wait(clock, matter, move || recheck(&fence, &stored)).await
}

/// `work` off the actor for at most `PREPARE_WAIT`, failing closed.
async fn wait(
    clock: Arc<dyn Clock>,
    matter: Option<MatterId>,
    work: impl FnOnce() -> Result<(), Error> + Send + 'static,
) -> Result<(), Error> {
    let job = crate::agent::codex::prepare::Job::short("the matter-inside re-check", matter);
    crate::agent::codex::prepare::off_actor(clock, job, work).await
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
