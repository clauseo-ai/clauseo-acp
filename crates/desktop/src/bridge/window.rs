//! One window's stream state, with no I/O: which streams it follows, what it has sent, what the
//! webview has acknowledged, and when to drop and restart its host subscriptions. The driver
//! (`pump.rs`) feeds it events, acks and clock ticks and carries out what it returns. Time comes
//! in as an argument, the time since the window opened, so tests run it with no clock or sleep.
//!
//! Backpressure counts bytes by channel message: each message sent appends the running total of
//! bytes sent, and an ack that says the webview has processed `messages` messages in all reads
//! the total after the `messages`th. The bridge never parses an event to count it.
//!
//! - Past [`UNACKED_CAP`] unacknowledged bytes the window pauses: every host subscription is
//!   dropped and nothing more is forwarded. Once acks bring the count back to 0, each stream
//!   restarts after its last acknowledged `(epoch, cursor)`, or from a `Reset` when it has none.
//! - [`WATCHDOG`] with bytes unacknowledged and no ack progress restarts every stream the same
//!   way at once, paused or not, so a lost ack cannot stall the window. It fires once per stall:
//!   it fires again only once acks cover every message sent before it fired. A page that stops
//!   acking so gets at most one event more after the fire, and its unacknowledged bytes stay at
//!   most the larger of the cap and what was unacknowledged at the fire, plus that one event.
//! - A restart whose host subscription failed is tried again on the clock, [`RETRY_FIRST`]
//!   after the failure, the wait doubling with each failure up to [`RETRY_MAX`].
//!
//! A restart gives each stream a new restart id; events from an older restart's subscription
//! are dropped, so a stream never mixes two subscriptions.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use clauseo_protocol::{Ack, Cursor, StreamId, ViewEpoch};

use crate::frame::{self, Frame};

/// The most bytes a window may have unacknowledged before it pauses.
pub(crate) const UNACKED_CAP: u64 = 256 * 1024;

/// How long bytes may stay unacknowledged with no ack progress before every stream restarts.
pub(crate) const WATCHDOG: Duration = Duration::from_secs(5);

/// How long after a failed restart the stream is tried again.
pub(crate) const RETRY_FIRST: Duration = Duration::from_secs(5);

/// The longest wait between tries of a stream whose restarts keep failing.
pub(crate) const RETRY_MAX: Duration = Duration::from_secs(60);

/// A position in a stream's view: the epoch and the last cursor applied.
pub(crate) type Position = (ViewEpoch, Cursor);

/// A host subscription to (re)start: the stream, its new restart id and where to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Restart {
    /// The stream.
    pub(crate) stream: StreamId,
    /// The restart id the new subscription's events carry. Not the host's epoch: the bridge
    /// counts it up for every subscription it starts.
    pub(crate) id: u64,
    /// The last acknowledged position, `None` for a `Reset`.
    pub(crate) after: Option<Position>,
}

/// What forwarding one event comes to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Forward {
    /// Send these channel messages, in order. `pause` says the window went past the cap with
    /// them: drop every host subscription after sending.
    Send {
        /// The messages.
        messages: Vec<Frame>,
        /// Whether the window is now paused.
        pause: bool,
    },
    /// The event is dropped: the window is paused, or the subscription is not current.
    Drop,
    /// The event cannot be framed (more than `u16::MAX` fragments).
    TooLarge,
}

/// Why an ack was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AckError {
    /// It claims more messages than the window was sent.
    TooManyMessages,
    /// It names this stream twice.
    RepeatedStream(StreamId),
}

/// What an ack comes to.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Acked {
    /// The subscriptions to restart: every stream's, when the ack ended a pause.
    pub(crate) restarts: Vec<Restart>,
    /// The positions of streams the window follows, for the host. A position of a stream the
    /// window does not follow (an ack that crossed an `unsubscribe`) is left out.
    pub(crate) positions: Vec<Ack>,
}

/// When a stream whose restart failed is tried again.
#[derive(Debug, Clone, Copy)]
struct Retry {
    /// The time of the next try.
    at: Duration,
    /// The wait that set `at`; the next failure doubles it.
    wait: Duration,
}

/// One followed stream.
#[derive(Debug, Clone, Copy)]
struct Followed {
    /// The restart id of its current subscription.
    restart_id: u64,
    /// The last position the webview acknowledged, or where it subscribed from.
    acked: Option<Position>,
    /// Set while its last restart failed and no later one has started.
    retry: Option<Retry>,
}

/// A window's stream state.
#[derive(Debug)]
pub(crate) struct WindowState {
    /// Messages sent since `open_stream`.
    sent: u64,
    /// Messages the webview has said it processed.
    acked: u64,
    /// Bytes sent since `open_stream`.
    sent_bytes: u64,
    /// Bytes of the messages acknowledged.
    acked_bytes: u64,
    /// For each sent, unacknowledged message, oldest first, `sent_bytes` just after it: an ack
    /// of `k` more messages reads the `k`th.
    in_flight: VecDeque<u64>,
    /// The id of the next fragmented event.
    next_fragment: u32,
    /// The last restart id handed out.
    last_restart_id: u64,
    /// Past the cap, waiting for acks to catch up.
    paused: bool,
    /// When acks last made progress (or the watchdog last fired).
    progress_at: Duration,
    /// The messages acks must cover before the watchdog may fire again: `sent` when it last
    /// fired.
    rearm_at: u64,
    /// The streams the window follows.
    streams: BTreeMap<StreamId, Followed>,
}

