//! The driver of each window's [`WindowState`] on Tauri's async runtime: one task per followed
//! stream pumps its host [`Subscription`] into the window's channel, a watchdog task ticks the
//! state's clock once a second, and acks may restart subscriptions. A restart the host refuses
//! sends the webview a `stream_failed` message, and the clock tries the stream again.
//!
//! Channel messages are sent while the window's lock is held, so the order the state counts them
//! in is the order the webview receives them. Tasks hold the window weakly: dropping the window (a
//! new `open_stream`, or the bridge going away) aborts every task and so drops every host
//! subscription.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use clauseo_host::{HostHandle, Subscription};
use clauseo_protocol::{Ack, HostError, StreamId};
use futures::StreamExt as _;
use tauri::async_runtime::{self, JoinHandle};
use tauri::ipc::Channel;

use super::window::{AckError, Forward, Position, Restart, WindowState};
use super::{PRINCIPAL, lock};
use crate::frame::{self, Frame};

/// How often the watchdog reads the clock.
const TICK: Duration = Duration::from_secs(1);

/// One window's channel, stream state and tasks.
pub(crate) struct Window {
    host: HostHandle,
    /// When the window opened: its state's time 0.
    opened: Instant,
    channel: Channel<Frame>,
    state: WindowState,
    /// The pump of each stream whose host subscription is live.
    pumps: HashMap<StreamId, JoinHandle<()>>,
    watchdog: Option<JoinHandle<()>>,
}

impl Drop for Window {
    fn drop(&mut self) {
        for (_, pump) in self.pumps.drain() {
            pump.abort();
        }
        if let Some(watchdog) = self.watchdog.take() {
            watchdog.abort();
        }
    }
}

/// The time now: the bridge's one clock read. The state machine takes time as input.
#[expect(
    clippy::disallowed_methods,
    reason = "the bridge driver's one clock read; window.rs takes time as an argument"
)]
fn now() -> Instant {
    Instant::now()
}

impl Window {
    /// A window sending on `channel` what `host` publishes, its watchdog running.
    pub(crate) fn open(host: HostHandle, channel: Channel<Frame>) -> Arc<Mutex<Window>> {
        let window = Arc::new(Mutex::new(Window {
            host,
            opened: now(),
            channel,
            state: WindowState::new(),
            pumps: HashMap::new(),
            watchdog: None,
        }));
        let watchdog = async_runtime::spawn(watch(Arc::downgrade(&window)));
        lock(&window).watchdog = Some(watchdog);
        window
    }

    /// Sends `messages` in order. Stops at the first that fails: the webview is gone. The e2e
    /// build adds each message's release length (`Frame::release_len`: a `timed` message
    /// without its wrapper) to `perf_channel_bytes`'s total once it is sent. Flow control
    /// counts the wrapped length, in `WindowState`.
    fn send(&self, messages: Vec<Frame>) -> bool {
        messages.into_iter().all(|message| {
            #[cfg(feature = "e2e")]
            let len = message.release_len();
            let sent = self.channel.send(message).is_ok();
            #[cfg(feature = "e2e")]
            if sent {
                crate::e2e::count_channel_bytes(len);
            }
            sent
        })
    }

    /// The window's stream state.
    #[cfg(test)]
    pub(crate) fn state(&self) -> &WindowState {
        &self.state
    }

    /// Drops every host subscription by aborting its pump.
    fn stop_pumps(&mut self) {
        for (_, pump) in self.pumps.drain() {
            pump.abort();
        }
    }
}

/// Follows `stream` from `after`. Returns the restart that starts the new subscription; the
/// current one stays until [`start`] replaces it, so the host keeps the stream loaded and can
/// answer with the tail rather than a `Reset`. Its events are dropped from now on.
pub(crate) fn follow(window: &Mutex<Window>, stream: StreamId, after: Option<Position>) -> Restart {
    let mut window = lock(window);
    let restart_id = window.state.follow(stream, after);
    Restart {
        stream,
        id: restart_id,
        after,
    }
}

/// Stops following the stream of `restart`, a `subscribe` the host refused, unless a newer
/// subscription of it came since: the webview has the error, and nothing retries it.
pub(crate) fn forget(window: &Mutex<Window>, restart: Restart) {
    let mut window = lock(window);
    if window.state.unfollow_restart(restart.stream, restart.id)
        && let Some(pump) = window.pumps.remove(&restart.stream)
    {
        pump.abort();
    }
}

/// Stops following `stream`.
pub(crate) fn unfollow(window: &Mutex<Window>, stream: StreamId) {
    let mut window = lock(window);
    window.state.unfollow(stream);
    if let Some(pump) = window.pumps.remove(&stream) {
        pump.abort();
    }
}

