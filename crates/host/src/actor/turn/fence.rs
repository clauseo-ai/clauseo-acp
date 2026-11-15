//! The fence between matters, read on the actor: which other matters a launch of a thread
//! fences and which it is invited into (`agent::matter_fence`), and the key a live agent's
//! launch took of them, which a change to the matter list or to the thread's invites makes
//! stale (`idle::fence`, which stops the agent once it is idle so its next prompt relaunches it
//! with the new rules).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use clauseo_protocol::{HeaderEntry, MatterId, StreamId, ThreadId};

use super::super::Actor;
use crate::agent::matter_fence::{FenceKey, MatterFence, Other};

/// The thread's header: the loaded stream's, else the one kept when it was unloaded.
fn header(actor: &Actor, thread: ThreadId) -> Option<&HeaderEntry> {
    actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(actor.threads.get(&thread)?.header.as_ref())
}

/// The other matters the lawyer let `thread` read, from its header, oldest first.
pub(crate) fn read_matters(actor: &Actor, thread: ThreadId) -> Vec<MatterId> {
    header(actor, thread).map_or_else(Vec::new, |header| header.read_matters.clone())
}

/// The other matters a launch of `thread` (in matter `own`) fences, not yet resolved: every
/// matter the host knows but `own`, forgotten ones too (their folders are still the lawyer's
/// other clients' files), each invited when the thread's header names it and it is on the
/// list (a forgotten matter's invite waits for it to come back).
pub(crate) fn matter_fence(actor: &Actor, thread: ThreadId, own: MatterId) -> MatterFence {
    let invited = read_matters(actor, thread);
    let listed = actor.matters.values().map(|entry| (entry, true));
    let forgotten = actor.forgotten.values().map(|entry| (entry, false));
    let others = listed
        .chain(forgotten)
        .filter(|(entry, _)| entry.row.id != own)
        .map(|(entry, on_list)| {
            let row = &entry.row;
            let folder = PathBuf::from(&row.folder_path);
            let invited = on_list && invited.contains(&row.id);
            Other::new(row.id, row.name.clone(), folder, invited)
        })
        .collect();
    let mut fence = MatterFence::asked(others);
    fence.key = key_of(actor, thread, matters_hash(actor));
    fence
}

/// The key a launch of `thread` takes now (`FenceKey`); `matters` is [`matters_hash`], made
/// once for all threads.
pub(crate) fn key_of(actor: &Actor, thread: ThreadId, matters: u64) -> FenceKey {
    let mut hasher = DefaultHasher::new();
    let mut invited = read_matters(actor, thread);
    invited.sort();
    invited.hash(&mut hasher);
    FenceKey {
        matters,
        invited: hasher.finish(),
    }
}

/// Every matter's id and folder the host knows, hashed, listed and forgotten apart.
pub(crate) fn matters_hash(actor: &Actor) -> u64 {
    let mut hasher = DefaultHasher::new();
    for (list, entries) in [(0_u8, &actor.matters), (1, &actor.forgotten)] {
        list.hash(&mut hasher);
        for (id, entry) in entries {
            id.hash(&mut hasher);
            entry.row.folder_path.hash(&mut hasher);
        }
    }
    hasher.finish()
}
