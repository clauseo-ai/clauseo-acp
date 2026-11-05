//! The feeds of no stream on a window's channel ([`Feed`]), each with its own pump beside the
//! streams' (`pump.rs`).
//!
//! - The app-wide feed (`subscribe_app`), which the window asks for. A restart the bridge makes
//!   (a pause's end, the watchdog) subscribes again, reads `GetSettings` and sends one
//!   `settings_changed` before the new pump starts, so the window never ends on a stale view
//!   though it missed events (0017's subscribe-then-read rule). H4's `matters_changed` and
//!   `threads_changed` (not merged) go through as they come; when they land, the restart also
//!   needs their current lists, unless the host makes a subscription's first frame `all: true`.
//!   The settings window gets `settings_changed` alone of it, and any other label none
//!   ([`AppEvents`], 0028).
//! - The limits (`HostHandle::subscribe_limits`), which `open_stream` follows for `main` and
//!   the settings window (U12). The host starts
//!   every limits subscription with the current `limits_view_changed`, so a restart needs no
//!   read and nothing is replayed. After a pause or a watchdog renewal the window so gets only
//!   the current view, never the `account_limits_changed` it missed: the view must carry
//!   everything the window shows of the limits.
//!
//! The host refuses either only once it has stopped, so a failed restart is not retried: the
//! window stops following the feed.

use std::sync::{Arc, Mutex, Weak};

use clauseo_host::{EncodedEvent, HostHandle};
use clauseo_protocol::{ErrorCode, HostError, HostEvent, HostQuery, QueryResult};
use futures::StreamExt as _;
use futures::stream::BoxStream;
use tauri::async_runtime;

use super::{Feed, Forward, Window};
use crate::bridge::{PRINCIPAL, lock};

/// Which feed events a window's channel carries: an allow-list keyed on its label (0028).
/// `main` carries them all. The settings window carries the limits (U12) and, of the app feed,
/// `settings_changed` alone, so it never gets the matter and thread lists (0023) or an app
/// event the host adds later. Any other label carries neither, whatever it follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppEvents {
    /// Every app and limits event: `main`.
    All,
    /// The app feed's `settings_changed` only, and the limits: the settings window.
    SettingsAndLimits,
    /// No app or limits event: any other label.
    Nothing,
}

impl AppEvents {
    /// The app events the window `label` carries.
    pub(crate) fn of(label: &str) -> Self {
        match label {
            crate::MAIN_WINDOW => Self::All,
            crate::settings_window::SETTINGS_WINDOW => Self::SettingsAndLimits,
            _ => Self::Nothing,
        }
    }

