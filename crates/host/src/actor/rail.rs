//! The matter rail's data: each matter's [`MatterSummary`] for `ListMatters` (its details, its
//! last activity, its folder for display and its status), each thread's [`ThreadSummary`] for
//! `ListThreads` (its status, seen mark, last stop and last activity), the status rule, and the
//! times and stops the host keeps for them from every returned commit. `app_lists.rs` sends
//! the same rows to app subscribers when they change.
//!
//! The status is computed each time it is asked, from what the host holds for each thread: its
//! header's status (the loaded state's, or the one kept when it was unloaded), whether a
//! permission is pending (only a thread with a live agent can wait on one, and its stream is
//! loaded then), and how its last turn ended. Nothing is read from the store, so `ListMatters`
//! stays a walk over memory: one pass over the threads, one map lookup per stream for the last
//! activity.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clauseo_protocol::{
    CompactString, DomainEvent, Envelope, FolderState, LastStop, MatterId, MatterStatus,
    MatterSummary, Seq, StopReason, StreamId, ThreadStatus, ThreadSummary, Timestamp,
};

use super::{Actor, MatterEntry, ThreadEntry};

/// How and when a thread's last turn ended, and the `seq` of its `TurnEnded`, which the seen
/// mark is compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stop {
    pub(crate) reason: StopReason,
    pub(crate) at: Timestamp,
    pub(crate) seq: Seq,
}

/// What the status rule needs of one thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThreadFacts {
    /// Its header's status.
    pub(crate) status: ThreadStatus,
    /// A permission request of it waits for the lawyer.
    pub(crate) permission_pending: bool,
    /// How and when its last turn ended.
    pub(crate) last_stop: Option<(StopReason, Timestamp)>,
    /// The lawyer has seen that ending (`MarkSeen` after it), or there is none.
    pub(crate) seen: bool,
}

/// The rank of each status when threads differ: the first that any thread gives wins. A
/// permission outranks everything (it blocks the agent and only the lawyer can answer it), then
/// a question, then a pause at the limit (also waiting on the lawyer's hand), then an answer
/// ready to read, then work in progress, then a stop.
fn rank(status: MatterStatus) -> u8 {
    match status {
        MatterStatus::NeedsPermission => 0,
        MatterStatus::NeedsAnswer => 1,
        MatterStatus::PausedAtLimit { .. } => 2,
        MatterStatus::Ready => 3,
        MatterStatus::Working => 4,
        MatterStatus::Stopped { .. } => 5,
        MatterStatus::Idle => 6,
    }
}

/// One thread's own status. An archived thread says nothing (`None`). A thread that runs nothing
/// says how its last turn ended while the lawyer has not seen it (no `MarkSeen` since):
/// `end_turn` is `Ready`, any other ending `Stopped`, since the answer did not finish as asked.
fn thread_status(facts: &ThreadFacts) -> Option<MatterStatus> {
    match facts.status {
        ThreadStatus::Archived => None,
        ThreadStatus::NeedsYou if facts.permission_pending => Some(MatterStatus::NeedsPermission),
        ThreadStatus::NeedsYou => Some(MatterStatus::NeedsAnswer),
        ThreadStatus::PausedAtLimit { resets_at } => {
            Some(MatterStatus::PausedAtLimit { resets_at })
        }
        ThreadStatus::Starting | ThreadStatus::Running | ThreadStatus::Queued { .. } => {
            Some(MatterStatus::Working)
        }
        ThreadStatus::Idle | ThreadStatus::AgentExited | ThreadStatus::IdleStopped => {
            Some(match facts.last_stop {
                Some((StopReason::EndTurn, _)) if !facts.seen => MatterStatus::Ready,
                Some((_, at)) if !facts.seen => MatterStatus::Stopped { at },
                Some(_) | None => MatterStatus::Idle,
            })
        }
    }
}

/// The matter's status over `threads`: the highest-ranked thread status ([`rank`]); among
/// paused threads the earliest known reset, and among stopped ones the latest stop. `Idle` with
/// no thread.
pub(crate) fn matter_status(threads: impl IntoIterator<Item = ThreadFacts>) -> MatterStatus {
    let mut best = MatterStatus::Idle;
    for facts in threads {
        let Some(status) = thread_status(&facts) else {
            continue;
        };
        best = match (best, status) {
            (
                MatterStatus::PausedAtLimit { resets_at: a },
                MatterStatus::PausedAtLimit { resets_at: b },
            ) => MatterStatus::PausedAtLimit {
                resets_at: match (a, b) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                },
            },
            (MatterStatus::Stopped { at: a }, MatterStatus::Stopped { at: b }) => {
                MatterStatus::Stopped { at: a.max(b) }
            }
            (best, status) if rank(status) < rank(best) => status,
            (best, _) => best,
        };
    }
    best
}

