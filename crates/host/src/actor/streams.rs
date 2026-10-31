//! Loaded streams: loading (reopen, snapshot, fold), applying returned commits in seq order,
//! publishing their view changes once to every view subscriber, the snapshot cadence, and
//! unloading a stream nobody needs.
//!
//! A stream stays loaded while it has a view subscriber, an in-flight commit, or the turn path
//! keeps it (a live agent or an open turn). A commit of an unloaded stream is not applied: the
//! store has it, and the next load reads it.

use clauseo_protocol::{DomainEvent, Envelope, StreamId};
use clauseo_reducer::{REDUCER_SCHEMA_VERSION, Snapshot, StreamState, ViewChange};
use clauseo_store::{Committed, READ_PAGE_MAX, Reopen, SNAPSHOT_EVERY_EVENTS, SnapshotIn};

use super::input::Input;
use super::subs::ViewSub;
use super::{Actor, Tag, turn};
use crate::error::Error;
use crate::pipeline::{Draft, Failure, Ticket};
use crate::sent::SentView;
use crate::view::StreamView;

/// What a caller of [`Actor::with_loaded`] runs once the stream is loaded or failed to load.
pub(crate) type Waiter = Box<dyn FnOnce(&mut Actor, Result<(), Error>)>;

/// A stream the actor holds in memory, or is loading.
pub(crate) enum StreamSlot {
    Loading(Loading),
    Loaded(Box<Loaded>),
}

/// A stream being loaded.
pub(crate) struct Loading {
    waiters: Vec<Waiter>,
    /// Envelopes of commits that returned during the load, applied after it when newer.
    buffered: Vec<Envelope>,
}

/// A loaded stream.
pub(crate) struct Loaded {
    pub(crate) state: StreamState,
    pub(crate) view: StreamView,
    pub(crate) sent: SentView,
    pub(crate) views: Vec<ViewSub>,
    /// The last position a viewer acknowledged; recorded only.
    pub(crate) ack: Option<(clauseo_protocol::ViewEpoch, clauseo_protocol::Cursor)>,
    /// Events applied since the last snapshot was taken.
    since_snapshot: u64,
}

/// Why a load failed, kept so that every waiter gets the same error.
#[derive(Debug, Clone)]
enum LoadFailure {
    Store(clauseo_store::Error),
    Internal(String),
}

impl LoadFailure {
    fn to_error(&self) -> Error {
        match self {
            Self::Store(error) => Error::Store(error.clone()),
            Self::Internal(detail) => Error::Internal(detail.clone()),
        }
    }
}

/// What a store read gave for a load.
enum Folded {
    State {
        state: Box<StreamState>,
        tail: Vec<Envelope>,
    },
    /// The snapshot did not decode: read the whole log and fold it.
    NeedsWholeLog,
}

/// Runs `then` once `stream` is loaded (at once when it is), loading it first.
pub(crate) fn with_loaded(actor: &mut Actor, stream: StreamId, then: Waiter) {
    match actor.streams.get_mut(&stream) {
        Some(StreamSlot::Loaded(_)) => then(actor, Ok(())),
        Some(StreamSlot::Loading(loading)) => loading.waiters.push(then),
        None => {
            actor.streams.insert(
                stream,
                StreamSlot::Loading(Loading {
                    waiters: vec![then],
                    buffered: Vec::new(),
                }),
            );
            actor.work.loads += 1;
            let store = actor.store.clone();
            actor.spawn_post(async move {
                let result = store.reopen(stream, REDUCER_SCHEMA_VERSION).await;
                Input::Reopened { stream, result }
            });
        }
    }
}

/// The loaded state of `stream`, if it is loaded.
pub(crate) fn loaded(actor: &Actor, stream: StreamId) -> Option<&Loaded> {
    match actor.streams.get(&stream) {
        Some(StreamSlot::Loaded(loaded)) => Some(loaded),
        Some(StreamSlot::Loading(_)) | None => None,
    }
}