    /// Whether an event of `feed`, as the host encoded it, goes on the channel. Checked before
    /// it is counted, so an ack counts only what the window received. The host writes each
    /// event as its one key first (`{"settings_changed":...}`), so the key's prefix names it.
    pub(in crate::bridge) fn carries(self, feed: Feed, json: &str) -> bool {
        match (feed, self) {
            (_, Self::All) | (Feed::Limits, Self::SettingsAndLimits) => true,
            // `update_changed` (0037) joins here in R23 PR 5, with a feed restart that reads
            // `GetUpdate` as it reads `GetSettings`; until then only main carries it.
            (Feed::App, Self::SettingsAndLimits) => json.starts_with(r#"{"settings_changed":"#),
            (_, Self::Nothing) => false,
        }
    }
}

/// Follows `feed`. Returns the restart id of the subscription [`start_feed`] starts; a current
/// one's events are dropped from now on.
pub(crate) fn follow_feed(window: &Mutex<Window>, feed: Feed) -> u64 {
    lock(window).state.follow_feed(feed)
}

/// Stops following `feed` for `restart_id`, a subscription the host refused, unless a newer one
/// came since.
pub(crate) fn forget_feed(window: &Mutex<Window>, feed: Feed, restart_id: u64) {
    let mut window = lock(window);
    if window.state.feeds_mut().unfollow_restart(feed, restart_id)
        && let Some(pump) = window.feed_pumps.remove(&feed)
    {
        pump.abort();
    }
}

/// Follows the limits on the window's channel and starts their subscription in a task of its
/// own, which sends the current view first. Refused (the host has stopped), the window stops
/// following them.
pub(crate) fn follow_limits(host: &HostHandle, window: &Arc<Mutex<Window>>) {
    let restart_id = follow_feed(window, Feed::Limits);
    spawn_feed_restarts(host, window, vec![(Feed::Limits, restart_id)]);
}

/// Starts each of `restarts` in its own task, the current state first. One the host refuses
/// stops the window following that feed.
pub(super) fn spawn_feed_restarts(
    host: &HostHandle,
    window: &Arc<Mutex<Window>>,
    restarts: Vec<(Feed, u64)>,
) {
    for (feed, restart_id) in restarts {
        let host = host.clone();
        let window = Arc::clone(window);
        drop(async_runtime::spawn(async move {
            if start_feed(&host, &window, feed, restart_id, true)
                .await
                .is_err()
            {
                forget_feed(&window, feed, restart_id);
            }
        }));
    }
}

/// The current settings as the `settings_changed` event's text, read after the subscription it
/// goes before.
// TODO(H4): drop this read, and `start_feed`'s `current`, once the host sends its own catch-up
// frames first after `subscribe_app` (the `all: true` lists and a first `settings_changed`).
async fn current_settings(host: &HostHandle) -> Result<String, HostError> {
    let QueryResult::Settings(settings) = host.query(PRINCIPAL, HostQuery::GetSettings).await?
    else {
        return Err(HostError::new(
            ErrorCode::Internal,
            "GetSettings answered with another kind of result",
        ));
    };
    serde_json::to_string(&HostEvent::SettingsChanged(settings))
        .map_err(|error| HostError::new(ErrorCode::Internal, error.to_string()))
}

/// A host subscription to `feed`, and the text to send before its events: for the app feed
/// with `current`, the settings, read after subscribing.
async fn subscribe(
    host: &HostHandle,
    feed: Feed,
    current: bool,
) -> Result<(BoxStream<'static, EncodedEvent>, Option<String>), HostError> {
    match feed {
        Feed::App => {
            let subscription = host.subscribe_app(PRINCIPAL).await?;
            // The host answers in order: a change after the read is in the subscription.
            let settings = if current {
                Some(current_settings(host).await?)
            } else {
                None
            };
            Ok((subscription.boxed(), settings))
        }
        // Its first event is the current view.
        Feed::Limits => Ok((host.subscribe_limits(PRINCIPAL).await?.boxed(), None)),
    }
}

/// Subscribes to `feed` for `restart_id` and starts its pump, unless the window paused or a
/// newer subscription came first while the host answered. With `current` (a restart the bridge
/// made) the app feed then reads the settings and sends them, as one `settings_changed`, before
/// any event of the new subscription.
pub(crate) async fn start_feed(
    host: &HostHandle,
    window: &Arc<Mutex<Window>>,
    feed: Feed,
    restart_id: u64,
    current: bool,
) -> Result<(), HostError> {
    if lock(window).state.is_paused() {
        // Started when acks catch up.
        return Ok(());
    }
    let (subscription, first) = match subscribe(host, feed, current).await {
        Ok(started) => started,
        Err(error) => {
            let mut guard = lock(window);
            if guard.state.feeds().is_current(feed, restart_id)
                && let Some(old) = guard.feed_pumps.remove(&feed)
            {
                old.abort();
            }
            return Err(error);
        }
    };
    let mut guard = lock(window);
    if guard.state.is_paused() || !guard.state.feeds().is_current(feed, restart_id) {
        return Ok(());
    }
    if let Some(first) = first.filter(|first| guard.app_events.carries(feed, first)) {
        let elapsed = guard.opened.elapsed();
        if let Forward::Send { messages, pause } =
            guard.state.forward_feed(feed, restart_id, &first, elapsed)
        {
            if !guard.send(messages) {
                return Ok(());
            }
            if pause {
                // The pause's end restarts the feed again, with the state then.
                guard.stop_pumps();
                return Ok(());
            }
        }
    }
    #[cfg(test)]
    let (probe, dropped) = std::sync::mpsc::channel();
    #[cfg(test)]
    guard.feed_dropped.insert(feed, dropped);
    #[cfg(not(test))]
    let probe = ();
    let pump = async_runtime::spawn(pump_feed(
        Arc::downgrade(window),
        feed,
        restart_id,
        subscription,
        probe,
    ));
    if let Some(old) = guard.feed_pumps.insert(feed, pump) {
        old.abort();
    }
    Ok(())
}

/// Forwards `feed`'s subscription events, restart `restart_id`, until it ends, the window is
/// gone, or the subscription is no longer current.
async fn pump_feed(
    window: Weak<Mutex<Window>>,
    feed: Feed,
    restart_id: u64,
    mut subscription: BoxStream<'static, EncodedEvent>,
    // Held by the future and dropped with it: an abort hangs it up.
    _probe: Probe,
) {
    while let Some(event) = subscription.next().await {
        let Some(window) = window.upgrade() else {
            return;
        };
        let mut window = lock(&window);
        if !window.app_events.carries(feed, event.json()) {
            continue;
        }
        let elapsed = window.opened.elapsed();
        match window
            .state
            .forward_feed(feed, restart_id, event.json(), elapsed)
        {
            Forward::Send { messages, pause } => {
                if !window.send(messages) {
                    return;
                }
                if pause {
                    window.stop_pumps();
                    return;
                }
            }
            // Dropped: paused or replaced, and this pump is about to be aborted. Too large:
            // never sent; the next restart sends the current state.
            Forward::Drop | Forward::TooLarge => {}
        }
    }
}

/// What a feed's pump holds for tests to see it dropped: a sender, in test builds only.
#[cfg(test)]
type Probe = std::sync::mpsc::Sender<()>;
#[cfg(not(test))]
type Probe = ();

/// For tests: the hang-up of the window's current pump of `feed` (see `Window::feed_dropped`).
#[cfg(test)]
pub(crate) fn feed_dropped(
    window: &Mutex<Window>,
    feed: Feed,
) -> Option<std::sync::mpsc::Receiver<()>> {
    lock(window).feed_dropped.remove(&feed)
}

/// For tests: counts `json` as an event of `feed`'s current subscription, as its pump would but
/// without sending it, and drops every pump when that paused the window.
#[cfg(test)]
pub(crate) fn count_feed_event(window: &Mutex<Window>, feed: Feed, json: &str) -> Forward {
    let mut window = lock(window);
    let Some(restart_id) = window.state.feeds().restart_id(feed) else {
        return Forward::Drop;
    };
    let elapsed = window.opened.elapsed();
    let forwarded = window.state.forward_feed(feed, restart_id, json, elapsed);
    if matches!(forwarded, Forward::Send { pause: true, .. }) {
        window.stop_pumps();
    }
    forwarded
}
