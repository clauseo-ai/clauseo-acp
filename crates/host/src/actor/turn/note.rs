//! The undo note of a thread (`undo_note.rs`): the files put back since the last turn that
//! carried it, which the next prompt starts with.
//!
//! First read from the thread's log, by the rule a restart reads it by (`undo_note::read`); then
//! kept current in memory. Before a turn's prompt, the matter's restores since the thread's
//! last turn that carried the note are read off the actor, from that turn's after checkpoint
//! on, decoded with no fold: the cost is the matter's events since that turn, not its history.
//! When that turn failed (no after checkpoint) or the store has no row of it, its before
//! checkpoint places it the same way: no restore runs during a turn. With neither, or when no
//! turn carried the note, they are read from the conversation's creation, found by halving the
//! log: nothing logged since was told yet, and nothing before is read.
//!
//! A prompt takes the note and it is held aside while its turn runs: once that turn has ended,
//! the note is dropped if the agent took the prompt (`undo_note::carried`), or put back first.

use clauseo_protocol::{
    CheckpointId, Envelope, MatterPath, Seq, StreamId, ThreadId, Timestamp, TurnId,
};

use super::super::Actor;
use super::super::input::Input;
use super::{LogFor, TurnInput, state};
use crate::undo_note::{self, Floor, RestoreNote, Told};

/// A thread's undo note.
#[derive(Debug, Default)]
pub(crate) struct NoteState {
    /// The undos and restores since the last turn that carried them, in order; `None` until
    /// first needed.
    pub(crate) told: Option<Vec<Told>>,
    /// The matter's restores while `told` is `None` (`restore.rs`), until the note takes them.
    pub(crate) restored: Vec<RestoreNote>,
    /// `told` still lacks the matter's logged restores from here on.
    since: Option<Bound>,
    /// The read of those restores is in flight; these run once it answers.
    waiting: Option<Waiting>,
    /// What a prompt took of `told`, until its turn ended and it reached the agent or not.
    sent: Option<(TurnId, Vec<Told>)>,
}

/// Where the matter's log is read from for the note's restores (`undo_note::Read`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bound {
    /// The after checkpoint of the last turn that carried the note, when it has one.
    after: Option<CheckpointId>,
    /// That turn's before checkpoint, when it has one.
    before: Option<CheckpointId>,
    /// When the conversation was created.
    at: Timestamp,
}

/// One step that waits for the matter's restores to be read.
type Then = Box<dyn FnOnce(&mut Actor)>;

/// What waits for the matter's restores to be read.
struct Waiting(Vec<Then>);

impl std::fmt::Debug for Waiting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Waiting({})", self.0.len())
    }
}

impl NoteState {
    /// Adds a restore of the matter after what the note holds.
    pub(crate) fn add_restore(&mut self, note: &RestoreNote) {
        match self.told.as_mut() {
            Some(told) => told.push(Told::Restored(note.clone())),
            None => self.restored.push(note.clone()),
        }
    }

    /// Adds the files an undo put back (read first: [`ensure`]).
    pub(crate) fn add_undone(&mut self, paths: &[MatterPath]) {
        if let Some(told) = self.told.as_mut() {
            for path in paths {
                undo_note::add_undone(told, path);
            }
        }
    }

    /// Takes `reported` out of the note, and out of what a prompt holds aside.
    pub(crate) fn trim(&mut self, reported: &[MatterPath]) {
        let sent = self.sent.as_mut().map(|(_, told)| told);
        for note in self.told.as_mut().into_iter().chain(sent) {
            note.retain(|told| !matches!(told, Told::Undone(path) if reported.contains(path)));
        }
    }
}

