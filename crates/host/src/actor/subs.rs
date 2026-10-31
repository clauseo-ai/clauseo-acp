//! Subscriptions: view subscriptions by `(epoch, cursor)`, event subscriptions with replay and
//! one `CaughtUp`, and what happens when a subscriber falls behind.
//!
//! Each subscriber has a `bounded(1024)` channel fed with `try_send`. A full view subscriber
//! gets nothing more until it has drained its channel and asked (`Input::Drained`); then it
//! gets one `Reset`. A full event subscriber likewise waits, then is refilled from the store
//! from the last envelope it got, with no second `CaughtUp`.
//!
//! An event subscriber catches up by reading pages of the store up to the stream's durable
//! head, reading again while the head moves; it gets live envelopes only once it is caught up.
//! So it sees exactly the envelopes after its `after_seq`, in order, once each.

use clauseo_protocol::{Cursor, Envelope, HostEvent, Principal, Seq, StreamId, ViewEpoch};
use clauseo_store::READ_PAGE_MAX;

use super::Actor;
use super::input::{Input, Reply, SubId};
use super::streams::StreamSlot;
use crate::encode::EncodedEvent;
use crate::error::Error;
use crate::handle::{EventSubscription, Sink, Subscription};
use crate::principal::{Access, authorize};

/// One view subscriber of a loaded stream.
pub(crate) struct ViewSub {
    id: SubId,
    sink: Sink<EncodedEvent>,
    /// Its channel was full: it gets nothing until it drains and gets a `Reset`.
    lagged: bool,
}

impl ViewSub {
    /// Offers one live event; returns whether the subscriber stays.
    pub(crate) fn offer(&mut self, event: &EncodedEvent) -> bool {
        if self.lagged {
            return true;
        }
        match self.sink.items.try_send(event.clone()) {
            Ok(()) => true,
            Err(async_channel::TrySendError::Full(_)) => {
                self.lagged = true;
                self.sink.lag.raise();
                true
            }
            Err(async_channel::TrySendError::Closed(_)) => false,
        }
    }
}

/// Where an event subscriber is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Caught up: it gets each returned commit's envelopes.
    Live,
    /// A page read from the store is in flight.
    Reading,
    /// Its channel was full; it waits until it has drained.
    Waiting,
}

/// One event subscriber.
pub(crate) struct EventSub {
    id: SubId,
    sink: Sink<HostEvent>,
    /// The last seq it was sent (or asked to start after).
    delivered: Option<Seq>,
    /// Whether its `CaughtUp` is still to send.
    owes_caught_up: bool,
    mode: Mode,
}

/// What a `try_send` to an event subscriber gave.
enum Sent {
    Ok,
    Full,
    Closed,
}

impl EventSub {
    fn send(&mut self, event: HostEvent) -> Sent {
        match self.sink.items.try_send(event) {
            Ok(()) => Sent::Ok,
            Err(async_channel::TrySendError::Full(_)) => {
                self.mode = Mode::Waiting;
                self.sink.lag.raise();
                Sent::Full
            }
            Err(async_channel::TrySendError::Closed(_)) => Sent::Closed,
        }
    }
}

fn next_sub_id(actor: &mut Actor) -> SubId {
    actor.next_sub += 1;
    SubId(actor.next_sub)
}

/// Whether the host knows `stream`: `command.unknown_thread` or `command.unknown_matter`.
pub(crate) fn check_stream(actor: &Actor, stream: StreamId) -> Result<(), Error> {
    match stream {
        StreamId::Thread(thread) => actor.check_thread(thread).map(|_| ()),
        StreamId::Matter(matter) => actor.check_matter(matter).map(|_| ()),
    }
}

/// `subscribe`: the tail or a `Reset`, then live patches.
pub(super) fn subscribe(
    actor: &mut Actor,
    principal: &Principal,
    stream: StreamId,
    after: Option<(ViewEpoch, Cursor)>,
    reply: Reply<Subscription>,
) {
    if let Err(error) =
        authorize(principal, Access::Subscribe).and_then(|()| actor.check_stream(stream))
    {
        let _gone = reply.send(Err(error));
        return;
    }
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let made = loaded.and_then(|()| add_view_sub(actor, stream, after));
            if let Err(subscription) = reply.send(made) {
                // The caller is gone; drop its subscription at once.
                drop(subscription);
            }
            actor.release(stream);
        }),
    );
}