/// A store read for a load came back.
pub(super) fn on_reopened(
    actor: &mut Actor,
    stream: StreamId,
    result: Result<Reopen, clauseo_store::Error>,
) {
    actor.work.loads = actor.work.loads.saturating_sub(1);
    let folded = result
        .map_err(LoadFailure::Store)
        .and_then(|reopen| fold(stream, reopen));
    match folded {
        Ok(Folded::NeedsWholeLog) => {
            actor.work.loads += 1;
            let store = actor.store.clone();
            actor.spawn_post(async move {
                let result = read_whole_log(&store, stream).await;
                Input::Reopened { stream, result }
            });
        }
        Ok(Folded::State { state, tail }) => finish_load(actor, stream, *state, &tail),
        Err(failure) => {
            if let Some(StreamSlot::Loading(loading)) = actor.streams.remove(&stream) {
                tracing::warn!(%stream, ?failure, "could not load a stream");
                for waiter in loading.waiters {
                    waiter(actor, Err(failure.to_error()));
                }
            }
        }
    }
}

fn finish_load(actor: &mut Actor, stream: StreamId, mut state: StreamState, tail: &[Envelope]) {
    let Some(StreamSlot::Loading(loading)) = actor.streams.remove(&stream) else {
        return;
    };
    let built = actor
        .ids
        .mint(actor.clock.now().wall)
        .and_then(|epoch| StreamView::new(stream, epoch, actor.window_cap));
    let view = match built {
        Ok(view) => view,
        Err(error) => {
            let failure = LoadFailure::Internal(error.to_string());
            for waiter in loading.waiters {
                waiter(actor, Err(failure.to_error()));
            }
            return;
        }
    };
    let mut sent = SentView::rebuild(stream, tail);
    let since_snapshot = u64::try_from(tail.len()).unwrap_or(u64::MAX);
    // Commits that returned during the load: the read may already hold the first of them.
    let first_new = loading
        .buffered
        .iter()
        .position(|envelope| is_new(&state, envelope))
        .unwrap_or(loading.buffered.len());
    let newer = loading.buffered.get(first_new..).unwrap_or_default();
    for envelope in newer {
        if let Err(error) = clauseo_reducer::apply(&mut state, envelope) {
            tracing::error!(%stream, %error, "a returned commit did not fold");
        }
    }
    sent.on_committed(newer);
    sent.on_sent(newer);
    // Commits sent and not returned yet, past what the read holds, are in the sent layer only.
    for envelope in actor
        .pipeline
        .unreleased(stream)
        .filter(|envelope| is_new(&state, envelope))
    {
        sent.on_sent(std::slice::from_ref(envelope));
    }
    let loaded = Box::new(Loaded {
        state,
        view,
        sent,
        views: Vec::new(),
        ack: None,
        since_snapshot,
    });
    actor.streams.insert(stream, StreamSlot::Loaded(loaded));
    for waiter in loading.waiters {
        waiter(actor, Ok(()));
    }
    if let Some(StreamSlot::Loaded(loaded)) = actor.streams.get_mut(&stream)
        && loaded.since_snapshot >= SNAPSHOT_EVERY_EVENTS
    {
        take_snapshot(&mut actor.work, &actor.snapshots, stream, loaded);
    }
    actor.release(stream);
}

/// Unloads `stream` when nothing needs it: no view subscriber, no commit in flight, and
/// the turn path does not keep it. A thread keeps its header for `ListThreads`.
pub(crate) fn release(actor: &mut Actor, stream: StreamId) {
    let Some(StreamSlot::Loaded(loaded)) = actor.streams.get(&stream) else {
        return;
    };
    if !loaded.views.is_empty() || actor.pipeline.in_flight(stream) {
        return;
    }
    if let StreamId::Thread(thread) = stream
        && turn::keeps_loaded(actor, thread)
    {
        return;
    }
    if let Some(StreamSlot::Loaded(loaded)) = actor.streams.remove(&stream)
        && let (StreamId::Thread(thread), Some(state)) = (stream, loaded.state.as_thread())
        && let Some(entry) = actor.threads.get_mut(&thread)
    {
        entry.header = state.header().cloned();
    }
}