impl WindowState {
    /// A window that has sent nothing, opened at time 0.
    pub(crate) fn new() -> Self {
        Self {
            sent: 0,
            acked: 0,
            sent_bytes: 0,
            acked_bytes: 0,
            in_flight: VecDeque::new(),
            next_fragment: 0,
            last_restart_id: 0,
            paused: false,
            progress_at: Duration::ZERO,
            rearm_at: 0,
            streams: BTreeMap::new(),
        }
    }

    /// Follows `stream` from `after` (or restarts it there). Returns the restart id its
    /// subscription's events must carry. While the window is paused the subscription is not
    /// started: the caller starts it only when [`is_paused`](Self::is_paused) is false.
    pub(crate) fn follow(&mut self, stream: StreamId, after: Option<Position>) -> u64 {
        self.last_restart_id += 1;
        let restart_id = self.last_restart_id;
        self.streams.insert(
            stream,
            Followed {
                restart_id,
                acked: after,
                retry: None,
            },
        );
        restart_id
    }

    /// Stops following `stream`. Returns whether it was followed.
    pub(crate) fn unfollow(&mut self, stream: StreamId) -> bool {
        self.streams.remove(&stream).is_some()
    }

    /// Stops following `stream` when `restart_id` is still its current subscription: a
    /// `subscribe` whose host subscription failed. Returns whether it did.
    pub(crate) fn unfollow_restart(&mut self, stream: StreamId, restart_id: u64) -> bool {
        self.is_current(stream, restart_id) && self.unfollow(stream)
    }

    /// Whether `restart_id` is `stream`'s current subscription.
    pub(crate) fn is_current(&self, stream: StreamId, restart_id: u64) -> bool {
        self.streams
            .get(&stream)
            .is_some_and(|followed| followed.restart_id == restart_id)
    }

    /// Whether the window is past the cap, waiting for acks.
    pub(crate) fn is_paused(&self) -> bool {
        self.paused
    }

    /// The bytes sent and not yet acknowledged.
    pub(crate) fn unacked_bytes(&self) -> u64 {
        self.sent_bytes - self.acked_bytes
    }

    /// Whether the window follows `stream`.
    #[cfg(test)]
    pub(crate) fn follows(&self, stream: StreamId) -> bool {
        self.streams.contains_key(&stream)
    }

    /// When `stream`, whose restart failed, is tried again; `None` when it is not waiting.
    #[cfg(test)]
    pub(crate) fn retry_at(&self, stream: StreamId) -> Option<Duration> {
        self.streams
            .get(&stream)
            .and_then(|followed| followed.retry)
            .map(|retry| retry.at)
    }

    /// Frames `json`, an event of `stream`'s subscription `restart_id`, and counts what it
    /// sends. The first bytes sent with nothing unacknowledged start the watchdog's wait at
    /// `now`. The e2e build's pump calls [`forward_timed`](Self::forward_timed) instead.
    #[cfg(any(test, not(feature = "e2e")))]
    pub(crate) fn forward(
        &mut self,
        stream: StreamId,
        restart_id: u64,
        json: &str,
        now: Duration,
    ) -> Forward {
        self.forward_framed(stream, restart_id, now, |id| frame::messages(json, id))
    }

    /// [`forward`](Self::forward) in the `e2e` build: `json` was received from the host at
    /// `received_ms` (ms since the Unix epoch) and goes as a `timed` message when it fits one
    /// (`frame::timed_messages`). A timed message counts as one message of its own length.
    #[cfg(feature = "e2e")]
    pub(crate) fn forward_timed(
        &mut self,
        stream: StreamId,
        restart_id: u64,
        json: &str,
        received_ms: f64,
        now: Duration,
    ) -> Forward {
        self.forward_framed(stream, restart_id, now, |id| {
            frame::timed_messages(json, received_ms, id)
        })
    }

    /// Forwards one event that `framed` turns into messages, given the next fragment id.
    fn forward_framed(
        &mut self,
        stream: StreamId,
        restart_id: u64,
        now: Duration,
        framed: impl FnOnce(u32) -> Option<Vec<Frame>>,
    ) -> Forward {
        if self.paused || !self.is_current(stream, restart_id) {
            return Forward::Drop;
        }
        let Some(messages) = framed(self.next_fragment) else {
            return Forward::TooLarge;
        };
        if messages.len() > 1 {
            self.next_fragment = self.next_fragment.wrapping_add(1);
        }
        for message in &messages {
            self.count(message, now);
        }
        let pause = self.unacked_bytes() > UNACKED_CAP;
        if pause {
            self.paused = true;
        }
        Forward::Send { messages, pause }
    }

