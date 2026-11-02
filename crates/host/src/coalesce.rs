//! Text coalescing for one thread: the display batch and the durable batch. Time comes in as an
//! argument (the host clock's `mono`); this module reads no clock and arms no timer itself.
//!
//! The display batch leads with its first text: a delta that comes after a quiet
//! [`DISPLAY_BATCH`] (no display flush in the last 16 ms, a durable flush included) is shown at
//! once, so the first token of a turn does not wait for a timer. A delta inside that window
//! waits for one timer, at the window's end, and that flush carries everything since. So the
//! screen gets at most one flush per 16 ms, a steady stream is shown every 16 ms, and no text
//! waits longer than 16 ms after the previous flush.
//!
//! Each message's un-durable text is held once, in first-delta order, with how much of it the
//! display has shown. A display flush hands the unshown part of each message to the caller,
//! borrowed, for `reducer::apply_text_delta`. A durable flush first hands over whatever the display
//! has not shown yet, then gives one `TextAppended` per message in first-delta order. So the only
//! order a caller can take is: display, then durable, then the non-text event that needed the
//! flush (R6's host contract).
//!
//! The actor keeps rule 3 of that contract: before a message's first delta goes in, it runs the
//! durable flush (which includes the display flush), then pushes the delta, so the new message
//! opens the next batch and its `TextAppended` is that batch's first event.

use std::time::Duration;

use clauseo_protocol::{CompactString, MessageId, TextAppended, TextKind, TurnId};

/// The display batch: at most one display flush per this window. Text that comes after a quiet
/// window is shown at once; text inside the window waits for the window's end.
pub(crate) const DISPLAY_BATCH: Duration = Duration::from_millis(16);

/// How long un-durable text waits before it is saved as `TextAppended` events.
pub(crate) const DURABLE_BATCH: Duration = Duration::from_millis(250);

/// Un-durable text held at which the durable batch is flushed at once.
pub(crate) const DURABLE_FLUSH_BYTES: usize = 8 * 1024;

/// One text delta from the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextIn {
    pub(crate) turn: TurnId,
    pub(crate) message: MessageId,
    pub(crate) acp_message_id: Option<CompactString>,
    pub(crate) kind: TextKind,
    pub(crate) text: String,
}

/// Display text of one message that the screen has not shown yet, merged from its deltas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DisplayText<'a> {
    pub(crate) turn: TurnId,
    pub(crate) message: MessageId,
    pub(crate) kind: TextKind,
    pub(crate) text: &'a str,
}

/// What a push asks of the caller: display text to show now or a timer to arm (deadlines in
/// `mono` time), and whether the durable batch is full and must be flushed now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub(crate) struct Pushed {
    /// Show the display text now (run the display flush): the first delta since the last
    /// display flush, and no display flush in the last [`DISPLAY_BATCH`]. No timer is needed.
    pub(crate) display_now: bool,
    /// Arm the display timer for this deadline: the first delta since the last display flush,
    /// inside [`DISPLAY_BATCH`] of it. The deadline is the end of that window.
    pub(crate) display_at: Option<Duration>,
    /// Arm the durable timer for this deadline: the first un-durable delta since the last
    /// durable flush.
    pub(crate) durable_at: Option<Duration>,
    /// [`DURABLE_FLUSH_BYTES`] of un-durable text are held: flush the durable batch now.
    pub(crate) full: bool,
}

/// One message's un-durable text.
#[derive(Debug)]
struct Held {
    turn: TurnId,
    message: MessageId,
    acp_message_id: Option<CompactString>,
    kind: TextKind,
    text: String,
    /// Bytes of `text` the display has shown; always at a `char` boundary.
    shown: usize,
}

impl Held {
    /// The part of the text the display has not shown, when there is one.
    fn unshown(&self) -> Option<DisplayText<'_>> {
        let text = self
            .text
            .get(self.shown..)
            .filter(|text| !text.is_empty())?;
        Some(DisplayText {
            turn: self.turn,
            message: self.message,
            kind: self.kind,
            text,
        })
    }
}

