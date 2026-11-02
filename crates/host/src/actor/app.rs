//! App subscriptions: the app-wide `HostEvent`s of no stream, sent to every app subscriber and
//! to no stream's subscribers. `HostHandle::subscribe_app`'s doc and `AppSubscription`'s give
//! the rules a caller sees; this is how the actor keeps them.
//!
//! The rules follow `subscribe`: the principal is checked first (`Access::SubscribeApp`); each
//! subscriber has its own `bounded(1024)` channel filled with `try_send`, so one slow window
//! never holds up the actor or another window; a full subscriber is marked lagged and gets
//! nothing until it has drained and asked (`Input::AppDrained`), then gets the current state
//! ([`catch_up`]), as a view subscriber gets a `Reset`; a new subscriber gets the same current
//! state as its first frames, so a window never reads it separately; a closed channel (the
//! subscription was dropped) removes the subscriber at the next send, and
//! `Input::AppUnsubscribed` at once. The actor owns every sink, so when the host stops and the
//! actor is dropped every app stream ends.
//!
//! The current state never goes through the bounded channel: it goes whole into the
//! subscription's front queue (`handle::Front`), which the subscription reads first, so the first
//! frames and every catch-up arrive whole however many matters there are (one `ThreadsChanged`
//! each), even past the channel's capacity. What is sent while some of it is unread queues behind
//! it, up to a channel's worth; past that the subscriber lags as it would on a full channel.
//!
//! Each event is serialised once ([`publish`]) and the same `EncodedEvent` goes to every
//! subscriber. What publishes here: `SettingsChanged` after each successful settings write
//! (`settings.rs`), and `MattersChanged` and `ThreadsChanged` when a row of the rail changes
//! (`app_lists.rs`). Still to come, each through [`publish`] and each adding its part of the
//! current state to [`catch_up`]: notices. The limits have their own subscribers (`limits.rs`).
//! `subscribe_app` carries no window identity, so the host sends every changed thread summary
//! to every app subscriber and each window drops those of threads it has open.
//!
//! The current state ([`catch_up`]) is whole: one `SettingsChanged`, one `MattersChanged` with
//! `all` and one `ThreadsChanged` with `all` per matter, each replacing what a window holds. The
//! partial rows that follow build on it, so a subscriber that missed some (it lagged) gets it
//! again before any partial row. An event added here must keep that: whole in the catch-up.

use std::sync::Arc;

use clauseo_protocol::{HostEvent, Principal};

use super::Actor;
use super::input::{Reply, SubId};
use crate::encode::EncodedEvent;
use crate::error::Error;
use crate::handle::{AppSubscription, Front, Sink};
use crate::principal::{Access, authorize};

/// One app subscriber; a limits subscriber (`limits.rs`) is one too, with the same rules.
pub(crate) struct AppSub {
    id: SubId,
    sink: Sink<EncodedEvent>,
    /// The queue it reads before its channel: its catch-ups, whole. An app subscriber has one;
    /// a limits subscriber, whose catch-up is one event, does not.
    front: Option<Arc<Front>>,
    /// How many events its front queue may hold: the last catch-up and a channel's worth more.
    front_limit: usize,
    /// Its channel (or its front queue) was full: it gets nothing until it drains and is caught
    /// up.
    lagged: bool,
}

impl AppSub {
    /// A subscriber that is not lagged, with no front queue: every event, catch-ups included,
    /// goes through its channel (a limits subscriber, whose catch-up is one event).
    pub(super) fn new(id: SubId, sink: Sink<EncodedEvent>) -> Self {
        Self {
            id,
            sink,
            front: None,
            front_limit: 0,
            lagged: false,
        }
    }

    /// An app subscriber, whose catch-ups go whole through `front` ([`Self::catch_up_whole`]).
    fn with_front(id: SubId, sink: Sink<EncodedEvent>, front: Arc<Front>) -> Self {
        Self {
            front: Some(front),
            ..Self::new(id, sink)
        }
    }

    /// Its number.
    pub(super) fn id(&self) -> SubId {
        self.id
    }

    /// It drained its channel: it is no longer lagged and gets `events`, the current state,
    /// through its channel. Returns whether it stays. For a subscriber with no front queue.
    pub(super) fn catch_up(&mut self, events: &[EncodedEvent]) -> bool {
        self.lagged = false;
        events.iter().all(|event| self.offer(event))
    }

