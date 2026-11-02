//! The app-wide feed (`subscribe_app`) on a window's channel: its own pump beside the streams'
//! (`pump.rs`). A restart the bridge makes (a pause's end, the watchdog) subscribes again, reads
//! `GetSettings` and sends one `settings_changed` before the new pump starts, so the window never
//! ends on a stale view though it missed events (0017's subscribe-then-read rule). H4's
//! `matters_changed` and `threads_changed` (not merged) go through as they come; when they land,
//! the restart also needs their current lists, unless the host makes a subscription's first frame
//! `all: true`. The host refuses `subscribe_app` only once it has stopped, so a failed restart is
//! not retried: the window stops following it.

use std::sync::{Arc, Mutex, Weak};

use clauseo_host::{AppSubscription, HostHandle};
use clauseo_protocol::{ErrorCode, HostError, HostEvent, HostQuery, QueryResult};
use futures::StreamExt as _;
use tauri::async_runtime;

use super::{Forward, Window};
use crate::bridge::{PRINCIPAL, lock};

/// Follows the app-wide feed. Returns the restart id of the subscription [`start_app`] starts;
/// a current one's events are dropped from now on.
pub(crate) fn follow_app(window: &Mutex<Window>) -> u64 {
    lock(window).state.follow_app()
}

/// Stops following the app-wide feed for `restart_id`, a subscription the host refused, unless
/// a newer one came since.
pub(crate) fn forget_app(window: &Mutex<Window>, restart_id: u64) {
    let mut window = lock(window);
    if window.state.unfollow_app_restart(restart_id)
        && let Some(pump) = window.app_pump.take()
    {
        pump.abort();
    }
}

/// Starts the app feed's restart `restart_id`, when there is one, in its own task, sending the
/// current state first. One the host refuses stops the window following the feed.
pub(super) fn spawn_app_restart(
    host: &HostHandle,
    window: &Arc<Mutex<Window>>,
    restart_id: Option<u64>,
) {
    let Some(restart_id) = restart_id else {
        return;
    };
    let host = host.clone();
    let window = Arc::clone(window);
    drop(async_runtime::spawn(async move {
        if start_app(&host, &window, restart_id, true).await.is_err() {
            forget_app(&window, restart_id);
        }
    }));
}

/// The current settings as the `settings_changed` event's text, read after the subscription it
/// goes before.
// TODO(H4): drop this read, and `start_app`'s `current`, once the host sends its own catch-up
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

/// Subscribes to the app-wide events for `restart_id` and starts its pump, unless the window
/// paused or a newer subscription came first while the host answered. With `current` (a
/// restart the bridge made) it then reads the settings and sends them, as one
/// `settings_changed`, before any event of the new subscription.
pub(crate) async fn start_app(
    host: &HostHandle,
    window: &Arc<Mutex<Window>>,
    restart_id: u64,
    current: bool,
) -> Result<(), HostError> {
    if lock(window).state.is_paused() {
        // Started when acks catch up.
        return Ok(());
    }
    let subscribed = host.subscribe_app(PRINCIPAL).await;
    // The host answers in order: a change after the read is in the subscription.
    let subscribed = match subscribed {
        Ok(subscription) if current => current_settings(host)
            .await
            .map(|settings| (subscription, Some(settings))),
        Ok(subscription) => Ok((subscription, None)),
        Err(error) => Err(error),
    };
    let (subscription, settings) = match subscribed {
        Ok(started) => started,
        Err(error) => {
            let mut guard = lock(window);
            if guard.state.is_current_app(restart_id)
                && let Some(old) = guard.app_pump.take()
            {
                old.abort();
            }
            return Err(error);
        }
    };
    let mut guard = lock(window);
    if guard.state.is_paused() || !guard.state.is_current_app(restart_id) {
        return Ok(());
    }
    if let Some(settings) = settings {
        let elapsed = guard.opened.elapsed();
        if let Forward::Send { messages, pause } =
            guard.state.forward_app(restart_id, &settings, elapsed)
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
    {
        guard.app_dropped = Some(dropped);
    }
    #[cfg(not(test))]
    let probe = ();
    let pump = async_runtime::spawn(pump_app(
        Arc::downgrade(window),
        restart_id,
        subscription,
        probe,
    ));
    if let Some(old) = guard.app_pump.replace(pump) {
        old.abort();
    }
    Ok(())
}

/// Forwards the app subscription's events, restart `restart_id`, until it ends, the window is
/// gone, or the subscription is no longer current.
async fn pump_app(
    window: Weak<Mutex<Window>>,
    restart_id: u64,
    mut subscription: AppSubscription,
    // Held by the future and dropped with it: an abort hangs it up.
    _probe: Probe,
) {
    while let Some(event) = subscription.next().await {
        let Some(window) = window.upgrade() else {
            return;
        };
        let mut window = lock(&window);
        let elapsed = window.opened.elapsed();
        match window.state.forward_app(restart_id, event.json(), elapsed) {
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

/// What an app pump holds for tests to see it dropped: a sender, in test builds only.
#[cfg(test)]
type Probe = std::sync::mpsc::Sender<()>;
#[cfg(not(test))]
type Probe = ();

/// For tests: the hang-up of the window's current app pump (see `Window::app_dropped`).
#[cfg(test)]
pub(crate) fn app_dropped(window: &Mutex<Window>) -> Option<std::sync::mpsc::Receiver<()>> {
    lock(window).app_dropped.take()
}

/// For tests: counts `json` as an app event of the feed's current subscription, as its pump
/// would but without sending it, and drops every pump when that paused the window.
#[cfg(test)]
pub(crate) fn count_app_event(window: &Mutex<Window>, json: &str) -> Forward {
    let mut window = lock(window);
    let Some(restart_id) = window.state.app_restart_id() else {
        return Forward::Drop;
    };
    let elapsed = window.opened.elapsed();
    let forwarded = window.state.forward_app(restart_id, json, elapsed);
    if matches!(forwarded, Forward::Send { pause: true, .. }) {
        window.stop_pumps();
    }
    forwarded
}
