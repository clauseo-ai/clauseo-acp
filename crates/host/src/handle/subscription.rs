//! The three subscription streams a handle returns. Each reads a `bounded(1024)` channel the
//! actor fills with `try_send`. When the channel was full the actor stops sending and raises a
//! flag; the stream, once it has drained the channel, asks the actor to catch it up (a `Reset`
//! for a view subscription, a refill from the store for an event subscription, the current state
//! for an app subscription).

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use clauseo_protocol::{HostEvent, StreamId};
use futures::Stream;
use futures::task::AtomicWaker;

use crate::actor::input::{Input, SubId};
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

/// What a subscription follows: one stream, or the app-wide events.
#[derive(Debug, Clone, Copy)]
enum Topic {
    Stream(StreamId),
    App,
}

impl Topic {
    /// What to tell the actor once the subscription has drained after a lag.
    fn drained(self, id: SubId) -> Input {
        match self {
            Self::Stream(stream) => Input::Drained { stream, id },
            Self::App => Input::AppDrained { id },
        }
    }

    /// What to tell the actor when the subscription is dropped.
    fn unsubscribed(self, id: SubId) -> Input {
        match self {
            Self::Stream(stream) => Input::Unsubscribed { stream, id },
            Self::App => Input::AppUnsubscribed { id },
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
#[derive(Debug)]
pub struct Subscription(Feed<EncodedEvent>);

/// An event subscription: `HostEvent::Events` with the stream's envelopes in `seq` order, no
/// gap and no duplicate, and one `HostEvent::CaughtUp` between the replayed and the live ones.
/// Ends when the host stops. It also ends, with the host still up, when a store read failed
/// while replaying or refilling it: subscribe again with `after_seq` set to the last `seq` seen.
#[derive(Debug)]
pub struct EventSubscription(Feed<HostEvent>);

/// An app subscription: the app-wide `HostEvent`s that belong to no stream, as
/// [`EncodedEvent`]s, each serialised once and shared by every app subscriber. Today that is
/// `SettingsChanged`, after every change of the settings; the matter list, summaries of threads
/// no window has open, notices and `AccountLimitsChanged` are to come here too. No stream's
/// patches or envelopes ever come here, and no app event goes to a stream's subscribers.
///
/// It starts with nothing: subscribe first, then read the current state (`GetSettings`); the
/// host answers calls in order, so no change falls between the two. A subscriber whose channel
/// is full gets nothing more until it has drained it, then gets the current state at once (one
/// `SettingsChanged`), so it misses steps but never ends on a stale view. Ends when the host
/// stops; dropping it unsubscribes.
#[derive(Debug)]
pub struct AppSubscription(Feed<EncodedEvent>);

/// The actor's end of a new subscription: the sender it fills and the shared flag.
pub(crate) struct Sink<T> {
    pub(crate) items: async_channel::Sender<T>,
    pub(crate) lag: Arc<Lag>,
}

/// Builds the two ends of a subscription of `topic` with id `id`.
fn feed<T>(actor: async_channel::Sender<Input>, topic: Topic, id: SubId) -> (Feed<T>, Sink<T>) {
    let (tx, rx) = async_channel::bounded(SUBSCRIPTION_CAPACITY);
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
        let (feed, sink) = feed(actor, Topic::Stream(stream), id);
        (Self(feed), sink)
    }
}

impl AppSubscription {
    /// An app subscription and the actor's end of it.
    pub(crate) fn new(
        actor: async_channel::Sender<Input>,
        id: SubId,
    ) -> (Self, Sink<EncodedEvent>) {
        let (feed, sink) = feed(actor, Topic::App, id);
        (Self(feed), sink)
    }
}

impl EventSubscription {
    /// An event subscription and the actor's end of it.
    pub(crate) fn new(
        actor: async_channel::Sender<Input>,
        stream: StreamId,
        id: SubId,
    ) -> (Self, Sink<HostEvent>) {
        let (feed, sink) = feed(actor, Topic::Stream(stream), id);
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
        self.get_mut().0.poll_next(cx)
    }
}

impl Stream for EventSubscription {
    type Item = HostEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<HostEvent>> {
        self.get_mut().0.poll_next(cx)
    }
}