/// The spellings of `home` that `folder_display` tries: as given, and canonicalised (read once,
/// at start-up) when that differs, so a folder stored by its real path still shows below a home
/// configured through a link. None without a home.
pub(crate) fn home_spellings(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let mut spellings = vec![home.to_path_buf()];
    if let Ok(canonical) = dunce::canonicalize(home)
        && canonical != home
    {
        spellings.push(canonical);
    }
    spellings
}

/// `folder` as the rail shows it: below one of `homes` (`home_spellings`), the part after it
/// with the system's separators (`Documents/Clauseo/State v. Kunal Desai`, no `~`), compared by
/// components and, on macOS and Windows, ignoring case, as the refused-folder check compares; the
/// home folder itself, or a folder elsewhere, in full.
pub(crate) fn folder_display(folder: &Path, homes: &[PathBuf]) -> String {
    homes
        .iter()
        .find_map(|home| below(folder, home))
        .map_or_else(
            || folder.display().to_string(),
            |rest| rest.display().to_string(),
        )
}

/// The part of `folder` after `home`, when `folder` is strictly below it.
fn below(folder: &Path, home: &Path) -> Option<PathBuf> {
    let mut parts = folder.components();
    for part in home.components() {
        let found = parts.next()?;
        if !same_component(found.as_os_str(), part.as_os_str()) {
            return None;
        }
    }
    let rest: PathBuf = parts.collect();
    (!rest.as_os_str().is_empty()).then_some(rest)
}

/// Whether two path components name the same entry: ignoring case where the file system usually
/// does (macOS, Windows), exactly elsewhere.
fn same_component(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    if cfg!(any(target_os = "macos", windows)) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Whether the lawyer has seen how `thread`'s last turn ended: no stop, or one at or before its
/// seen mark.
pub(crate) fn seen(thread: &ThreadEntry) -> bool {
    thread
        .last_stop
        .is_none_or(|stop| thread.row.seen_seq.is_some_and(|mark| stop.seq <= mark))
}

/// What the rail reads of one thread: its header (the loaded state's, else the one kept when it
/// was unloaded), whether a permission waits, and its status.
struct Read<'a> {
    header: Option<&'a clauseo_protocol::HeaderEntry>,
    permission_pending: bool,
    status: ThreadStatus,
}

fn read<'a>(actor: &'a Actor, thread: &'a ThreadEntry) -> Read<'a> {
    let loaded = actor.loaded(StreamId::Thread(thread.row.id));
    let header = loaded
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(thread.header.as_ref());
    let status = match header {
        Some(header) => header.status,
        None if thread.row.archived => ThreadStatus::Archived,
        None => ThreadStatus::Idle,
    };
    Read {
        header,
        permission_pending: loaded.is_some_and(|loaded| loaded.sent.permissions().next().is_some()),
        status,
    }
}

/// What the status rule needs of `thread`.
pub(crate) fn thread_facts(actor: &Actor, thread: &ThreadEntry) -> ThreadFacts {
    let read = read(actor, thread);
    ThreadFacts {
        status: read.status,
        permission_pending: read.permission_pending,
        last_stop: thread.last_stop.map(|stop| (stop.reason, stop.at)),
        seen: seen(thread),
    }
}

/// `thread`'s row, as `ListThreads` answers and `ThreadsChanged` carries, and whether a
/// permission of it waits (which the row does not show but the matter's status does).
pub(crate) fn thread_summary(actor: &Actor, thread: &ThreadEntry) -> (ThreadSummary, bool) {
    let read = read(actor, thread);
    let summary = ThreadSummary {
        thread: thread.row.id,
        matter: thread.row.matter,
        title: read
            .header
            .and_then(|header| header.title.clone())
            .or_else(|| thread.row.title.as_deref().map(CompactString::from)),
        agent: thread.row.agent.clone(),
        status: read.status,
        queue_held: super::turn::queue_held(actor, thread.row.id),
        seen: seen(thread),
        snoozed_until: read.header.and_then(|header| header.snoozed_until),
        done: read.header.is_some_and(|header| header.done),
        last_stop: thread.last_stop.map(|stop| LastStop {
            reason: stop.reason,
            at: stop.at,
        }),
        last_activity: actor.tails.get(&StreamId::Thread(thread.row.id)).copied(),
    };
    (summary, read.permission_pending)
}

/// The threads of `matter`, in `ListThreads`' order (by id, which is by creation).
pub(crate) fn thread_summaries(actor: &Actor, matter: MatterId) -> Vec<ThreadSummary> {
    actor
        .threads
        .values()
        .filter(|thread| thread.row.matter == matter)
        .map(|thread| thread_summary(actor, thread).0)
        .collect()
}

