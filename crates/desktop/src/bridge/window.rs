//! One window's stream state, with no I/O: which streams it follows, what it has sent, what the
//! webview has acknowledged, and when to drop and restart its host subscriptions. The driver
//! (`pump.rs`) feeds it events, acks and clock ticks and carries out what it returns. Time comes
//! in as an argument, the time since the window opened, so tests run it with no clock or sleep.
//!
//! Backpressure counts bytes by message: each message sent appends the running total of bytes
//! sent, and an ack that says the webview has processed `frames` messages in all reads the total
//! after the `frames`th. The bridge never parses an event to count it.
//!
//! - Past [`UNACKED_CAP`] unacknowledged bytes the window pauses: every host subscription is
//!   dropped and nothing more is forwarded. Once acks bring the count back to 0, each stream
//!   restarts after its last acknowledged `(epoch, cursor)`, or from a `Reset` when it has none.
//! - [`WATCHDOG`] with bytes unacknowledged and no ack progress restarts every stream the same
//!   way at once, paused or not, so a lost ack cannot stall the window.
//!
//! A restart gives each stream a new generation; events from an older generation's
//! subscription are dropped, so a stream never mixes two subscriptions.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use clauseo_protocol::{Cursor, StreamId, ViewEpoch};

use crate::frame::{self, Frame};

/// The most bytes a window may have unacknowledged before it pauses.
pub(crate) const UNACKED_CAP: u64 = 256 * 1024;

/// How long bytes may stay unacknowledged with no ack progress before every stream restarts.
pub(crate) const WATCHDOG: Duration = Duration::from_secs(5);

/// A position in a stream's view: the epoch and the last cursor applied.
pub(crate) type Position = (ViewEpoch, Cursor);

/// A host subscription to (re)start: the stream, its new generation and where to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Restart {
    /// The stream.
    pub(crate) stream: StreamId,
    /// The generation the new subscription's events carry.
    pub(crate) generation: u64,
    /// The last acknowledged position, `None` for a `Reset`.
    pub(crate) after: Option<Position>,
}

/// What forwarding one event comes to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Forward {
    /// Send these frames, in order. `pause` says the window went past the cap with them:
    /// drop every host subscription after sending.
    Send {
        /// The frames.
        frames: Vec<Frame>,
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
    TooManyFrames,
}

/// One followed stream.
#[derive(Debug, Clone, Copy)]
struct Followed {
    /// The generation of its current subscription.
    generation: u64,
    /// The last position the webview acknowledged, or where it subscribed from.
    acked: Option<Position>,
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
    /// The last generation handed out.
    generation: u64,
    /// Past the cap, waiting for acks to catch up.
    paused: bool,
    /// When acks last made progress (or the watchdog last fired).
    progress_at: Duration,
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
            generation: 0,
            paused: false,
            progress_at: Duration::ZERO,
            streams: BTreeMap::new(),
        }
    }

    /// Follows `stream` from `after` (or restarts it there). Returns the generation its
    /// subscription's events must carry. While the window is paused the subscription is not
    /// started: the caller starts it only when [`is_paused`](Self::is_paused) is false.
    pub(crate) fn follow(&mut self, stream: StreamId, after: Option<Position>) -> u64 {
        self.generation += 1;
        let generation = self.generation;
        self.streams.insert(
            stream,
            Followed {
                generation,
                acked: after,
            },
        );
        generation
    }

    /// Stops following `stream`. Returns whether it was followed.
    pub(crate) fn unfollow(&mut self, stream: StreamId) -> bool {
        self.streams.remove(&stream).is_some()
    }

    /// Whether `generation` is `stream`'s current subscription.
    pub(crate) fn is_current(&self, stream: StreamId, generation: u64) -> bool {
        self.streams
            .get(&stream)
            .is_some_and(|followed| followed.generation == generation)
    }

    /// Whether the window is past the cap, waiting for acks.
    pub(crate) fn is_paused(&self) -> bool {
        self.paused
    }

    /// The bytes sent and not yet acknowledged.
    pub(crate) fn unacked_bytes(&self) -> u64 {
        self.sent_bytes - self.acked_bytes
    }

    /// Frames `json`, an event of `stream`'s subscription `generation`, and counts what it sends.
    /// The first bytes sent with nothing unacknowledged start the watchdog's wait at `now`.
    pub(crate) fn forward(
        &mut self,
        stream: StreamId,
        generation: u64,
        json: &str,
        now: Duration,
    ) -> Forward {
        if self.paused || !self.is_current(stream, generation) {
            return Forward::Drop;
        }
        let Some(frames) = frame::frames(json, self.next_fragment) else {
            return Forward::TooLarge;
        };
        if frames.len() > 1 {
            self.next_fragment = self.next_fragment.wrapping_add(1);
        }
        if self.unacked_bytes() == 0 {
            self.progress_at = now;
        }
        for sent in &frames {
            self.sent_bytes += sent.len() as u64;
            self.in_flight.push_back(self.sent_bytes);
        }
        self.sent += frames.len() as u64;
        let pause = self.unacked_bytes() > UNACKED_CAP;
        if pause {
            self.paused = true;
        }
        Forward::Send { frames, pause }
    }

    /// The webview applied `stream` up to `position` and has processed `frames` messages in
    /// all. Returns the subscriptions to restart: every stream's, when this ack ended a pause.
    ///
    /// A `frames` lower than one already acknowledged (acks that crossed) retires nothing, and a
    /// position behind the one held for the same epoch is a replay and changes nothing.
    pub(crate) fn ack(
        &mut self,
        stream: StreamId,
        position: Position,
        frames: u32,
        now: Duration,
    ) -> Result<Vec<Restart>, AckError> {
        let frames = u64::from(frames);
        if frames > self.sent {
            return Err(AckError::TooManyFrames);
        }
        if frames > self.acked {
            for _ in self.acked..frames {
                if let Some(total) = self.in_flight.pop_front() {
                    self.acked_bytes = total;
                }
            }
            self.acked = frames;
            self.progress_at = now;
        }
        if let Some(followed) = self.streams.get_mut(&stream) {
            let replay = followed
                .acked
                .is_some_and(|(epoch, cursor)| epoch == position.0 && cursor > position.1);
            if !replay {
                followed.acked = Some(position);
            }
        }
        if self.paused && self.unacked_bytes() == 0 {
            return Ok(self.restart_all());
        }
        Ok(Vec::new())
    }

    /// The clock moved to `now`. Returns the subscriptions to restart when the watchdog fired.
    pub(crate) fn tick(&mut self, now: Duration) -> Vec<Restart> {
        let stalled = now.saturating_sub(self.progress_at) >= WATCHDOG;
        if self.unacked_bytes() == 0 || !stalled {
            return Vec::new();
        }
        self.progress_at = now;
        self.restart_all()
    }

    /// Ends a pause and gives every followed stream a new generation from its last position.
    fn restart_all(&mut self) -> Vec<Restart> {
        self.paused = false;
        let mut restarts = Vec::with_capacity(self.streams.len());
        for (stream, followed) in &mut self.streams {
            self.generation += 1;
            followed.generation = self.generation;
            restarts.push(Restart {
                stream: *stream,
                generation: self.generation,
                after: followed.acked,
            });
        }
        restarts
    }
}

#[cfg(test)]
mod tests;