fn add_view_sub(
    actor: &mut Actor,
    stream: StreamId,
    after: Option<(ViewEpoch, Cursor)>,
) -> Result<Subscription, Error> {
    let id = next_sub_id(actor);
    let (subscription, sink) = Subscription::new(actor.post.clone(), stream, id);
    let Some(StreamSlot::Loaded(loaded)) = actor.streams.get_mut(&stream) else {
        return Err(Error::Internal(format!("{stream} is not loaded")));
    };
    let first = loaded
        .view
        .catch_up(after, &loaded.state, &mut actor.encoder)?;
    let mut sub = ViewSub {
        id,
        sink,
        lagged: false,
    };
    for event in &first {
        sub.offer(event);
    }
    loaded.views.push(sub);
    Ok(subscription)
}

/// `subscribe_events`: replay after `after_seq`, `CaughtUp`, then live envelopes.
pub(super) fn subscribe_events(
    actor: &mut Actor,
    principal: &Principal,
    stream: StreamId,
    after_seq: Option<Seq>,
    reply: Reply<EventSubscription>,
) {
    if let Err(error) =
        authorize(principal, Access::SubscribeEvents).and_then(|()| actor.check_stream(stream))
    {
        let _gone = reply.send(Err(error));
        return;
    }
    let id = next_sub_id(actor);
    let (subscription, sink) = EventSubscription::new(actor.post.clone(), stream, id);
    actor.event_subs.entry(stream).or_default().push(EventSub {
        id,
        sink,
        delivered: after_seq,
        owes_caught_up: true,
        mode: Mode::Live,
    });
    if reply.send(Ok(subscription)).is_ok() {
        pump(actor, stream, id);
    }
}

/// Moves an event subscriber on: read the next page while it is behind the durable head,
/// else send its `CaughtUp` if it still owes one and go live.
fn pump(actor: &mut Actor, stream: StreamId, id: SubId) {
    let head = actor.pipeline.durable_head(stream);
    let Some(subs) = actor.event_subs.get_mut(&stream) else {
        return;
    };
    let Some(index) = subs.iter().position(|sub| sub.id == id) else {
        return;
    };
    let Some(sub) = subs.get_mut(index) else {
        return;
    };
    if sub.mode != Mode::Live {
        return;
    }
    if sub.delivered < head {
        sub.mode = Mode::Reading;
        let after = sub.delivered;
        let store = actor.store.clone();
        actor.spawn_post(async move {
            let result = store.read_after(stream, after, READ_PAGE_MAX).await;
            Input::EventPage { stream, id, result }
        });
        return;
    }
    if !sub.owes_caught_up {
        return;
    }
    let Some(last_seq) = head else {
        // A stream with no durable event yet: the first live envelope comes first.
        return;
    };
    match sub.send(HostEvent::CaughtUp { stream, last_seq }) {
        Sent::Ok => sub.owes_caught_up = false,
        Sent::Full => {}
        Sent::Closed => {
            subs.remove(index);
        }
    }
}

/// A page of stored envelopes for a catching-up subscriber.
pub(super) fn on_event_page(
    actor: &mut Actor,
    stream: StreamId,
    id: SubId,
    result: Result<Vec<Envelope>, clauseo_store::Error>,
) {
    let head = actor.pipeline.durable_head(stream);
    let Some(subs) = actor.event_subs.get_mut(&stream) else {
        return;
    };
    let Some(index) = subs.iter().position(|sub| sub.id == id) else {
        return;
    };
    let Some(sub) = subs.get_mut(index) else {
        return;
    };
    let delivered = sub.delivered;
    let page: Vec<Envelope> = match result {
        Ok(page) => page
            .into_iter()
            .filter(|envelope| Some(envelope.seq) > delivered && Some(envelope.seq) <= head)
            .collect(),
        Err(error) => {
            tracing::warn!(%stream, %error, "an event subscription could not read the store");
            subs.remove(index);
            return;
        }
    };
    let Some(last) = page.last().map(|envelope| envelope.seq) else {
        // The store holds less than the durable head: end the subscription rather than
        // leave a gap.
        tracing::error!(%stream, "the store ended before the durable head");
        subs.remove(index);
        return;
    };
    match sub.send(HostEvent::Events(page)) {
        Sent::Ok => {
            sub.delivered = Some(last);
            sub.mode = Mode::Live;
            pump(actor, stream, id);
        }
        Sent::Full => {}
        Sent::Closed => {
            subs.remove(index);
        }
    }
}