/// The text batches of one thread.
#[derive(Debug, Default)]
pub(crate) struct ThreadText {
    /// Un-durable text per message, in first-delta order.
    held: Vec<Held>,
    /// Bytes of text in `held`.
    held_bytes: usize,
    /// Text arrived since the last display flush (or the last durable flush).
    display_armed: bool,
    /// When the display last flushed (a durable flush runs the display flush too), in `mono`
    /// time; `None` until the first flush.
    last_display: Option<Duration>,
}

impl ThreadText {
    /// No text held.
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether no un-durable text is held for `message`: its next delta closes the current
    /// durable batch first (rule 3).
    pub(crate) fn is_new_message(&self, message: MessageId) -> bool {
        !self.held.iter().any(|held| held.message == message)
    }

    /// Whether some text has not been shown yet.
    pub(crate) fn has_display(&self) -> bool {
        self.held.iter().any(|held| held.shown < held.text.len())
    }

    /// Whether some text is not durable yet.
    pub(crate) fn has_durable(&self) -> bool {
        !self.held.is_empty()
    }

    /// Adds one delta, merged into its message's held text. An empty delta changes nothing.
    pub(crate) fn push(&mut self, now: Duration, delta: TextIn) -> Pushed {
        if delta.text.is_empty() {
            return Pushed::default();
        }
        let mut pushed = Pushed::default();
        if self.held.is_empty() {
            pushed.durable_at = Some(now.saturating_add(DURABLE_BATCH));
        }
        if !self.display_armed {
            self.display_armed = true;
            match self.last_display {
                Some(last) if now.saturating_sub(last) < DISPLAY_BATCH => {
                    pushed.display_at = Some(last.saturating_add(DISPLAY_BATCH));
                }
                _ => pushed.display_now = true,
            }
        }
        self.held_bytes += delta.text.len();
        if let Some(held) = self
            .held
            .iter_mut()
            .find(|held| held.message == delta.message)
        {
            held.text.push_str(&delta.text);
        } else {
            let TextIn {
                turn,
                message,
                acp_message_id,
                kind,
                text,
            } = delta;
            self.held.push(Held {
                turn,
                message,
                acp_message_id,
                kind,
                text,
                shown: 0,
            });
        }
        pushed.full = self.held_bytes >= DURABLE_FLUSH_BYTES;
        pushed
    }

    /// Whether a display timer that fires at `now` should flush: some text is unshown and the
    /// display has not flushed within the last [`DISPLAY_BATCH`]. A timer armed before a later
    /// flush finds the window still open and does nothing; the timer of the current batch is
    /// the one that flushes.
    pub(crate) fn display_due(&self, now: Duration) -> bool {
        self.has_display()
            && self
                .last_display
                .is_none_or(|last| now.saturating_sub(last) >= DISPLAY_BATCH)
    }

    /// The display flush at `now`: hands `show` the unshown text of each message, in
    /// first-delta order, and marks it shown once `show` returns `Ok`. The first error stops the
    /// flush; the text not shown stays for the next one. Only a flush that showed some text opens
    /// a new window: a durable flush with everything already shown (the one a new message's first
    /// delta runs, rule 3) leaves the window as it was, so an answer after a thought still leads.
    pub(crate) fn flush_display<E>(
        &mut self,
        now: Duration,
        mut show: impl FnMut(DisplayText<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        self.display_armed = false;
        let mut result = Ok(());
        let mut showed = false;
        for held in &mut self.held {
            if let Some(text) = held.unshown() {
                if let Err(error) = show(text) {
                    result = Err(error);
                    break;
                }
                held.shown = held.text.len();
                showed = true;
            }
        }
        if showed {
            self.last_display = Some(now);
        }
        result
    }

    /// The durable flush at `now`: runs the display flush through `show` (so it counts as a
    /// display flush), then returns one `TextAppended` per message in first-delta order and holds
    /// nothing more. On an error from `show` nothing is returned and the text stays held.
    pub(crate) fn flush_durable<E>(
        &mut self,
        now: Duration,
        show: impl FnMut(DisplayText<'_>) -> Result<(), E>,
    ) -> Result<Vec<TextAppended>, E> {
        self.flush_display(now, show)?;
        self.held_bytes = 0;
        Ok(self
            .held
            .drain(..)
            .map(|held| TextAppended {
                turn: held.turn,
                message: held.message,
                acp_message_id: held.acp_message_id,
                kind: held.kind,
                text: held.text,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests;
