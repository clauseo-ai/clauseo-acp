//! `clauseo-reducer`: the pure fold from a stream's events to its state and the view changes
//! they cause.
//!
//! # Owns
//!
//! [`StreamState`] (a thread's or a matter's), [`apply`], which folds one envelope into it and
//! returns only the view changes it caused, [`apply_text_delta`] for display-stage text that is
//! not an event yet, [`view_snapshot`] and [`page_entries`] for (re)subscribing and scrolling
//! back, and the snapshot format: [`Snapshot`], [`SnapshotRef`], [`REDUCER_SCHEMA_VERSION`].
//!
//! # Interface
//!
//! `apply(state, envelope)` takes an envelope only when its stream is the state's and its `seq`
//! is the state's `seq + 1` (1 for a new state); otherwise it returns [`Error::WrongStream`] or
//! [`Error::SeqOutOfOrder`] and leaves the state as it was. It returns [`Applied`]: the
//! [`ViewChange`]s, without cursors (`host` maps each to a `ViewPatchBody` and stamps the next
//! cursor), and the [`Anomaly`]s it met, which `host` logs. `Snapshot::of(state).to_bytes()`
//! writes a snapshot from a borrow; `Snapshot::from_bytes(bytes)?.into_state()` reads one back,
//! or fails with [`Error::SnapshotDecode`] or [`Error::SchemaMismatch`], and `host` then replays
//! from events. `store` and `host` decide when to take snapshots; this crate defines what they
//! hold.
//!
//! Host contract for text deltas, which `host` keeps so that a message a delta opens is already
//! at the order its first `TextAppended` gets:
//!
//! 1. `host` passes display-stage text to `apply_text_delta` with `pending_seq`, its pending head:
//!    the last `seq` it has sent to the store for this stream, never behind the state's `seq`.
//! 2. Its durability batch turns the text it holds into `TextAppended` events and sends them. It
//!    is flushed before any non-text event of the stream is sent.
//! 3. A message's first delta closes the current batch: before passing that delta, `host`
//!    flushes the batch (the other messages' un-durable text) and passes the pending head that
//!    results as `pending_seq`. The new message opens the next batch, and that batch sends its
//!    `TextAppended` first, so its `seq` is `pending_seq + 1`.
//! 4. The reducer opens the message at `max(last entry order, pending_seq) + 1`. Under rule 3 no
//!    entry is above the pending head when a message opens, so that is `pending_seq + 1`, the
//!    `seq` its first `TextAppended` gets, and the event confirms it in place with no change.
//!
//! A first `TextAppended` at another `seq` breaks the contract: the reducer moves the entry to
//! that `seq`, sends it again and reports [`Anomaly::OrderCorrected`].
//!
//! # Invariants
//!
//! - No I/O, no clock, no randomness: time comes in as the envelope's `ts`.
//! - The fold is total and never panics: an event that names something the state does not hold
//!   changes nothing and adds an [`Anomaly`]; an unknown kind advances `seq` and changes nothing.
//!   Queue and steer events and the restart and idle-stop detach reasons are not folded yet:
//!   they advance `seq` and add [`Anomaly::NotYetFolded`].
//! - No change for an unchanged entry: each field is compared as it is set, and an `Upsert` is
//!   emitted only when something changed.
//! - No change carries more than `INLINE_LIMIT_BYTES` of text, counted as `escaped_len` counts
//!   it: a longer append is split at `char` boundaries, and the `Upsert` of a message carries at
//!   most that much text, followed by `AppendText` changes for the rest. A user message's `Text`
//!   blocks are cut to the limit in all, the rest sent with `AppendTarget::UserBlock`.
//! - Each message keeps the UTF-16 length of its shown text and of its durable part beside the
//!   text; an `AppendText`'s `from_len` is the shown count before it, never a rescan.
//! - Un-durable text: the view snapshot shows it; a snapshot drops it (each message cut to its
//!   durable part, messages only deltas opened left out), so a snapshot equals the fold of the
//!   log. When `TextAppended` matches the shown un-durable text, nothing is emitted (when it
//!   extends it, only the rest is appended); on a mismatch the un-durable text is dropped and
//!   the entry sent again ([`Anomaly::DeltaMismatch`]).
//! - Entries live in a `BTreeMap` by `order`; the id indexes are std `HashMap`s, never iterated
//!   or saved, rebuilt when a state is decoded. Everything serialised is ordered, so equal
//!   states serialise to equal bytes.
//! - [`REDUCER_SCHEMA_VERSION`] versions the snapshot shape: any change to a state field or how
//!   it serialises bumps it (the insta test `snapshot_shape_is_pinned` fails until it is).
//!
//! # Depends on
//!
//! `clauseo-protocol`, `serde`, `rmp-serde` (snapshot bytes) and `thiserror`. Never any I/O,
//! the clock or `tauri` (`ARCHITECTURE.md`).
//!
//! # Start reading
//!
//! `apply` below, then `thread.rs` (the thread fold and its status rules), `text.rs` (text,
//! deltas and the durable part), `split.rs` (splitting text at the inline limit), `order.rs`
//! (inserting entries and moving a message a delta opened), `matter.rs`, `state.rs` (the state
//! and its indexes), `snapshot.rs` (the snapshot format), `view.rs` and `change.rs`. The tests
//! are one binary, `tests/reducer.rs` with its modules in `tests/reducer/`, and shared fixtures in
//! `tests/support/fixtures.rs`; the instruction-count bench is the `clauseo-reducer_bench`
//! package (`crates/reducer_bench`).

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod change;
mod error;
mod matter;
mod order;
mod snapshot;
mod split;
mod state;
mod text;
mod thread;
mod view;

