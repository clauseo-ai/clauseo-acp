//! Building a commit of a thread stream for the turn path. Every draft that holds a thread
//! event starts with that thread's held text, flushed display first and then durable, so the
//! text keeps its place before the event (the coalescer's order rule).

use clauseo_protocol::{CommandId, DomainEvent, MatterId, Seq, StreamId, ThreadId, Timestamp};

use super::super::{Actor, Tag};
use super::{Then, TurnTag, end, text};
use crate::error::Error;
use crate::pipeline::Draft;

/// A commit being built for one thread.
pub(super) struct Out {
    pub(super) draft: Draft,
    thread: ThreadId,
    command: Option<CommandId>,
    ts: Timestamp,
}

impl Out {
    /// A draft for `thread`, holding its flushed text first. `command` marks the events of a
    /// command's commit.
    pub(super) fn new(
        actor: &mut Actor,
        thread: ThreadId,
        command: Option<CommandId>,
    ) -> Result<Self, Error> {
        let mut draft = Draft::new();
        flush_text(actor, thread, &mut draft)?;
        Ok(Self {
            draft,
            thread,
            command,
            ts: actor.now().wall,
        })
    }

    /// Adds an event to the thread's stream.
    pub(super) fn push(
        &mut self,
        actor: &mut Actor,
        event: impl Into<DomainEvent>,
    ) -> Result<Seq, Error> {
        let stream = StreamId::Thread(self.thread);
        actor.pipeline.push(
            &mut self.draft,
            stream,
            event.into(),
            self.command,
            self.ts,
            &mut actor.ids,
        )
    }

    /// Adds an event to a matter's stream.
    pub(super) fn push_matter(
        &mut self,
        actor: &mut Actor,
        matter: MatterId,
        event: impl Into<DomainEvent>,
    ) -> Result<Seq, Error> {
        actor.pipeline.push(
            &mut self.draft,
            StreamId::Matter(matter),
            event.into(),
            self.command,
            self.ts,
            &mut actor.ids,
        )
    }

    /// Sends the commit with a turn tag, unless it holds nothing.
    pub(super) fn send(self, actor: &mut Actor, then: Then) -> Result<(), Error> {
        if self.draft.is_empty() {
            return Ok(());
        }
        let thread = self.thread;
        let tag = Tag::Turn(TurnTag { thread, then });
        actor.send_commit(self.draft, tag)?;
        end::check_abort(actor, thread);
        Ok(())
    }

    /// Sends the commit with `tag` (a command's), even when it holds only a receipt.
    pub(super) fn send_tagged(self, actor: &mut Actor, tag: Tag) -> Result<(), Error> {
        let thread = self.thread;
        actor.send_commit(self.draft, tag)?;
        end::check_abort(actor, thread);
        Ok(())
    }
}

/// Moves `thread`'s held text into `draft` as `TextAppended` events, after showing what the
/// display has not shown yet. Every commit of a thread stream runs it first. While the stream
/// is blocked it fails and keeps the text.
pub(crate) fn flush_text(
    actor: &mut Actor,
    thread: ThreadId,
    draft: &mut Draft,
) -> Result<(), Error> {
    let held = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| entry.turn.text.has_durable());
    if !held {
        return Ok(());
    }
    // A blocked stream refuses the push: the text stays held for the commit after the unblock.
    if let Some(failure) = actor.pipeline.blocked(StreamId::Thread(thread)) {
        return Err(Error::Store(failure.clone()));
    }
    let appended = text::take_durable(actor, thread)?;
    let ts = actor.now().wall;
    for event in appended {
        actor.pipeline.push(
            draft,
            StreamId::Thread(thread),
            DomainEvent::TextAppended(event),
            None,
            ts,
            &mut actor.ids,
        )?;
    }
    Ok(())
}
