//! The view of one loaded stream: its epoch, its cursor and its patch window.
//!
//! [`StreamView::publish`] stamps the next cursors on the reducer's changes, encodes them once
//! into batches under 8 KB and keeps each patch's text in the window; the actor hands a clone of
//! each [`EncodedEvent`] to every subscriber of the stream. [`StreamView::catch_up`] serves a new
//! subscriber: the window's tail when its epoch is current and its cursor still inside, otherwise
//! one `Reset` at the current cursor. A `Reset` is encoded for the subscriber that needs it and
//! never kept.

use clauseo_protocol::{Cursor, StreamId, ViewEpoch, ViewPatch, ViewPatchBody};
use clauseo_reducer::{StreamState, ViewChange, view_snapshot};

use crate::encode::{BatchWriter, EncodedEvent, Encoder, batch_prefix};
use crate::error::Error;
use crate::window::PatchWindow;

/// One loaded stream's view: epoch, cursor and patch window.
#[derive(Debug)]
pub(crate) struct StreamView {
    epoch: ViewEpoch,
    cursor: Cursor,
    /// The JSON every batch of this stream starts with (stream and epoch).
    prefix: Box<[u8]>,
    window: PatchWindow,
}

impl StreamView {
    /// A view of `stream` at cursor 0 whose window holds at most `window_cap` bytes.
    pub(crate) fn new(
        stream: StreamId,
        epoch: ViewEpoch,
        window_cap: usize,
    ) -> Result<Self, Error> {
        Ok(Self {
            epoch,
            cursor: Cursor::ZERO,
            prefix: batch_prefix(stream, epoch)?,
            window: PatchWindow::new(window_cap),
        })
    }

    /// The epoch every cursor of this view belongs to.
    pub(crate) fn epoch(&self) -> ViewEpoch {
        self.epoch
    }

    /// The cursor of the last patch published, 0 before the first.
    pub(crate) fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// The serialised bytes the window holds.
    pub(crate) fn window_bytes(&self) -> usize {
        self.window.bytes_held()
    }

    /// Stamps cursors on the reducer's changes, keeps them in the window and encodes them once
    /// into batches under 8 KB. The caller hands clones of each event to every subscriber.
    pub(crate) fn publish(
        &mut self,
        changes: Vec<ViewChange>,
        enc: &mut Encoder,
    ) -> Result<Vec<EncodedEvent>, Error> {
        if changes.is_empty() {
            return Ok(Vec::new());
        }
        let count = changes.len() as u64;
        // Checked once, so no cursor below can fail halfway through the changes.
        Cursor::new(self.cursor.get().saturating_add(count)).map_err(|err| cursor_error(&err))?;
        let mut batch = BatchWriter::new(&mut enc.scratch, &self.prefix);
        for change in changes {
            let cursor = self.cursor.next().map_err(|err| cursor_error(&err))?;
            let patch = ViewPatch {
                cursor,
                body: patch_body(change),
            };
            let json = batch.push(&patch)?;
            self.window.push(json);
            self.cursor = cursor;
        }
        batch.finish()
    }

    /// For a new subscriber: the tail after `after` when its epoch is current and its cursor is
    /// still in the window (nothing when it is at the current cursor), else one `Reset` carrying
    /// `view_snapshot(state)` at the current cursor.
    pub(crate) fn catch_up(
        &self,
        after: Option<(ViewEpoch, Cursor)>,
        state: &StreamState,
        enc: &mut Encoder,
    ) -> Result<Vec<EncodedEvent>, Error> {
        if let Some((_, cursor)) = after.filter(|(epoch, _)| *epoch == self.epoch) {
            let mut batch = BatchWriter::new(&mut enc.scratch, &self.prefix);
            if self.window.tail(cursor, |json| batch.push_encoded(json))? {
                return batch.finish();
            }
        }
        Ok(vec![self.reset(state, enc)?])
    }

    /// One `Reset` at the current cursor carrying `view_snapshot(state)`, encoded alone.
    pub(crate) fn reset(
        &self,
        state: &StreamState,
        enc: &mut Encoder,
    ) -> Result<EncodedEvent, Error> {
        let view_window = Box::new(view_snapshot(state));
        let patch = ViewPatch {
            cursor: self.cursor,
            body: ViewPatchBody::Reset { view_window },
        };
        let mut batch = BatchWriter::new(&mut enc.scratch, &self.prefix);
        batch.push(&patch)?;
        let mut events = batch.finish()?;
        events
            .pop()
            .ok_or_else(|| Error::Internal("a reset encoded to no event".to_owned()))
    }
}

/// The patch body for one reducer change.
fn patch_body(change: ViewChange) -> ViewPatchBody {
    match change {
        ViewChange::Upsert(entry) => ViewPatchBody::Upsert { entry },
        ViewChange::Remove(entry_id) => ViewPatchBody::Remove { entry_id },
        ViewChange::AppendText {
            message_id,
            target,
            from_len,
            text,
        } => ViewPatchBody::AppendText {
            message_id,
            target,
            from_len,
            text,
        },
    }
}

fn cursor_error(err: &clauseo_protocol::Error) -> Error {
    Error::Internal(format!("view cursor overflow: {err}"))
}

#[cfg(test)]
mod tests;