use clauseo_protocol::Envelope;

pub use change::{Anomaly, Applied, TextDelta, ViewChange};
pub use error::Error;
pub use snapshot::{REDUCER_SCHEMA_VERSION, Snapshot, SnapshotRef};
pub use state::{MatterState, StreamState, ThreadState};
pub use view::{VIEW_SNAPSHOT_ENTRIES, page_entries, view_snapshot};

/// A constant for xtask's `test_reducer` buildtime probe: it edits it to force a rebuild of this
/// crate. It means nothing at run time.
#[doc(hidden)]
pub const BUILDTIME_MARKER: u64 = 0;

/// Folds `envelope` into `state` and returns the view changes it caused.
///
/// Refuses, leaving `state` unchanged, an envelope of another stream ([`Error::WrongStream`])
/// or one whose `seq` is not the state's `seq + 1` ([`Error::SeqOutOfOrder`]).
pub fn apply(state: &mut StreamState, envelope: &Envelope) -> Result<Applied, Error> {
    let stream = state.stream();
    if envelope.stream != stream {
        return Err(Error::WrongStream {
            expected: stream,
            got: envelope.stream,
        });
    }
    let expected = state.seq().map_or(1, |seq| seq.get().saturating_add(1));
    if envelope.seq.get() != expected {
        return Err(Error::SeqOutOfOrder {
            expected,
            got: envelope.seq.get(),
        });
    }
    let mut out = Applied::default();
    match state {
        StreamState::Thread(thread) => {
            thread::fold(thread, envelope, &mut out);
            thread.seq = Some(envelope.seq);
        }
        StreamState::Matter(matter) => {
            matter::fold(matter, envelope, &mut out);
            matter.seq = Some(envelope.seq);
        }
    }
    Ok(out)
}

/// Folds display-stage text into the thread: appends it to its message's shown text (an
/// `AppendText`), or opens the message (an `Upsert`) at a provisional order, which the host
/// contract above makes the `seq` of its first `TextAppended`. The text is not durable until its
/// `TextAppended` is applied.
///
/// Refuses a delta whose `pending_seq` is behind the state's `seq`
/// ([`Error::PendingSeqBehind`]).
pub fn apply_text_delta(state: &mut ThreadState, delta: &TextDelta<'_>) -> Result<Applied, Error> {
    text::text_delta(state, delta)
}