/// A commit's result came back: release the commits now complete in send order, apply
/// and publish each, then run its continuation.
pub(super) fn on_committed(
    actor: &mut Actor,
    ticket: Ticket,
    result: Result<Committed, clauseo_store::Error>,
) {
    actor.work.commits = actor.work.commits.saturating_sub(1);
    let completed = actor.pipeline.on_result(ticket, result);
    let mut touched: Vec<StreamId> = Vec::new();
    // Every commit that returned `Ok` is folded first, then the sent views of drained streams
    // roll back to what is durable, and only then do continuations run: a commit they send is
    // built on, and folded into, the rolled-back view.
    let mut continuations = Vec::with_capacity(completed.done.len());
    for done in completed.done {
        match done.outcome {
            Ok(envelopes) => {
                apply_commit(actor, envelopes, &mut touched);
                continuations.push((done.tag, Ok(())));
            }
            Err(failure) => {
                if failure.first {
                    tracing::warn!(error = %failure.error, "a commit failed");
                }
                continuations.push((done.tag, Err(failure)));
            }
        }
    }
    for stream in &completed.unblocked {
        if let Some(StreamSlot::Loaded(loaded)) = actor.streams.get_mut(stream) {
            loaded.sent.roll_back();
        }
    }
    for (tag, outcome) in continuations {
        continue_commit(actor, tag, outcome.as_ref().map(|&()| ()));
    }
    for stream in completed.unblocked {
        turn::unblocked(actor, stream);
        touched.push(stream);
    }
    for stream in touched {
        actor.release(stream);
    }
}

fn continue_commit(actor: &mut Actor, tag: Tag, outcome: Result<(), &Failure>) {
    match tag {
        Tag::Command {
            id,
            outcome: answer,
            after,
        } => {
            super::commands::command_committed(actor, id, answer, after, outcome);
        }
        Tag::Scratch(entry) => super::matters::scratch_committed(actor, *entry, outcome),
        Tag::Snapshot => {
            if let Err(failure) = outcome {
                tracing::warn!(error = %failure.error, "a snapshot was not stored");
            }
        }
        Tag::Turn(tag) => turn::committed(actor, tag, outcome),
    }
}

/// Applies one returned commit, stream by stream: fold, publish to view subscribers, take a
/// snapshot when one is due; then hands each stream's envelopes to its event subscribers.
fn apply_commit(actor: &mut Actor, envelopes: Vec<Envelope>, touched: &mut Vec<StreamId>) {
    let mut streams: Vec<StreamId> = Vec::new();
    for envelope in &envelopes {
        if !streams.contains(&envelope.stream) {
            streams.push(envelope.stream);
        }
    }
    for &stream in &streams {
        touched.push(stream);
        let envelopes = envelopes.as_slice();
        match actor.streams.get_mut(&stream) {
            Some(StreamSlot::Loaded(loaded)) => {
                let due = fold_commit(loaded, stream, envelopes);
                let encoded = match changes_of(loaded, stream, envelopes, &mut actor.encoder) {
                    Ok(encoded) => encoded,
                    Err(error) => {
                        tracing::error!(%stream, %error, "view changes did not encode");
                        Vec::new()
                    }
                };
                for event in &encoded {
                    loaded.views.retain_mut(|sub| sub.offer(event));
                }
                if due || loaded.since_snapshot >= SNAPSHOT_EVERY_EVENTS {
                    take_snapshot(&mut actor.work, &actor.snapshots, stream, loaded);
                }
            }
            Some(StreamSlot::Loading(loading)) => loading.buffered.extend(
                envelopes
                    .iter()
                    .filter(|envelope| envelope.stream == stream)
                    .cloned(),
            ),
            None => {}
        }
    }
    // The batch is built once per stream; a commit of one stream hands over its own.
    if let [stream] = streams.as_slice() {
        super::subs::deliver_events(actor, *stream, envelopes);
        return;
    }
    for stream in streams {
        if actor.event_subs.contains_key(&stream) {
            let batch = envelopes
                .iter()
                .filter(|envelope| envelope.stream == stream)
                .cloned()
                .collect();
            super::subs::deliver_events(actor, stream, batch);
        }
    }
}

/// A snapshot came back compressed: store it in a commit of its own.
pub(super) fn on_snapshot(actor: &mut Actor, snapshot: SnapshotIn) {
    actor.work.snapshots = actor.work.snapshots.saturating_sub(1);
    let mut draft = Draft::new();
    draft.snapshot(snapshot);
    if let Err(error) = actor.send_commit(draft, Tag::Snapshot) {
        tracing::warn!(%error, "a snapshot was not sent");
    }
}

