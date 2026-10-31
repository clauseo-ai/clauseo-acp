//! The commit pipeline: seq assignment, pipelined commits and their results in seq order.
//!
//! Each stream has a durable head (from `Store::heads`, moved when a commit returns `Ok`) and a
//! pending head (the last seq sent). A new commit takes `pending + 1` and is sent at once,
//! without waiting for the stream's earlier commits: with `synchronous=FULL` and `fullfsync` a
//! sync costs about 4 ms, so waiting for each commit would cap a stream near 250 commits a
//! second, and group commit puts the waiting ones in one sync. The feeder task
//! ([`spawn_feeder`]) hands commits to the store in send order; results come back on their own
//! tasks in any order, and [`Pipeline::on_result`] releases them in send order, which is seq
//! order per stream. Nothing of a commit is applied or published before its own result is `Ok`.
//!
//! The store refuses a seq gap, so when one commit fails, every later in-flight commit of its
//! stream fails too. The first failure blocks the stream: nothing more is sent for it until its
//! in-flight commits have all returned, then its pending head goes back to the durable head. A
//! later failure caused by the first reports the first one's error, never its own `SeqGap`.

use std::collections::{BTreeMap, VecDeque};

use clauseo_protocol::{CommandId, DomainEvent, Envelope, EventId, Seq, StreamId, Timestamp};
use clauseo_store::{BlobIn, Commit, Committed, MatterRow, Receipt, SnapshotIn, ThreadRow};

use crate::error::Error;
use crate::ids::Ids;

mod feeder;

pub(crate) use feeder::{FeederTx, Post, spawn_feeder};

/// Names one sent commit; results are matched to commits by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Ticket(u64);

/// The seqs one draft takes in one stream.
#[derive(Debug, Clone, Copy)]
struct Span {
    stream: StreamId,
    first: Seq,
    last: Seq,
}

/// One commit being built: events of one or more streams, with the blobs, receipt, rows and
/// snapshot that go with them. [`Pipeline::push`] adds events; [`Pipeline::send`] sends it.
#[derive(Debug, Default)]
pub(crate) struct Draft {
    commit: Commit,
    spans: Vec<Span>,
}

impl Draft {
    /// An empty draft.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether the draft holds nothing to write.
    pub(crate) fn is_empty(&self) -> bool {
        self.commit == Commit::default()
    }

    /// Adds blobs the events name, so they are durable no later than the events.
    pub(crate) fn blobs(&mut self, blobs: impl IntoIterator<Item = BlobIn>) {
        self.commit.blobs.extend(blobs);
    }

    /// Sets the command's receipt.
    pub(crate) fn receipt(&mut self, receipt: Receipt) {
        self.commit.receipt = Some(receipt);
    }

    /// Sets the thread row to upsert.
    pub(crate) fn thread_row(&mut self, row: ThreadRow) {
        self.commit.thread = Some(row);
    }

    /// Sets the matter row to upsert.
    pub(crate) fn matter_row(&mut self, row: MatterRow) {
        self.commit.matter = Some(row);
    }

    /// Sets the compressed snapshot to store.
    pub(crate) fn snapshot(&mut self, snapshot: SnapshotIn) {
        self.commit.snapshot = Some(snapshot);
    }

    /// The events pushed so far (tests).
    #[cfg(test)]
    pub(crate) fn events(&self) -> &[Envelope] {
        &self.commit.events
    }

    fn span(&self, stream: StreamId) -> Option<&Span> {
        self.spans.iter().find(|span| span.stream == stream)
    }
}

/// The heads of one stream.
#[derive(Debug, Default)]
struct Heads {
    /// The last durable seq.
    durable: Option<Seq>,
    /// The last seq sent; never behind `durable`.
    pending: Option<Seq>,
    /// Commits holding events of the stream that have not been released by `on_result`.
    in_flight: usize,
    /// The stream's first failure since it last drained.
    failure: Option<clauseo_store::Error>,
}

/// A sent commit waiting for its result.
struct InFlight<T> {
    ticket: Ticket,
    spans: Vec<Span>,
    envelopes: Vec<Envelope>,
    tag: T,
    result: Option<Result<Committed, clauseo_store::Error>>,
}

/// A failed commit, as the caller reports it.
#[derive(Debug, Clone)]
pub(crate) struct Failure {
    /// The error to report: the stream's first failure, which is this commit's own error when
    /// `first` is set.
    pub(crate) error: clauseo_store::Error,
    /// Whether this commit is the failure that blocked its streams. A later one failed because
    /// of it (a seq gap); it is answered with `error` but not reported again.
    pub(crate) first: bool,
}