    /// Offers one event; returns whether the subscriber stays. Behind a catch-up still unread in
    /// its front queue it goes there, after it; otherwise through the channel.
    pub(super) fn offer(&mut self, event: &EncodedEvent) -> bool {
        if self.sink.items.is_closed() {
            return false;
        }
        if self.lagged {
            return true;
        }
        match self
            .front
            .as_ref()
            .and_then(|front| front.push_behind(event, self.front_limit))
        {
            Some(true) => return true,
            Some(false) => {
                self.lag();
                return true;
            }
            None => {}
        }
        match self.sink.items.try_send(event.clone()) {
            Ok(()) => true,
            Err(async_channel::TrySendError::Full(_)) => {
                self.lag();
                true
            }
            Err(async_channel::TrySendError::Closed(_)) => false,
        }
    }

    /// It gets nothing more until it has read all it holds and asked to be caught up.
    fn lag(&mut self) {
        self.lagged = true;
        self.sink.lag.raise();
    }

    /// Puts the current state, whole, in its front queue, whatever its channel's capacity, and
    /// sends to it again. Called when its channel is empty: when it subscribes, and when it has
    /// drained after a lag (nothing was sent to it meanwhile). Returns whether it stays.
    /// A subscriber with no front queue takes them through its channel ([`Self::catch_up`]).
    fn catch_up_whole(&mut self, events: Vec<EncodedEvent>, capacity: usize) -> bool {
        if self.sink.items.is_closed() {
            return false;
        }
        let Some(front) = self.front.as_ref() else {
            return self.catch_up(&events);
        };
        self.front_limit = events.len().saturating_add(capacity);
        front.fill(events);
        self.lagged = false;
        true
    }
}

/// `subscribe_app`: the principal check, then a subscriber that gets every later app event.
pub(super) fn subscribe(actor: &mut Actor, principal: &Principal, reply: Reply<AppSubscription>) {
    if let Err(error) = authorize(principal, Access::SubscribeApp) {
        let _gone = reply.send(Err(error));
        return;
    }
    let id = super::subs::next_sub_id(actor);
    let (subscription, sink, front) =
        AppSubscription::new(actor.post.clone(), id, actor.app_capacity);
    if reply.send(Ok(subscription)).is_err() {
        return;
    }
    let mut sub = AppSub::with_front(id, sink, front);
    // The first frames: the current state, whole, before any change.
    let events = catch_up(actor);
    if sub.catch_up_whole(events, actor.app_capacity) {
        actor.app_subs.push(sub);
    }
}

/// Sends `event` to every app subscriber, serialised once. An event that does not serialise
/// is a host bug: it is logged and goes to nobody.
pub(crate) fn publish(actor: &mut Actor, event: &HostEvent) {
    if actor.app_subs.is_empty() {
        return;
    }
    match encode(event) {
        Ok(encoded) => actor.app_subs.retain_mut(|sub| sub.offer(&encoded)),
        Err(error) => tracing::error!(%error, "an app event did not serialise"),
    }
}

/// A lagged subscriber drained its channel and front queue: send it the current state.
pub(super) fn on_drained(actor: &mut Actor, id: SubId) {
    let Some(index) = actor.app_subs.iter().position(|sub| sub.id == id) else {
        return;
    };
    let events = catch_up(actor);
    let capacity = actor.app_capacity;
    let stays = actor
        .app_subs
        .get_mut(index)
        .is_some_and(|sub| sub.catch_up_whole(events, capacity));
    if !stays {
        actor.app_subs.remove(index);
    }
}

/// An app subscription was dropped.
pub(super) fn on_unsubscribed(actor: &mut Actor, id: SubId) {
    actor.app_subs.retain(|sub| sub.id != id);
}

/// The current state, as the events that start a subscription and bring a lagged subscriber up
/// to date: the settings, the matter list and each matter's thread list. Each event here must
/// replace what a window holds whole; see the module doc before adding a partial one.
fn catch_up(actor: &mut Actor) -> Vec<EncodedEvent> {
    let mut events = vec![HostEvent::SettingsChanged(super::settings::view(
        &actor.effective,
    ))];
    events.extend(super::app_lists::current(actor));
    events
        .iter()
        .filter_map(|event| {
            encode(event)
                .inspect_err(|error| tracing::error!(%error, "an app event did not serialise"))
                .ok()
        })
        .collect()
}

/// `event` as JSON, once.
pub(super) fn encode(event: &HostEvent) -> Result<EncodedEvent, Error> {
    let json = serde_json::to_vec(event).map_err(|error| crate::encode::encode_error(&error))?;
    EncodedEvent::from_json(&json)
}