/// Folds the commit's envelopes of `stream` into the loaded state, skipping any the load
/// already read. Returns whether a `TurnEnded` makes a snapshot due. The view changes are
/// gathered by [`changes_of`].
fn fold_commit(loaded: &mut Loaded, stream: StreamId, envelopes: &[Envelope]) -> bool {
    loaded.sent.on_committed(envelopes);
    envelopes
        .iter()
        .filter(|envelope| envelope.stream == stream)
        .any(|envelope| matches!(envelope.event, DomainEvent::TurnEnded(_)))
}

/// Applies the commit's envelopes of `stream` and publishes their changes once.
fn changes_of(
    loaded: &mut Loaded,
    stream: StreamId,
    envelopes: &[Envelope],
    encoder: &mut crate::encode::Encoder,
) -> Result<Vec<crate::encode::EncodedEvent>, Error> {
    let mut changes: Vec<ViewChange> = Vec::new();
    for envelope in envelopes
        .iter()
        .filter(|envelope| envelope.stream == stream)
    {
        if !is_new(&loaded.state, envelope) {
            continue;
        }
        match clauseo_reducer::apply(&mut loaded.state, envelope) {
            Ok(applied) => {
                for anomaly in &applied.anomalies {
                    tracing::warn!(%stream, ?anomaly, "reducer anomaly");
                }
                changes.extend(applied.changes);
                loaded.since_snapshot = loaded.since_snapshot.saturating_add(1);
            }
            Err(error) => tracing::error!(%stream, %error, "a returned commit did not fold"),
        }
    }
    loaded.view.publish(changes, encoder)
}

/// Whether `envelope` is past what `state` holds.
fn is_new(state: &StreamState, envelope: &Envelope) -> bool {
    state.seq().is_none_or(|seq| envelope.seq > seq)
}

/// Encodes a snapshot of the loaded state on the actor and hands it to the snapshot thread.
fn take_snapshot(
    work: &mut super::Work,
    snapshots: &crate::snapshot::SnapshotWorker,
    stream: StreamId,
    loaded: &mut Loaded,
) {
    loaded.since_snapshot = 0;
    let Some(seq) = loaded.state.seq() else {
        return;
    };
    match Snapshot::of(&loaded.state).to_bytes() {
        Ok(bytes) => match snapshots.submit(stream, seq, bytes) {
            Ok(()) => work.snapshots += 1,
            Err(error) => tracing::warn!(%stream, %error, "the snapshot thread is gone"),
        },
        Err(error) => tracing::warn!(%stream, %error, "a snapshot did not encode"),
    }
}

/// The state a reopen gives: the snapshot (when it decodes and is this stream's) with the
/// tail folded on top.
fn fold(stream: StreamId, reopen: Reopen) -> Result<Folded, LoadFailure> {
    let mut state = match reopen.snapshot {
        Some(snapshot) => {
            let decoded = Snapshot::from_bytes(&snapshot.state).and_then(Snapshot::into_state);
            match decoded {
                Ok(state) if state.stream() == stream && state.seq() == Some(snapshot.seq) => state,
                Ok(_) => {
                    tracing::warn!(%stream, "a snapshot names another place; folding events");
                    return Ok(Folded::NeedsWholeLog);
                }
                Err(error) => {
                    tracing::warn!(%stream, %error, "a snapshot did not decode; folding events");
                    return Ok(Folded::NeedsWholeLog);
                }
            }
        }
        None => StreamState::new(stream),
    };
    for envelope in &reopen.tail {
        let applied = clauseo_reducer::apply(&mut state, envelope)
            .map_err(|error| LoadFailure::Internal(format!("folding {stream}: {error}")))?;
        for anomaly in &applied.anomalies {
            tracing::warn!(%stream, ?anomaly, "reducer anomaly");
        }
    }
    Ok(Folded::State {
        state: Box::new(state),
        tail: reopen.tail,
    })
}

/// Every envelope of `stream`, as a reopen with no snapshot.
pub(super) async fn read_whole_log(
    store: &clauseo_store::Store,
    stream: StreamId,
) -> Result<Reopen, clauseo_store::Error> {
    let mut tail: Vec<Envelope> = Vec::new();
    loop {
        let after = tail.last().map(|envelope| envelope.seq);
        let page = store.read_after(stream, after, READ_PAGE_MAX).await?;
        if page.is_empty() {
            break;
        }
        tail.extend(page);
    }
    let head = tail.last().map(|envelope| envelope.seq);
    Ok(Reopen {
        snapshot: None,
        tail,
        head,
    })
}
