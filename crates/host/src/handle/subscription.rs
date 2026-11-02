//! The three subscription streams a handle returns. Each reads a `bounded(1024)` channel the
//! actor fills with `try_send`. When the channel was full the actor stops sending and raises a
//! flag; the stream, once it has drained the channel, asks the actor to catch it up (a `Reset`
//! for a view subscription, a refill from the store for an event subscription, the current state
//! for an app subscription, the current limits view for a limits subscription).
//!
//! An app subscription also reads a [`Front`] queue before its channel: the current state the
//! actor puts there whole (its first frames, and each catch-up), which may hold more events than
//! the channel has places, so a catch-up always arrives whole however many matters there are.
//! While any of it is unread, the actor queues what follows behind it, up to the channel's
//! capacity more; past that the subscriber lags again and is caught up once it has read it all.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};

use clauseo_protocol::{HostEvent, StreamId};
use futures::Stream;
use futures::task::AtomicWaker;

use crate::actor::input::{Input, SubId};
use crate::actor::limits::LimitsInput;
use crate::encode::EncodedEvent;

/// The capacity of each subscription's channel.
pub(crate) const SUBSCRIPTION_CAPACITY: usize = 1024;

/// What the actor and a subscription share: the "fell behind" flag and the waker to call when
/// the actor raises it.
#[derive(Debug, Default)]
pub(crate) struct Lag {
    lagged: AtomicBool,
    waker: AtomicWaker,
}

impl Lag {
    /// The actor's side: the channel was full, so ask the subscription to call back once it
    /// has drained what it holds.
    pub(crate) fn raise(&self) {
        self.lagged.store(true, Ordering::Release);
        self.waker.wake();
    }

    /// The subscription's side: whether the flag was up, lowering it.
    fn take(&self) -> bool {
        self.lagged.swap(false, Ordering::AcqRel)
    }
}

/// The queue an app subscription reads before its channel: a catch-up, whole, and what the actor
/// sent behind it while it was unread. Shared by the actor and the subscription; both hold the
/// lock only to move events, so whether the queue is empty and what is pushed or popped are
/// decided together, and nothing queued behind a catch-up can overtake it through the channel.
#[derive(Debug, Default)]
pub(crate) struct Front {
    queue: Mutex<VecDeque<EncodedEvent>>,
    waker: AtomicWaker,
}

impl Front {
    fn lock(&self) -> MutexGuard<'_, VecDeque<EncodedEvent>> {
        // A panic while holding the lock leaves a queue of whole events: still usable.
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The actor's side: `events` replace what is queued, and the subscription is woken. The
    /// actor calls it only when the channel is empty (a new subscriber, or one that drained it).
    pub(crate) fn fill(&self, events: Vec<EncodedEvent>) {
        let mut queue = self.lock();
        queue.clear();
        queue.extend(events);
        drop(queue);
        self.waker.wake();
    }

    /// The actor's side: queues `event` behind a catch-up still being read. `None` when nothing
    /// is queued (the event goes through the channel); `Some(false)` when `limit` events are
    /// already queued (the subscriber lags).
    pub(crate) fn push_behind(&self, event: &EncodedEvent, limit: usize) -> Option<bool> {
        let mut queue = self.lock();
        if queue.is_empty() {
            return None;
        }
        if queue.len() >= limit {
            return Some(false);
        }
        queue.push_back(event.clone());
        drop(queue);
        self.waker.wake();
        Some(true)
    }

    fn pop(&self) -> Option<EncodedEvent> {
        self.lock().pop_front()
    }
}

/// What a subscription follows: one stream, the app-wide events, or the limits.
#[derive(Debug, Clone, Copy)]
enum Topic {
    Stream(StreamId),
    App,
    Limits,
}

impl Topic {
    /// What to tell the actor once the subscription has drained after a lag.
    fn drained(self, id: SubId) -> Input {
        match self {
            Self::Stream(stream) => Input::Drained { stream, id },
            Self::App => Input::AppDrained { id },
            Self::Limits => Input::Limits(LimitsInput::Drained { id }),
        }
    }

    /// What to tell the actor when the subscription is dropped.
    fn unsubscribed(self, id: SubId) -> Input {
        match self {
            Self::Stream(stream) => Input::Unsubscribed { stream, id },
            Self::App => Input::AppUnsubscribed { id },
            Self::Limits => Input::Limits(LimitsInput::Unsubscribed { id }),
        }
    }
}

/// The parts every subscription kind holds.
#[derive(Debug)]
struct Feed<T> {
    items: Pin<Box<async_channel::Receiver<T>>>,
    lag: Arc<Lag>,
    actor: async_channel::Sender<Input>,
    topic: Topic,
    id: SubId,
}

impl<T> Feed<T> {
    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        match self.items.as_mut().poll_next(cx) {
            Poll::Ready(item) => Poll::Ready(item),
            Poll::Pending => {
                // Register first, then read the flag, so a raise between the two still wakes us.
                self.lag.waker.register(cx.waker());
                if self.lag.take() && self.actor.try_send(self.topic.drained(self.id)).is_err() {
                    // The host is gone: nothing more will come.
                    return Poll::Ready(None);
                }
                Poll::Pending
            }
        }
    }
}

impl<T> Drop for Feed<T> {
    fn drop(&mut self) {
        // The host may already be gone; then there is nobody to tell.
        let _gone = self.actor.try_send(self.topic.unsubscribed(self.id));
    }
}