/// Hands a returned commit's envelopes of `stream` (`batch`, all of that stream, in seq order)
/// to its live event subscribers. A subscriber that has none of them yet (the usual case) gets
/// the batch itself: cloned for all but the last such subscriber, which takes it. One that has
/// some already gets only the rest.
pub(super) fn deliver_events(actor: &mut Actor, stream: StreamId, batch: Vec<Envelope>) {
    let Some(subs) = actor.event_subs.get_mut(&stream) else {
        return;
    };
    let (Some(first), Some(last)) = (
        batch.first().map(|envelope| envelope.seq),
        batch.last().map(|envelope| envelope.seq),
    ) else {
        return;
    };
    let mut closed: Vec<SubId> = Vec::new();
    let mut whole: Vec<SubId> = Vec::new();
    for sub in subs.iter_mut().filter(|sub| sub.mode == Mode::Live) {
        if sub.delivered < Some(first) {
            whole.push(sub.id);
            continue;
        }
        let rest: Vec<Envelope> = batch
            .iter()
            .filter(|envelope| Some(envelope.seq) > sub.delivered)
            .cloned()
            .collect();
        if let Some(seq) = rest.last().map(|envelope| envelope.seq) {
            offer(sub, rest, seq, &mut closed);
        }
    }
    let mut batch = Some(batch);
    let count = whole.len();
    for (index, id) in whole.into_iter().enumerate() {
        let events = if index + 1 == count {
            batch.take()
        } else {
            batch.clone()
        };
        if let (Some(events), Some(sub)) = (events, subs.iter_mut().find(|sub| sub.id == id)) {
            offer(sub, events, last, &mut closed);
        }
    }
    subs.retain(|sub| !closed.contains(&sub.id));
    let owing: Vec<SubId> = subs
        .iter()
        .filter(|sub| sub.owes_caught_up && sub.mode == Mode::Live)
        .map(|sub| sub.id)
        .collect();
    for id in owing {
        pump(actor, stream, id);
    }
}

/// Sends one batch ending at `last` to a live subscriber; a closed one goes in `closed`.
fn offer(sub: &mut EventSub, events: Vec<Envelope>, last: Seq, closed: &mut Vec<SubId>) {
    match sub.send(HostEvent::Events(events)) {
        Sent::Ok => sub.delivered = Some(last),
        Sent::Full => {}
        Sent::Closed => closed.push(sub.id),
    }
}

/// A subscriber drained its channel after falling behind.
pub(super) fn on_drained(actor: &mut Actor, stream: StreamId, id: SubId) {
    if let Some(StreamSlot::Loaded(loaded)) = actor.streams.get_mut(&stream)
        && let Some(sub) = loaded.views.iter_mut().find(|sub| sub.id == id)
    {
        if sub.lagged {
            match loaded.view.reset(&loaded.state, &mut actor.encoder) {
                Ok(reset) => {
                    sub.lagged = false;
                    sub.offer(&reset);
                }
                Err(error) => tracing::error!(%stream, %error, "a reset did not encode"),
            }
        }
        return;
    }
    let waiting = actor
        .event_subs
        .get_mut(&stream)
        .and_then(|subs| subs.iter_mut().find(|sub| sub.id == id))
        .filter(|sub| sub.mode == Mode::Waiting);
    if let Some(sub) = waiting {
        sub.mode = Mode::Live;
        pump(actor, stream, id);
    }
}

/// A subscription was dropped.
pub(super) fn on_unsubscribed(actor: &mut Actor, stream: StreamId, id: SubId) {
    if let Some(StreamSlot::Loaded(loaded)) = actor.streams.get_mut(&stream) {
        loaded.views.retain(|sub| sub.id != id);
    }
    if let Some(subs) = actor.event_subs.get_mut(&stream) {
        subs.retain(|sub| sub.id != id);
        if subs.is_empty() {
            actor.event_subs.remove(&stream);
        }
    }
    actor.release(stream);
}

/// `ack`: recorded only.
pub(super) fn ack(actor: &mut Actor, stream: StreamId, epoch: ViewEpoch, cursor: Cursor) {
    if let Some(StreamSlot::Loaded(loaded)) = actor.streams.get_mut(&stream) {
        loaded.ack = Some((epoch, cursor));
    }
}
