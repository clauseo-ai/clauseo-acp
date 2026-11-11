//! Whether `main` is in the OS's own full screen, as `Window::is_fullscreen` last said, and the
//! windows' app feeds that follow it (`frame::window_full_screen_text`).
//!
//! The page cannot tell on its own: on a notched Mac, macOS puts a full-screen window below the
//! camera band, so the window is shorter than its screen and a size read says "windowed". The
//! value is published only when it changes, and a new follower (a `subscribe_app`, or the app
//! feed restarted after a pause or the watchdog) gets the last value first. Setting and
//! following take one lock, so no change falls between the two.

use std::sync::{Arc, Mutex};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::bridge::lock;

/// The last full-screen value of `main`, and the feeds that follow it.
#[derive(Default)]
pub(crate) struct FullScreen {
    state: Mutex<Latest>,
}

/// The last value, and the followers.
#[derive(Default)]
struct Latest {
    /// `None` until the first read.
    value: Option<bool>,
    /// Each follower's channel; one whose receiver is gone is dropped at the next change.
    followers: Vec<UnboundedSender<Arc<str>>>,
}

impl FullScreen {
    /// Keeps `full` as the value and, when it differs from the last one, sends it to every
    /// follower as a `window_full_screen` message. Returns whether it changed.
    pub(crate) fn set(&self, full: bool) -> bool {
        let mut latest = lock(&self.state);
        if latest.value == Some(full) {
            return false;
        }
        latest.value = Some(full);
        let text: Arc<str> = crate::frame::window_full_screen_text(full).into();
        latest
            .followers
            .retain(|follower| follower.unbounded_send(Arc::clone(&text)).is_ok());
        true
    }

    /// A new follower: the last value, when there is one, then every change, as channel
    /// message texts.
    pub(crate) fn follow(&self) -> UnboundedReceiver<Arc<str>> {
        let (follower, feed) = unbounded();
        let mut latest = lock(&self.state);
        if let Some(full) = latest.value {
            // The receiver is in hand: an unbounded send to it cannot fail.
            let _sent = follower.unbounded_send(crate::frame::window_full_screen_text(full).into());
        }
        latest.followers.retain(|follower| !follower.is_closed());
        latest.followers.push(follower);
        feed
    }
}