/// A view subscription: the stream's patches as [`EncodedEvent`]s, each a `HostEvent::Patches`
/// serialised once and shared by every subscriber. It starts with the tail after the position
/// asked for, or a `Reset`. Ends when the host stops.
///
/// `HostHandle::subscribe_limits` returns one too, following the limits instead of a stream:
/// first the current `LimitsViewChanged`, then for each change one `AccountLimitsChanged` per
/// changed account and one `LimitsViewChanged`. One whose channel was full gets nothing until
/// it has drained it, then the current `LimitsViewChanged`.
#[derive(Debug)]
pub struct Subscription(Feed<EncodedEvent>);

/// An event subscription: `HostEvent::Events` with the stream's envelopes in `seq` order, no
/// gap and no duplicate, and one `HostEvent::CaughtUp` between the replayed and the live ones.
/// Ends when the host stops. It also ends, with the host still up, when a store read failed
/// while replaying or refilling it: subscribe again with `after_seq` set to the last `seq` seen.
#[derive(Debug)]
pub struct EventSubscription(Feed<HostEvent>);

/// An app subscription: the app-wide `HostEvent`s that belong to no stream, as
/// [`EncodedEvent`]s, each serialised once and shared by every app subscriber: `SettingsChanged`
/// after every change of the settings, `MattersChanged` when matters on the list change, come or
/// go, and `ThreadsChanged` when a matter's threads change or come; notices are to come here too
/// (the limits have their own subscription). A subscription carries no window identity, so
/// every app subscriber gets every app event: a window that wants only the summaries of threads
/// it has not opened drops the others itself. No stream's patches or envelopes ever come here,
/// and no app event goes to a stream's subscribers.
///
/// It starts with the current state, whole: one `SettingsChanged`, one `MattersChanged` with
/// `all` set, and one `ThreadsChanged` with `all` set per matter in the list's order, so a
/// window needs no separate read and no change falls between the two; the partial rows that
/// follow build on it. A subscriber whose channel is full gets nothing more until it has drained
/// it, then gets the same whole state at once, so it misses steps but never ends on a stale
/// view. Ends when the host stops; dropping it unsubscribes.
#[derive(Debug)]
pub struct AppSubscription {
    feed: Feed<EncodedEvent>,
    front: Arc<Front>,
}

/// The actor's end of a new subscription: the sender it fills and the shared flag.
pub(crate) struct Sink<T> {
    pub(crate) items: async_channel::Sender<T>,
    pub(crate) lag: Arc<Lag>,
}

/// Builds the two ends of a subscription of `topic` with id `id`.
fn feed<T>(
    actor: async_channel::Sender<Input>,
    topic: Topic,
    id: SubId,
    capacity: usize,
) -> (Feed<T>, Sink<T>) {
    let (tx, rx) = async_channel::bounded(capacity);
    let lag = Arc::new(Lag::default());
    let sink = Sink {
        items: tx,
        lag: Arc::clone(&lag),
    };
    let feed = Feed {
        items: Box::pin(rx),
        lag,
        actor,
        topic,
        id,
    };
    (feed, sink)
}

impl Subscription {
    /// A view subscription and the actor's end of it.
    pub(crate) fn new(
        actor: async_channel::Sender<Input>,
        stream: StreamId,
        id: SubId,
    ) -> (Self, Sink<EncodedEvent>) {
        let (feed, sink) = feed(actor, Topic::Stream(stream), id, SUBSCRIPTION_CAPACITY);
        (Self(feed), sink)
    }

    /// A limits subscription and the actor's end of it.
    pub(crate) fn limits(
        actor: async_channel::Sender<Input>,
        id: SubId,
    ) -> (Self, Sink<EncodedEvent>) {
        let (feed, sink) = feed(actor, Topic::Limits, id, SUBSCRIPTION_CAPACITY);
        (Self(feed), sink)
    }
}

impl AppSubscription {
    /// An app subscription with a channel of `capacity`, and the actor's ends of it: the channel
    /// and the front queue.
    pub(crate) fn new(
        actor: async_channel::Sender<Input>,
        id: SubId,
        capacity: usize,
    ) -> (Self, Sink<EncodedEvent>, Arc<Front>) {
        let (feed, sink) = feed(actor, Topic::App, id, capacity);
        let front = Arc::new(Front::default());
        let subscription = Self {
            feed,
            front: Arc::clone(&front),
        };
        (subscription, sink, front)
    }
}

impl EventSubscription {
    /// An event subscription and the actor's end of it.
    pub(crate) fn new(
        actor: async_channel::Sender<Input>,
        stream: StreamId,
        id: SubId,
    ) -> (Self, Sink<HostEvent>) {
        let (feed, sink) = feed(actor, Topic::Stream(stream), id, SUBSCRIPTION_CAPACITY);
        (Self(feed), sink)
    }
}

impl Stream for Subscription {
    type Item = EncodedEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<EncodedEvent>> {
        self.get_mut().0.poll_next(cx)
    }
}

impl Stream for AppSubscription {
    type Item = EncodedEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<EncodedEvent>> {
        let this = self.get_mut();
        // Register first, then look, so a fill between the two still wakes us.
        this.front.waker.register(cx.waker());
        if let Some(event) = this.front.pop() {
            return Poll::Ready(Some(event));
        }
        this.feed.poll_next(cx)
    }
}

impl Stream for EventSubscription {
    type Item = HostEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<HostEvent>> {
        self.get_mut().0.poll_next(cx)
    }
}
