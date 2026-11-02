//! The matter rail's data: each matter's [`MatterSummary`] for `ListMatters` (its details, its
//! last activity, its folder for display and its status), the status rule, and the times the
//! host keeps for it from every returned commit.
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
    DomainEvent, Envelope, FolderState, MatterId, MatterStatus, MatterSummary, StopReason,
    StreamId, ThreadStatus, Timestamp,
};

use super::{Actor, MatterEntry};

/// What the status rule needs of one thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThreadFacts {
    /// Its header's status.
    pub(crate) status: ThreadStatus,
    /// A permission request of it waits for the lawyer.
    pub(crate) permission_pending: bool,
    /// How and when its last turn ended.
    pub(crate) last_stop: Option<(StopReason, Timestamp)>,
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
/// says how its last turn ended, if that was after `opened` (the matter's `last_opened`; no
/// opening means every end counts): `end_turn` is `Ready`, any other ending `Stopped`, since
/// the answer did not finish as asked.
fn thread_status(facts: &ThreadFacts, opened: Option<Timestamp>) -> Option<MatterStatus> {
    let after_opened = |at: Timestamp| opened.is_none_or(|opened| at > opened);
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
                Some((StopReason::EndTurn, at)) if after_opened(at) => MatterStatus::Ready,
                Some((_, at)) if after_opened(at) => MatterStatus::Stopped { at },
                Some(_) | None => MatterStatus::Idle,
            })
        }
    }
}

/// The matter's status over `threads`, opened last at `opened`: the highest-ranked thread status
/// ([`rank`]); among paused threads the earliest known reset, and among stopped ones the latest
/// stop. `Idle` with no thread.
pub(crate) fn matter_status(
    threads: impl IntoIterator<Item = ThreadFacts>,
    opened: Option<Timestamp>,
) -> MatterStatus {
    let mut best = MatterStatus::Idle;
    for facts in threads {
        let Some(status) = thread_status(&facts, opened) else {
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
        facts.entry(matter).or_default().push(ThreadFacts {
            status,
            permission_pending: loaded
                .is_some_and(|loaded| loaded.sent.permissions().next().is_some()),
            last_stop: thread.last_stop,
        });
    }
    let mut rows: Vec<&MatterEntry> = actor.matters.values().collect();
    rows.sort_by_key(|entry| (entry.row.created, entry.row.id));
    rows.into_iter()
        .map(|entry| {
            let row = &entry.row;
            let own = actor.tails.get(&StreamId::Matter(row.id)).copied();
            let last_activity = match (own, activity.get(&row.id).copied()) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
            MatterSummary {
                matter: row.id,
                name: row.name.clone(),
                kind: row.kind,
                last_activity,
                client: row.client.clone(),
                next_date: row.next_date,
                pinned: row.pinned,
                last_opened: row.last_opened,
                status: matter_status(facts.remove(&row.id).unwrap_or_default(), row.last_opened),
                folder_display: folder_display(Path::new(&row.folder_path), &actor.home_spellings),
                // Filled in off the actor by `ListMatters` (`folder_state.rs`).
                folder_state: FolderState::Found,
            }
        })
        .collect()
}

/// Keeps what the rail needs from durable envelopes: each stream's newest time, and each
/// thread's last `TurnEnded`.
pub(crate) fn note_commit(actor: &mut Actor, envelopes: &[Envelope]) {
    for envelope in envelopes {
        let newest = actor.tails.entry(envelope.stream).or_insert(envelope.ts);
        *newest = (*newest).max(envelope.ts);
        if let (StreamId::Thread(thread), DomainEvent::TurnEnded(ended)) =
            (envelope.stream, &envelope.event)
            && let Some(entry) = actor.threads.get_mut(&thread)
        {
            entry.last_stop = Some((ended.stop, envelope.ts));
        }
    }
}

/// How and when the last turn in a thread's folded state ended: its newest step.
pub(crate) fn last_step(state: &clauseo_reducer::ThreadState) -> Option<(StopReason, Timestamp)> {
    state.entries().values().rev().find_map(|entry| {
        if let clauseo_protocol::ViewEntry::Step(step) = entry {
            Some((step.stop, step.ts))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests;