/// One commit released by [`Pipeline::on_result`].
#[derive(Debug)]
pub(crate) struct Done<T> {
    /// What the caller tagged it with at send.
    pub(crate) tag: T,
    /// `Ok` with its events, to apply and publish in this order, or the failure.
    pub(crate) outcome: Result<Vec<Envelope>, Failure>,
}

/// What one result released.
#[derive(Debug)]
pub(crate) struct Completed<T> {
    /// The commits now complete, in send order.
    pub(crate) done: Vec<Done<T>>,
    /// Streams whose in-flight commits have all returned after a failure: their pending head is
    /// the durable head again and they take commits again. Their sent view rolls back.
    pub(crate) unblocked: Vec<StreamId>,
}

/// The pipeline of every stream's commits.
pub(crate) struct Pipeline<T> {
    streams: BTreeMap<StreamId, Heads>,
    in_flight: VecDeque<InFlight<T>>,
    next_ticket: u64,
}

impl<T> Pipeline<T> {
    /// A pipeline whose streams start at `heads`, as `Store::heads` gives them.
    pub(crate) fn new(heads: Vec<(StreamId, Seq)>) -> Self {
        let streams = heads
            .into_iter()
            .map(|(stream, seq)| {
                let heads = Heads {
                    durable: Some(seq),
                    pending: Some(seq),
                    ..Heads::default()
                };
                (stream, heads)
            })
            .collect();
        Self {
            streams,
            in_flight: VecDeque::new(),
            next_ticket: 0,
        }
    }

    /// The last seq sent for `stream`; `None` for a stream with no events.
    pub(crate) fn pending_head(&self, stream: StreamId) -> Option<Seq> {
        self.streams.get(&stream).and_then(|heads| heads.pending)
    }

    /// The last durable seq of `stream`; `None` for a stream with no durable events.
    pub(crate) fn durable_head(&self, stream: StreamId) -> Option<Seq> {
        self.streams.get(&stream).and_then(|heads| heads.durable)
    }

    /// The first failure of `stream` while it drains; the stream takes no commit meanwhile.
    pub(crate) fn blocked(&self, stream: StreamId) -> Option<&clauseo_store::Error> {
        self.streams
            .get(&stream)
            .and_then(|heads| heads.failure.as_ref())
    }

    /// Whether any commit holding events of `stream` has not been released yet.
    pub(crate) fn in_flight(&self, stream: StreamId) -> bool {
        self.streams
            .get(&stream)
            .is_some_and(|heads| heads.in_flight > 0)
    }

    /// Adds `event` to `draft` in `stream` with the next seq (the pending head plus one, or
    /// after the draft's own events of the stream), minting its event id. Fails with the
    /// stream's first failure while it is blocked.
    pub(crate) fn push(
        &self,
        draft: &mut Draft,
        stream: StreamId,
        event: DomainEvent,
        command_id: Option<CommandId>,
        ts: Timestamp,
        ids: &mut Ids,
    ) -> Result<Seq, Error> {
        if let Some(failure) = self.blocked(stream) {
            return Err(Error::Store(failure.clone()));
        }
        let before = match draft.span(stream) {
            Some(span) => Some(span.last),
            None => self.pending_head(stream),
        };
        let seq = match before {
            Some(seq) => seq.next().map_err(|error| {
                Error::Internal(format!("{stream}: no seq after {seq:?}: {error}"))
            })?,
            None => Seq::FIRST,
        };
        let event_id: EventId = ids.mint(ts)?;
        draft
            .commit
            .events
            .push(Envelope::new(event_id, stream, seq, ts, command_id, event));
        match draft.spans.iter_mut().find(|span| span.stream == stream) {
            Some(span) => span.last = seq,
            None => draft.spans.push(Span {
                stream,
                first: seq,
                last: seq,
            }),
        }
        Ok(seq)
    }

