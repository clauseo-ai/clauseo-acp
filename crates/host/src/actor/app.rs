//! App subscriptions: the app-wide `HostEvent`s of no stream, sent to every app subscriber and
//! to no stream's subscribers. `HostHandle::subscribe_app`'s doc and `AppSubscription`'s give
//! the rules a caller sees; this is how the actor keeps them.
//!
//! The rules follow `subscribe`: the principal is checked first (`Access::SubscribeApp`); each
//! subscriber has its own `bounded(1024)` channel filled with `try_send`, so one slow window
//! never holds up the actor or another window; a full subscriber is marked lagged and gets
//! nothing until it has drained and asked (`Input::AppDrained`), then gets the current state
//! ([`catch_up`]), as a view subscriber gets a `Reset`; a closed channel (the subscription was
//! dropped) removes the subscriber at the next send, and `Input::AppUnsubscribed` at once. The
//! actor owns every sink, so when the host stops and the actor is dropped every app stream ends.
//!
//! Each event is serialised once ([`publish`]) and the same `EncodedEvent` goes to every
//! subscriber. What publishes here: `SettingsChanged` after each successful settings write
//! (`settings.rs`). Still to come, each through [`publish`] and each adding its part of the
//! current state to [`catch_up`]: the matter list and thread summaries (H4), notices, and
//! `limits`' `AccountLimitsChanged` (R16). H4's shape sends summaries of the threads *a window*
//! is not subscribed to, but `subscribe_app` carries no window identity, so the host sends every
//! changed summary to every app subscriber and each window drops those of threads it has open.
//!
//! For H4: today's catch-up is one whole `SettingsChanged`, which replaces what a window holds.
//! Once `matters_changed` sends partial rows (only the matters that changed), a lagged
//! subscriber has missed rows it cannot rebuild from the next delta, so its catch-up needs an
//! event that replaces the whole list (a reset or caught-up variant) before any partial row is
//! sent on this stream.

use clauseo_protocol::{HostEvent, Principal};

use super::Actor;
use super::input::{Reply, SubId};
use crate::encode::EncodedEvent;
use crate::error::Error;
use crate::handle::{AppSubscription, Sink};
use crate::principal::{Access, authorize};

/// One app subscriber.
pub(crate) struct AppSub {
    id: SubId,
    sink: Sink<EncodedEvent>,
    /// Its channel was full: it gets nothing until it drains and is caught up.
    lagged: bool,
}

impl AppSub {
    /// Offers one event; returns whether the subscriber stays.
    fn offer(&mut self, event: &EncodedEvent) -> bool {
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

/// `subscribe_app`: the principal check, then a subscriber that gets every later app event.
pub(super) fn subscribe(actor: &mut Actor, principal: &Principal, reply: Reply<AppSubscription>) {
    if let Err(error) = authorize(principal, Access::SubscribeApp) {
        let _gone = reply.send(Err(error));
        return;
    }
    let id = super::subs::next_sub_id(actor);
    let (subscription, sink) = AppSubscription::new(actor.post.clone(), id);
    if reply.send(Ok(subscription)).is_ok() {
        actor.app_subs.push(AppSub {
            id,
            sink,
            lagged: false,
        });
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

/// A lagged subscriber drained its channel: send it the current state.
pub(super) fn on_drained(actor: &mut Actor, id: SubId) {
    let events = catch_up(actor);
    let Some(index) = actor.app_subs.iter().position(|sub| sub.id == id) else {
        return;
    };
    let mut stays = true;
    if let Some(sub) = actor.app_subs.get_mut(index) {
        sub.lagged = false;
        for event in &events {
            stays = sub.offer(event);
            if !stays {
                break;
            }
        }
    }
    if !stays {
        actor.app_subs.remove(index);
    }
}

/// An app subscription was dropped.
pub(super) fn on_unsubscribed(actor: &mut Actor, id: SubId) {
    actor.app_subs.retain(|sub| sub.id != id);
}

/// The current state, as the events that bring a lagged subscriber up to date. Each event here
/// must replace what a window holds whole; see the module doc before adding a partial one.
fn catch_up(actor: &Actor) -> Vec<EncodedEvent> {
    let settings = HostEvent::SettingsChanged(super::settings::view(&actor.effective));
    match encode(&settings) {
        Ok(encoded) => vec![encoded],
        Err(error) => {
            tracing::error!(%error, "an app event did not serialise");
            Vec::new()
        }
    }
}

/// `event` as JSON, once.
fn encode(event: &HostEvent) -> Result<EncodedEvent, Error> {
    let json = serde_json::to_vec(event).map_err(|error| crate::encode::encode_error(&error))?;
    EncodedEvent::from_json(&json)
}
