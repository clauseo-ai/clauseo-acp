//! What a fold returns: the view changes it caused and the anomalies it met, and the text delta
//! the host passes in.

use clauseo_protocol::{AppendTarget, EntryId, MessageId, Seq, TextKind, TurnId, ViewEntry};

/// What one `apply` or `apply_text_delta` changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Applied {
    /// The view changes, in the order the screen applies them. Empty when nothing visible
    /// changed.
    pub changes: Vec<ViewChange>,
    /// What the fold met that a correct log never holds; `host` logs them.
    pub anomalies: Vec<Anomaly>,
}

/// One change to a stream's view, without a cursor: `host` maps it to a `ViewPatchBody` and
/// stamps the next cursor. There is no `Reset`: the reducer never emits one.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewChange {
    /// Add an entry, or replace the entry with the same id. A message's text in it is cut to at
    /// most `INLINE_LIMIT_BYTES`, and `AppendText` changes after it carry the rest.
    Upsert(Box<ViewEntry>),
    /// Remove an entry. No event of this version removes one.
    Remove(EntryId),
    /// Append `text` to a message's text, which is `from_len` UTF-16 code units long before it.
    AppendText {
        /// The message.
        message_id: MessageId,
        /// Which text of the message grows: an agent text, or a block of a user message.
        target: AppendTarget,
        /// The text's length before this append, in UTF-16 code units.
        from_len: u32,
        /// The text to append; its `escaped_len` is at most `INLINE_LIMIT_BYTES`.
        text: String,
    },
}

/// Something a correct log never holds. The fold goes on: the event changes nothing it cannot
/// apply, and the anomaly says what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anomaly {
    /// The event names an entry the state does not hold (a `ToolCallUpdated` of an unknown tool
    /// call, a thread event before `ThreadCreated`). It changed nothing.
    UnknownReference {
        /// The event's kind.
        event: &'static str,
        /// The entry it names.
        target: EntryId,
    },
    /// The event creates an entry that already exists. It changed nothing.
    Duplicate {
        /// The event's kind.
        event: &'static str,
        /// The entry.
        target: EntryId,
    },
    /// The event settles a permission or an elicitation that is no longer pending (a second
    /// resolution). It changed nothing.
    AlreadySettled {
        /// The event's kind.
        event: &'static str,
        /// The entry.
        target: EntryId,
    },
    /// A `TextAppended` did not match the text its deltas showed: the shown text past the
    /// durable part was dropped, the event's text appended, and the whole entry sent again.
    DeltaMismatch {
        /// The message.
        message: MessageId,
    },
    /// A message opened by a delta, or moved out of an event's place, got another `order`.
    OrderCorrected {
        /// The message.
        message: MessageId,
        /// Its order before.
        from: Seq,
        /// Its order now.
        to: Seq,
    },
    /// Text of the other kind for an agent message: it was appended to the message, which keeps
    /// the kind of its first text.
    KindMismatch {
        /// The message.
        message: MessageId,
        /// The kind of the text that did not match.
        kind: TextKind,
    },
    /// A matter event in a thread's stream, or a thread event in a matter's. It changed nothing.
    WrongStreamKind {
        /// The event's kind.
        event: &'static str,
    },
    /// An event this version does not fold yet (queue and steer events, the restart and idle-stop
    /// detach reasons). It advanced `seq` and changed nothing else.
    NotYetFolded {
        /// The event's kind.
        event: &'static str,
    },
}

/// Display-stage text the host passes in before the text is an event: it reaches the screen
/// at once and the log later, as a `TextAppended`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextDelta {
    /// The turn.
    pub turn: TurnId,
    /// The agent's message.
    pub message: MessageId,
    /// Answer or thought.
    pub kind: TextKind,
    /// The text.
    pub text: String,
    /// The host's pending head: the last `seq` it has sent to the store for this stream.
    pub pending_seq: Seq,
}