    /// Counts `message`, one the bridge writes itself (a stream failure), sent at `now`. It
    /// goes out paused or not, and does not pause the window: its stream's retries bound how
    /// often it is sent.
    pub(crate) fn count_notice(&mut self, message: &Frame, now: Duration) {
        self.count(message, now);
    }

    /// Counts one message sent at `now`.
    fn count(&mut self, message: &Frame, now: Duration) {
        if self.unacked_bytes() == 0 {
            self.progress_at = now;
        }
        self.sent_bytes += message.len() as u64;
        self.in_flight.push_back(self.sent_bytes);
        self.sent += 1;
    }

    /// The webview applied each stream in `positions` up to its position and has processed
    /// `messages` messages in all. Nothing changes when the ack is refused.
    ///
    /// A `messages` lower than one already acknowledged (acks that crossed) retires nothing, and
    /// a position behind the one held for the same epoch is a replay and changes nothing.
    pub(crate) fn ack(
        &mut self,
        positions: &[Ack],
        messages: u32,
        now: Duration,
    ) -> Result<Acked, AckError> {
        let messages = u64::from(messages);
        if messages > self.sent {
            return Err(AckError::TooManyMessages);
        }
        let mut named = BTreeSet::new();
        if let Some(repeated) = positions.iter().find(|ack| !named.insert(ack.stream)) {
            return Err(AckError::RepeatedStream(repeated.stream));
        }
        if messages > self.acked {
            for _ in self.acked..messages {
                if let Some(total) = self.in_flight.pop_front() {
                    self.acked_bytes = total;
                }
            }
            self.acked = messages;
            self.progress_at = now;
        }
        let mut acked = Acked::default();
        for ack in positions {
            let Some(followed) = self.streams.get_mut(&ack.stream) else {
                continue;
            };
            let replay = followed
                .acked
                .is_some_and(|(epoch, cursor)| epoch == ack.epoch && cursor > ack.cursor);
            if !replay {
                followed.acked = Some((ack.epoch, ack.cursor));
            }
            acked.positions.push(*ack);
        }
        if self.paused && self.unacked_bytes() == 0 {
            acked.restarts = self.restart_all(now);
        }
        Ok(acked)
    }

    /// The clock moved to `now`. Returns the subscriptions to restart: every stream's when the
    /// watchdog fired, else those of failed streams whose retry is due (none while paused).
    pub(crate) fn tick(&mut self, now: Duration) -> Vec<Restart> {
        let stalled = now.saturating_sub(self.progress_at) >= WATCHDOG;
        let armed = self.acked >= self.rearm_at;
        if self.unacked_bytes() > 0 && stalled && armed {
            self.progress_at = now;
            self.rearm_at = self.sent;
            return self.restart_all(now);
        }
        if self.paused {
            return Vec::new();
        }
        let mut restarts = Vec::new();
        for (stream, followed) in &mut self.streams {
            let Some(retry) = followed.retry.as_mut().filter(|retry| retry.at <= now) else {
                continue;
            };
            retry.at = now + retry.wait;
            self.last_restart_id += 1;
            followed.restart_id = self.last_restart_id;
            restarts.push(Restart {
                stream: *stream,
                id: self.last_restart_id,
                after: followed.acked,
            });
        }
        restarts
    }

    /// The host refused `stream`'s subscription `restart_id` at `now`. When that is still the
    /// current one, schedules its next try and returns true: the webview is to be told.
    pub(crate) fn failed(&mut self, stream: StreamId, restart_id: u64, now: Duration) -> bool {
        let Some(followed) = self
            .streams
            .get_mut(&stream)
            .filter(|followed| followed.restart_id == restart_id)
        else {
            return false;
        };
        let wait = followed
            .retry
            .map_or(RETRY_FIRST, |retry| (retry.wait * 2).min(RETRY_MAX));
        followed.retry = Some(Retry {
            at: now + wait,
            wait,
        });
        true
    }

    /// `stream`'s subscription `restart_id` started: when it is current, no retry is due.
    pub(crate) fn started(&mut self, stream: StreamId, restart_id: u64) {
        if let Some(followed) = self
            .streams
            .get_mut(&stream)
            .filter(|followed| followed.restart_id == restart_id)
        {
            followed.retry = None;
        }
    }

    /// Ends a pause and gives every followed stream a new restart id from its last position.
    /// A failed stream's next retry moves to a full wait from `now`, so the clock does not
    /// restart it a second time.
    fn restart_all(&mut self, now: Duration) -> Vec<Restart> {
        self.paused = false;
        let mut restarts = Vec::with_capacity(self.streams.len());
        for (stream, followed) in &mut self.streams {
            self.last_restart_id += 1;
            followed.restart_id = self.last_restart_id;
            if let Some(retry) = followed.retry.as_mut() {
                retry.at = now + retry.wait;
            }
            restarts.push(Restart {
                stream: *stream,
                id: self.last_restart_id,
                after: followed.acked,
            });
        }
        restarts
    }
}

#[cfg(test)]
mod tests;