/// Counts the window's ack and starts the subscriptions it restarts. Returns the positions of
/// followed streams, for the host.
pub(crate) fn ack(
    window: &Arc<Mutex<Window>>,
    positions: &[Ack],
    messages: u32,
) -> Result<Vec<Ack>, AckError> {
    let (host, acked) = {
        let mut guard = lock(window);
        let elapsed = guard.opened.elapsed();
        // Restarts end a pause, whose pumps are already stopped.
        let acked = guard.state.ack(positions, messages, elapsed)?;
        (guard.host.clone(), acked)
    };
    spawn_restarts(&host, window, acked.restarts);
    Ok(acked.positions)
}

/// Starts each of `restarts` in its own task. One the host refuses stays followed with no
/// subscription: the webview gets a `stream_failed` message, and the clock tries it again.
pub(crate) fn spawn_restarts(
    host: &HostHandle,
    window: &Arc<Mutex<Window>>,
    restarts: Vec<Restart>,
) {
    for restart in restarts {
        let host = host.clone();
        let window = Arc::clone(window);
        drop(async_runtime::spawn(async move {
            if let Err(error) = start(&host, &window, restart).await {
                failed(&window, restart, &error);
            }
        }));
    }
}

/// The host refused `restart` with `error`: when it is still its stream's current subscription,
/// schedules the next try and tells the webview.
fn failed(window: &Mutex<Window>, restart: Restart, error: &HostError) {
    let mut window = lock(window);
    let elapsed = window.opened.elapsed();
    if !window.state.failed(restart.stream, restart.id, elapsed) {
        return;
    }
    let message = frame::stream_failed(restart.stream, error);
    window.state.count_notice(&message, elapsed);
    // A send that fails means the webview is gone; its window is about to be dropped.
    let _sent = window.send(vec![message]);
}

/// Subscribes to the host for `restart` and starts its pump, unless the window paused or a
/// newer subscription of the stream came first while the host answered.
pub(crate) async fn start(
    host: &HostHandle,
    window: &Arc<Mutex<Window>>,
    restart: Restart,
) -> Result<(), HostError> {
    if lock(window).state.is_paused() {
        // Started when acks catch up.
        return Ok(());
    }
    let subscription = match host
        .subscribe(PRINCIPAL, restart.stream, restart.after)
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => {
            let mut guard = lock(window);
            if guard.state.is_current(restart.stream, restart.id)
                && let Some(old) = guard.pumps.remove(&restart.stream)
            {
                old.abort();
            }
            return Err(error);
        }
    };
    let mut guard = lock(window);
    if guard.state.is_paused() || !guard.state.is_current(restart.stream, restart.id) {
        return Ok(());
    }
    guard.state.started(restart.stream, restart.id);
    let pump = async_runtime::spawn(pump(
        Arc::downgrade(window),
        restart.stream,
        restart.id,
        subscription,
    ));
    if let Some(old) = guard.pumps.insert(restart.stream, pump) {
        old.abort();
    }
    Ok(())
}

/// Forwards `subscription`'s events, restart `restart_id` of `stream`, until it ends, the
/// window is gone, or the subscription is no longer current.
async fn pump(
    window: Weak<Mutex<Window>>,
    stream: StreamId,
    restart_id: u64,
    mut subscription: Subscription,
) {
    while let Some(event) = subscription.next().await {
        // The e2e build's receive time: when the event left the host subscription.
        #[cfg(feature = "e2e")]
        let received_ms = crate::e2e::epoch_ms();
        let Some(window) = window.upgrade() else {
            return;
        };
        let mut window = lock(&window);
        let elapsed = window.opened.elapsed();
        #[cfg(not(feature = "e2e"))]
        let forwarded = window
            .state
            .forward(stream, restart_id, event.json(), elapsed);
        #[cfg(feature = "e2e")]
        let forwarded =
            window
                .state
                .forward_timed(stream, restart_id, event.json(), received_ms, elapsed);
        match forwarded {
            Forward::Send { messages, pause } => {
                if !window.send(messages) {
                    return;
                }
                if pause {
                    window.stop_pumps();
                    return;
                }
            }
            // Dropped: paused (this pump is about to be aborted) or replaced (likewise, once
            // the new subscription starts). Too large (over 200 MB of JSON): never sent; the
            // webview's next resubscribe gets a `Reset`.
            Forward::Drop | Forward::TooLarge => {}
        }
    }
}

/// Ticks the window's clock every [`TICK`] and starts the restarts it asks for: the watchdog's,
/// and the retries of streams whose restart failed.
async fn watch(window: Weak<Mutex<Window>>) {
    let mut ticks = async_io::Timer::interval(TICK);
    while ticks.next().await.is_some() {
        let Some(window) = window.upgrade() else {
            return;
        };
        let (host, restarts) = {
            let mut guard = lock(&window);
            let elapsed = guard.opened.elapsed();
            // Each restart replaces its stream's pump, if any, once the new subscription starts.
            let restarts = guard.state.tick(elapsed);
            (guard.host.clone(), restarts)
        };
        spawn_restarts(&host, &window, restarts);
    }
}