/// The notes' text for the next prompt: the owed offline-shell note (`shell_network.rs`), then
/// the undo note; both stay until [`sent`].
pub(super) fn text(actor: &mut Actor, thread: ThreadId) -> Option<String> {
    ensure(actor, thread);
    let state = state(actor, thread)?;
    let undo = undo_note::note_text(state.note.told.as_deref().unwrap_or_default());
    let owed = state.offline_owed;
    let owed = owed.then(|| {
        let web_search = crate::actor::header_tools::web_search_on(actor, thread);
        super::shell_network::owed_note(web_search)
    });
    match (owed, undo) {
        (Some(owed), Some(undo)) => Some(format!("{owed}\n{undo}")),
        (owed, undo) => owed.or(undo),
    }
}

/// The note goes out with `turn`'s prompt: it is held aside until that turn has ended.
pub(super) fn sent(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    if let Some(state) = state(actor, thread) {
        state.offline_owed = false;
        let note = &mut state.note;
        let told = note.told.replace(Vec::new()).unwrap_or_default();
        note.sent = Some((turn, told));
        note.since = None;
    }
}

/// The note the last prompt took, once that turn has ended, its end is durable and the thread's
/// stream shows it: dropped if the turn carried it, else put back first.
fn take_back(actor: &mut Actor, thread: ThreadId) {
    let Some(turn) = sent_turn(actor, thread) else {
        return;
    };
    let Some(loaded) = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
    else {
        // Kept aside until the stream is loaded: a note the agent heard never comes back.
        return;
    };
    let carried = undo_note::carried(loaded.entries().values(), turn);
    let Some(state) = state(actor, thread) else {
        return;
    };
    let note = &mut state.note;
    let Some((_, mut told)) = note.sent.take() else {
        return;
    };
    if !carried {
        told.extend(note.told.take().unwrap_or_default());
        note.told = Some(told);
    }
}

/// The turn whose prompt took the note, once it is no longer current and its end is durable.
fn sent_turn(actor: &mut Actor, thread: ThreadId) -> Option<TurnId> {
    let turns = state(actor, thread)?;
    let turn = turns.note.sent.as_ref()?.0;
    let current = turns
        .current
        .as_ref()
        .is_some_and(|current| current.turn == turn);
    (!current && turns.closing.is_none()).then_some(turn)
}

/// Reads the note from the thread's log the first time it is needed, with the restores memory
/// holds; the matter's logged restores come later ([`read_first`]). Later, takes back what the
/// last prompt took if it did not reach the agent.
pub(super) fn ensure(actor: &mut Actor, thread: ThreadId) {
    take_back(actor, thread);
    let Some(turns) = state(actor, thread) else {
        return;
    };
    if turns.note.told.is_some() {
        return;
    }
    let restores = std::mem::take(&mut turns.note.restored);
    let created = actor.threads.get(&thread).map(|entry| entry.row.created);
    let loaded = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread());
    let (told, since) = match loaded.zip(created) {
        None => (undo_note::since_last_turn([], restores), None),
        Some((state, created)) => {
            let read = undo_note::read(state.entries().values());
            let since = Bound {
                after: read.since,
                before: read.before,
                at: created,
            };
            (
                undo_note::since_last_turn(read.inputs, restores),
                Some(since),
            )
        }
    };
    if let Some(turns) = state(actor, thread) {
        turns.note.told = Some(told);
        turns.note.since = since;
    }
}

/// Runs `then` once the note holds the matter's restores since the thread's last turn that
/// carried it: before a turn's prompt, so a restart forgets no restore. Reads them off the
/// actor when the note still lacks them and no prompt holds it aside.
pub(super) fn read_first(
    actor: &mut Actor,
    thread: ThreadId,
    then: impl FnOnce(&mut Actor) + 'static,
) {
    ensure(actor, thread);
    let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
    let Some((turns, matter)) = state(actor, thread).zip(matter) else {
        then(actor);
        return;
    };
    let note = &mut turns.note;
    if let Some(waiting) = note.waiting.as_mut() {
        waiting.0.push(Box::new(then));
        return;
    }
    let Some(since) = note.since.filter(|_| note.sent.is_none()) else {
        then(actor);
        return;
    };
    note.waiting = Some(Waiting(vec![Box::new(then)]));
    let store = actor.store.clone();
    actor.spawn_post(async move {
        let result = restores_log(&store, StreamId::Matter(matter), since).await;
        Input::Turn(TurnInput::Log {
            thread,
            why: LogFor::Restores(since),
            result,
        })
    });
}

