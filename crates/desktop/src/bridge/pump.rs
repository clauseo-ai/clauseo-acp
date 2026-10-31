//! The driver of each window's [`WindowState`] on Tauri's async runtime: one task per followed
//! stream pumps its host [`Subscription`] into the window's channel, a watchdog task ticks the
//! state's clock once a second, and acks may restart subscriptions.
//!
//! Frames are sent while the window's lock is held, so the order the state counts them in is
//! the order the webview receives them. Tasks hold the window weakly: dropping the window (a
//! new `open_stream`, or the bridge going away) aborts every task and so drops every host
//! subscription.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use clauseo_host::{HostHandle, Subscription};
use clauseo_protocol::{HostError, StreamId};
use futures::StreamExt as _;
use tauri::async_runtime::{self, JoinHandle};
use tauri::ipc::Channel;

use super::window::{AckError, Forward, Position, Restart, WindowState};
use super::{PRINCIPAL, lock};
use crate::frame::Frame;

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

    /// Sends `frames` in order. Stops at the first that fails: the webview is gone.
    fn send(&self, frames: Vec<Frame>) -> bool {
        frames
            .into_iter()
            .all(|frame| self.channel.send(frame).is_ok())
    }

    /// Whether the window is paused past the cap.
    #[cfg(test)]
    pub(crate) fn is_paused(&self) -> bool {
        self.state.is_paused()
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
    let generation = window.state.follow(stream, after);
    Restart {
        stream,
        generation,
        after,
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

/// Counts the window's ack and starts the subscriptions it restarts.
pub(crate) fn ack(
    window: &Arc<Mutex<Window>>,
    stream: StreamId,
    position: Position,
    frames: u32,
) -> Result<(), AckError> {
    let (host, restarts) = {
        let mut guard = lock(window);
        let elapsed = guard.opened.elapsed();
        // Restarts end a pause, whose pumps are already stopped.
        let restarts = guard.state.ack(stream, position, frames, elapsed)?;
        (guard.host.clone(), restarts)
    };
    spawn_restarts(&host, window, restarts);
    Ok(())
}

/// Starts each of `restarts` in its own task.
fn spawn_restarts(host: &HostHandle, window: &Arc<Mutex<Window>>, restarts: Vec<Restart>) {
    for restart in restarts {
        let host = host.clone();
        let window = Arc::clone(window);
        drop(async_runtime::spawn(async move {
            // A restart that fails leaves the stream followed with no subscription; the
            // webview's next subscribe starts it again.
            let _failed = start(&host, &window, restart).await;
        }));
    }
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
            if guard.state.is_current(restart.stream, restart.generation)
                && let Some(old) = guard.pumps.remove(&restart.stream)
            {
                old.abort();
            }
            return Err(error);
        }
    };
    let mut guard = lock(window);
    if guard.state.is_paused() || !guard.state.is_current(restart.stream, restart.generation) {
        return Ok(());
    }
    let pump = async_runtime::spawn(pump(
        Arc::downgrade(window),
        restart.stream,
        restart.generation,
        subscription,
    ));
    if let Some(old) = guard.pumps.insert(restart.stream, pump) {
        old.abort();
    }
    Ok(())
}

/// Forwards `subscription`'s events, generation `generation` of `stream`, until it ends, the
/// window is gone, or the subscription is no longer current.
async fn pump(
    window: Weak<Mutex<Window>>,
    stream: StreamId,
    generation: u64,
    mut subscription: Subscription,
) {
    while let Some(event) = subscription.next().await {
        let Some(window) = window.upgrade() else {
            return;
        };
        let mut window = lock(&window);
        let elapsed = window.opened.elapsed();
        match window
            .state
            .forward(stream, generation, event.json(), elapsed)
        {
            Forward::Send { frames, pause } => {
                if !window.send(frames) {
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

/// Ticks the window's clock every [`TICK`] and starts the restarts the watchdog asks for.
async fn watch(window: Weak<Mutex<Window>>) {
    let mut ticks = async_io::Timer::interval(TICK);
    while ticks.next().await.is_some() {
        let Some(window) = window.upgrade() else {
            return;
        };
        let (host, restarts) = {
            let mut guard = lock(&window);
            let elapsed = guard.opened.elapsed();
            // Each restart replaces its stream's pump once the new subscription starts.
            let restarts = guard.state.tick(elapsed);
            (guard.host.clone(), restarts)
        };
        spawn_restarts(&host, &window, restarts);
    }
}