/// The newer of two optional times.
fn newest(a: Option<Timestamp>, b: Option<Timestamp>) -> Option<Timestamp> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// One matter's row from its threads' facts and their newest activity.
fn matter_row(
    actor: &Actor,
    entry: &MatterEntry,
    facts: Vec<ThreadFacts>,
    activity: Option<Timestamp>,
) -> MatterSummary {
    let row = &entry.row;
    let own = actor.tails.get(&StreamId::Matter(row.id)).copied();
    MatterSummary {
        matter: row.id,
        name: row.name.clone(),
        kind: row.kind,
        last_activity: newest(own, activity),
        client: row.client.clone(),
        next_date: row.next_date,
        skill_packs: row.skill_packs.clone(),
        pinned: row.pinned,
        last_opened: row.last_opened,
        status: matter_status(facts),
        folder_display: folder_display(Path::new(&row.folder_path), &actor.home_spellings),
        // As the last `ListMatters` read it (`found` before any); `ListMatters` fills its own
        // answer off the actor (`folder_state.rs`).
        folder_state: actor
            .folder_states
            .get(&row.id)
            .copied()
            .unwrap_or(FolderState::Found),
        // As the folder check last answered at the create or an agent launch
        // (`agent/folder_support.rs`): read from memory, never checked here.
        folder_support: actor.folder_supports.get(&row.id).copied(),
    }
}

/// The row of the listed matter `matter` alone: one pass over the threads.
pub(crate) fn matter_summary(actor: &Actor, matter: MatterId) -> Option<MatterSummary> {
    let entry = actor.matters.get(&matter)?;
    let mut facts = Vec::new();
    let mut activity = None;
    for thread in actor
        .threads
        .values()
        .filter(|thread| thread.row.matter == matter)
    {
        let ts = actor.tails.get(&StreamId::Thread(thread.row.id)).copied();
        activity = newest(activity, ts);
        facts.push(thread_facts(actor, thread));
    }
    Some(matter_row(actor, entry, facts, activity))
}

/// Every matter on the list, oldest first, as `ListMatters` answers.
pub(crate) fn summaries(actor: &Actor) -> Vec<MatterSummary> {
    let mut facts: BTreeMap<MatterId, Vec<ThreadFacts>> = BTreeMap::new();
    let mut activity: BTreeMap<MatterId, Timestamp> = BTreeMap::new();
    for thread in actor.threads.values() {
        let matter = thread.row.matter;
        if !actor.matters.contains_key(&matter) {
            continue;
        }
        if let Some(&ts) = actor.tails.get(&StreamId::Thread(thread.row.id)) {
            let newest = activity.entry(matter).or_insert(ts);
            *newest = (*newest).max(ts);
        }
        facts
            .entry(matter)
            .or_default()
            .push(thread_facts(actor, thread));
    }
    let mut rows: Vec<&MatterEntry> = actor.matters.values().collect();
    rows.sort_by_key(|entry| (entry.row.created, entry.row.id));
    rows.into_iter()
        .map(|entry| {
            let id = entry.row.id;
            let threads = facts.remove(&id).unwrap_or_default();
            matter_row(actor, entry, threads, activity.get(&id).copied())
        })
        .collect()
}

/// Keeps what the rail needs from durable envelopes: each stream's newest time, and each
/// thread's last `TurnEnded` (every turn ends with one, at the limit and failed ones too); and
/// marks each thread touched, for `app_lists.rs`.
pub(crate) fn note_commit(actor: &mut Actor, envelopes: &[Envelope]) {
    for envelope in envelopes {
        let newest = actor.tails.entry(envelope.stream).or_insert(envelope.ts);
        *newest = (*newest).max(envelope.ts);
        let StreamId::Thread(thread) = envelope.stream else {
            continue;
        };
        actor.lists.touch_thread(thread);
        actor.triage.committed(&envelope.event);
        if let DomainEvent::TurnEnded(ended) = &envelope.event
            && let Some(entry) = actor.threads.get_mut(&thread)
        {
            entry.last_stop = Some(Stop {
                reason: ended.stop,
                at: envelope.ts,
                seq: envelope.seq,
            });
        }
    }
}

/// How and when the last turn in a thread's folded state ended: its newest step.
pub(crate) fn last_step(state: &clauseo_reducer::ThreadState) -> Option<Stop> {
    state.entries().values().rev().find_map(|entry| {
        if let clauseo_protocol::ViewEntry::Step(step) = entry {
            Some(Stop {
                reason: step.stop,
                at: step.ts,
                seq: step.order,
            })
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests;