/// The matter's restores the note lacks (`undo_note::kept`): from its log after the
/// `CheckpointRecorded` of `since`'s after checkpoint, else of its before checkpoint, the first
/// the store has a row of; with neither, those logged since the conversation was created. Never
/// the whole log's restores: those were told, or are older than the conversation.
async fn restores_log(
    store: &clauseo_store::Store,
    stream: StreamId,
    since: Bound,
) -> Result<Vec<Envelope>, clauseo_store::Error> {
    let mut placed = None;
    for checkpoint in [since.after, since.before].into_iter().flatten() {
        if let Some(row) = store.checkpoint(checkpoint).await? {
            placed = Some((row.seq, checkpoint));
            break;
        }
    }
    let (mut after, floor) = match placed {
        Some((seq, checkpoint)) => (Some(seq), Floor::After(checkpoint)),
        None => (seek(store, stream, since.at).await?, Floor::At(since.at)),
    };
    let mut log: Vec<Envelope> = Vec::new();
    loop {
        let page = store
            .read_after(stream, after, clauseo_store::READ_PAGE_MAX)
            .await?;
        let Some(last) = page.last() else {
            break;
        };
        after = Some(last.seq);
        log.extend(
            page.into_iter()
                .filter(|envelope| undo_note::kept(envelope, floor)),
        );
    }
    Ok(log)
}

/// Where to read `stream` from for the events logged at `at` or later: the first such event,
/// found by halving its seqs with one-event reads. `None` reads from the start.
async fn seek(
    store: &clauseo_store::Store,
    stream: StreamId,
    at: Timestamp,
) -> Result<Option<Seq>, clauseo_store::Error> {
    let heads = store.heads().await?;
    let Some(head) = heads
        .iter()
        .find(|(id, _)| *id == stream)
        .map(|(_, head)| head.get())
    else {
        return Ok(None);
    };
    // Every event after `low` that is before `high + 1` is to be looked at.
    let (mut low, mut high) = (0, head);
    while low < high {
        let middle = low + (high - low) / 2;
        let page = store.read_after(stream, Seq::new(middle).ok(), 1).await?;
        match page.first() {
            Some(next) if next.ts < at => low = middle + 1,
            Some(_) | None => high = middle,
        }
    }
    Ok(Seq::new(low).ok())
}

/// The matter's restores from `since` came back: they join the note, unless a prompt took
/// the note meanwhile; then what waited for them runs.
pub(super) fn on_restores(
    actor: &mut Actor,
    thread: ThreadId,
    since: Bound,
    result: Result<Vec<Envelope>, clauseo_store::Error>,
) {
    let logged = match result {
        Ok(log) => undo_note::restores_in(&log),
        Err(error) => {
            tracing::warn!(
                %thread,
                error = %crate::log_file::Cause(&error),
                "a matter's log was not read; the undo note leaves out its restores"
            );
            Vec::new()
        }
    };
    let full = match actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
    {
        Some(state) => {
            let inputs = undo_note::read(state.entries().values()).inputs;
            Ok(undo_note::since_last_turn(inputs, logged))
        }
        None => Err(logged),
    };
    let Some(turns) = state(actor, thread) else {
        return;
    };
    let note = &mut turns.note;
    let waiting = note.waiting.take();
    if note.since == Some(since)
        && note.sent.is_none()
        && let Some(told) = note.told.take()
    {
        note.since = None;
        note.told = Some(match full {
            Ok(full) => undo_note::with_restores(told, full),
            Err(logged) => undo_note::append_restores(told, logged),
        });
    }
    for then in waiting.map(|waiting| waiting.0).unwrap_or_default() {
        then(actor);
    }
}

#[cfg(test)]
mod tests;