    /// Sends `draft` to the feeder and returns its ticket; `on_result` hands `tag` back with
    /// the result. Keeps a copy of the events to release when the result is `Ok`. Fails with a
    /// stream's first failure while that stream is blocked, and with an internal error for a
    /// draft that does not run on from the pending head (two drafts built at once).
    pub(crate) fn send(
        &mut self,
        draft: Draft,
        tag: T,
        feeder: &FeederTx,
    ) -> Result<Ticket, Error> {
        for span in &draft.spans {
            if let Some(failure) = self.blocked(span.stream) {
                return Err(Error::Store(failure.clone()));
            }
            let expected = match self.pending_head(span.stream) {
                Some(seq) => seq.next().ok(),
                None => Some(Seq::FIRST),
            };
            if expected != Some(span.first) {
                return Err(Error::Internal(format!(
                    "{}: draft starts at {:?}, the pending head is {:?}",
                    span.stream,
                    span.first,
                    self.pending_head(span.stream)
                )));
            }
        }
        let ticket = Ticket(self.next_ticket);
        let Draft { commit, spans } = draft;
        let envelopes = commit.events.clone();
        feeder.send(ticket, commit)?;
        self.next_ticket = self.next_ticket.saturating_add(1);
        for span in &spans {
            let heads = self.streams.entry(span.stream).or_default();
            heads.pending = Some(span.last);
            heads.in_flight = heads.in_flight.saturating_add(1);
        }
        self.in_flight.push_back(InFlight {
            ticket,
            spans,
            envelopes,
            tag,
            result: None,
        });
        Ok(ticket)
    }

    /// The events of the in-flight commit `ticket`, as sent; empty once it is released. The
    /// caller folds them into its sent views right after `send`.
    pub(crate) fn events(&self, ticket: Ticket) -> &[Envelope] {
        self.in_flight
            .iter()
            .rev()
            .find(|flight| flight.ticket == ticket)
            .map_or(&[], |flight| flight.envelopes.as_slice())
    }

    /// The events of `stream` in commits sent and not released yet, in send order.
    pub(crate) fn unreleased(&self, stream: StreamId) -> impl Iterator<Item = &Envelope> {
        self.in_flight
            .iter()
            .flat_map(|flight| flight.envelopes.iter())
            .filter(move |envelope| envelope.stream == stream)
    }

    /// One commit's result came back. Returns the commits now complete, in send order, and the
    /// streams that drained after a failure. A result for an unknown ticket releases nothing.
    pub(crate) fn on_result(
        &mut self,
        ticket: Ticket,
        result: Result<Committed, clauseo_store::Error>,
    ) -> Completed<T> {
        let mut completed = Completed {
            done: Vec::new(),
            unblocked: Vec::new(),
        };
        match self
            .in_flight
            .iter_mut()
            .find(|flight| flight.ticket == ticket)
        {
            Some(flight) if flight.result.is_none() => flight.result = Some(result),
            Some(_) | None => {
                tracing::warn!(?ticket, "a commit result with no commit waiting for it");
                return completed;
            }
        }
        while let Some(mut flight) = self.in_flight.pop_front() {
            let Some(result) = flight.result.take() else {
                self.in_flight.push_front(flight);
                break;
            };
            let outcome = match result {
                Ok(_) => {
                    self.committed(&flight);
                    Ok(())
                }
                Err(error) => Err(self.failed(&flight, error)),
            };
            for span in &flight.spans {
                if let Some(heads) = self.streams.get_mut(&span.stream) {
                    heads.in_flight = heads.in_flight.saturating_sub(1);
                    if heads.in_flight == 0 && heads.failure.is_some() {
                        heads.failure = None;
                        heads.pending = heads.durable;
                        completed.unblocked.push(span.stream);
                    }
                }
            }
            completed.done.push(Done {
                tag: flight.tag,
                outcome: outcome.map(|()| flight.envelopes),
            });
        }
        completed
    }

    /// Moves the durable heads of a commit that returned `Ok`.
    fn committed(&mut self, flight: &InFlight<T>) {
        for span in &flight.spans {
            if let Some(heads) = self.streams.get_mut(&span.stream) {
                heads.durable = Some(span.last);
            }
        }
    }

    /// Records a failed commit: the first failure of its streams blocks them; a commit failing
    /// after one reports the earliest failure among its streams.
    fn failed(&mut self, flight: &InFlight<T>, own: clauseo_store::Error) -> Failure {
        let earlier = flight.spans.iter().find_map(|span| {
            self.streams
                .get(&span.stream)
                .and_then(|heads| heads.failure.clone())
        });
        let failure = match earlier {
            Some(error) => Failure {
                error,
                first: false,
            },
            None => Failure {
                error: own,
                first: true,
            },
        };
        for span in &flight.spans {
            if let Some(heads) = self.streams.get_mut(&span.stream)
                && heads.failure.is_none()
            {
                heads.failure = Some(failure.error.clone());
            }
        }
        failure
    }
}

#[cfg(test)]
pub(crate) mod tests;
