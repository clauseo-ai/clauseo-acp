//! Encode once: each `HostEvent::Patches` is written as `serde_json` text one time, into a scratch
//! buffer the actor reuses, then copied into an exact-size `Arc<str>` that every subscriber of the
//! stream shares.
//!
//! A batch stays under [`MAX_BATCH_BYTES`], the size below which Tauri's Channel takes its fast
//! path: the encoder closes the batch before a patch that would take it to 8 KB or more. A single
//! patch that does not fit goes alone. The batch's JSON is the same text `serde_json::to_string`
//! gives for the `HostEvent`: a fixed prefix (stream and epoch), the patches joined by `,`, and
//! [`BATCH_SUFFIX`].

use std::sync::Arc;

use clauseo_protocol::{HostEvent, PatchBatch, StreamId, ViewEpoch, ViewPatch};

use crate::error::Error;

/// The largest encoded batch is one byte under this, unless it holds a single patch.
pub(crate) const MAX_BATCH_BYTES: usize = 8192;

/// What closes a `HostEvent::Patches` after its last patch: the patch list, the batch, the event.
pub(crate) const BATCH_SUFFIX: &[u8] = b"]}}";

/// One `HostEvent` as `serde_json` text, encoded once by the host and shared by every subscriber
/// of its stream. A viewer sends [`json`](Self::json) on as it is and never re-encodes it.
#[derive(Clone, Debug)]
pub struct EncodedEvent(Arc<str>);

impl EncodedEvent {
    /// Copies `bytes`, which `serde_json` wrote, into an exact-size shared string.
    pub(crate) fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        let text = std::str::from_utf8(bytes)
            .map_err(|err| Error::Internal(format!("encoded event is not UTF-8: {err}")))?;
        Ok(Self(Arc::from(text)))
    }

    /// The event as `serde_json` text.
    pub fn json(&self) -> &str {
        &self.0
    }

    /// The event as its type, for tests and Rust callers that need it.
    pub fn decode(&self) -> Result<HostEvent, serde_json::Error> {
        serde_json::from_str(&self.0)
    }

    /// Whether both share the same text: one encoding handed to several subscribers.
    #[cfg(feature = "test-support")]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// The actor's encoding buffer, reused for every batch so encoding allocates only the shared
/// copy of each result.
#[derive(Debug, Default)]
pub(crate) struct Encoder {
    pub(crate) scratch: Vec<u8>,
}

impl Encoder {
    /// An encoder whose buffer already holds one full batch.
    pub(crate) fn new() -> Self {
        Self {
            scratch: Vec::with_capacity(MAX_BATCH_BYTES),
        }
    }
}

/// The JSON of a `HostEvent::Patches` for `stream` and `epoch` up to the opening `[` of its patch
/// list. Every batch of a stream view starts with it, so the view keeps one copy.
pub(crate) fn batch_prefix(stream: StreamId, epoch: ViewEpoch) -> Result<Box<[u8]>, Error> {
    let empty = HostEvent::Patches(PatchBatch {
        stream,
        epoch,
        patches: Vec::new(),
    });
    let mut bytes = serde_json::to_vec(&empty).map_err(|err| encode_error(&err))?;
    // `patches` is the batch's last field, so the empty batch ends with `[]}}`.
    let cut = bytes
        .len()
        .checked_sub(BATCH_SUFFIX.len())
        .filter(|_| bytes.ends_with(b"[]}}"));
    let Some(cut) = cut else {
        return Err(Error::Internal(
            "a patch batch does not end with its patch list".to_owned(),
        ));
    };
    bytes.truncate(cut);
    Ok(bytes.into_boxed_slice())
}

/// Writes batches of patches into `scratch`, closing each before it would reach
/// [`MAX_BATCH_BYTES`], and collects the encoded events.
///
/// The patch is written in place after the batch so far. When it does not fit, the batch is
/// closed at the patch's comma (the three bytes there are saved, overwritten with
/// [`BATCH_SUFFIX`], copied out, and put back), and the patch's bytes move to just after the
/// prefix, where they start the next batch. Nothing is encoded twice.
pub(crate) struct BatchWriter<'a> {
    scratch: &'a mut Vec<u8>,
    prefix_len: usize,
    in_batch: usize,
    out: Vec<EncodedEvent>,
}

impl<'a> BatchWriter<'a> {
    /// Starts the first batch in `scratch`, which is cleared first.
    pub(crate) fn new(scratch: &'a mut Vec<u8>, prefix: &[u8]) -> Self {
        scratch.clear();
        scratch.extend_from_slice(prefix);
        Self {
            scratch,
            prefix_len: prefix.len(),
            in_batch: 0,
            out: Vec::new(),
        }
    }

    /// Encodes `patch` into the current batch, or into a new one when it would take the current
    /// one to [`MAX_BATCH_BYTES`]. Returns the patch's own `serde_json` text.
    pub(crate) fn push(&mut self, patch: &ViewPatch) -> Result<&[u8], Error> {
        let mark = self.scratch.len();
        if self.in_batch > 0 {
            self.scratch.push(b',');
        }
        let start = self.scratch.len();
        serde_json::to_writer(&mut *self.scratch, patch).map_err(|err| encode_error(&err))?;
        let end = self.scratch.len();
        let range = if self.in_batch > 0 && end + BATCH_SUFFIX.len() >= MAX_BATCH_BYTES {
            self.split_before(mark, start, end)?
        } else {
            start..end
        };
        self.in_batch += 1;
        self.scratch.get(range).ok_or_else(|| out_of_range("patch"))
    }

    /// Closes the batch at `mark` (the comma before the patch at `start..end`) and moves the patch
    /// to start the next batch. Returns where the patch now is.
    fn split_before(
        &mut self,
        mark: usize,
        start: usize,
        end: usize,
    ) -> Result<std::ops::Range<usize>, Error> {
        let close = mark..mark + BATCH_SUFFIX.len();
        // The comma and the patch's first two bytes: a patch is a JSON object, at least `{}`.
        let mut saved = [0u8; BATCH_SUFFIX.len()];
        saved.copy_from_slice(
            self.scratch
                .get(close.clone())
                .ok_or_else(|| out_of_range("mark"))?,
        );
        let slot = self
            .scratch
            .get_mut(close.clone())
            .ok_or_else(|| out_of_range("mark"))?;
        slot.copy_from_slice(BATCH_SUFFIX);
        let batch = self
            .scratch
            .get(..close.end)
            .ok_or_else(|| out_of_range("batch"))?;
        self.out.push(EncodedEvent::from_json(batch)?);
        let slot = self
            .scratch
            .get_mut(close)
            .ok_or_else(|| out_of_range("mark"))?;
        slot.copy_from_slice(&saved);
        self.scratch.copy_within(start..end, self.prefix_len);
        let moved = self.prefix_len..self.prefix_len + (end - start);
        self.scratch.truncate(moved.end);
        self.in_batch = 0;
        Ok(moved)
    }

    /// Adds `json`, one patch's text that was encoded before (a window's tail), with the same
    /// split rule as [`push`](Self::push).
    pub(crate) fn push_encoded(&mut self, json: &[u8]) -> Result<(), Error> {
        let comma = usize::from(self.in_batch > 0);
        let end = self.scratch.len() + comma + json.len();
        if self.in_batch > 0 && end + BATCH_SUFFIX.len() >= MAX_BATCH_BYTES {
            self.close()?;
        } else if comma == 1 {
            self.scratch.push(b',');
        }
        self.scratch.extend_from_slice(json);
        self.in_batch += 1;
        Ok(())
    }

    /// Closes the current batch, when it holds a patch, and starts the next one.
    fn close(&mut self) -> Result<(), Error> {
        if self.in_batch == 0 {
            return Ok(());
        }
        self.scratch.extend_from_slice(BATCH_SUFFIX);
        self.out.push(EncodedEvent::from_json(self.scratch)?);
        self.scratch.truncate(self.prefix_len);
        self.in_batch = 0;
        Ok(())
    }

    /// Closes the last batch and returns every encoded batch, in cursor order.
    pub(crate) fn finish(mut self) -> Result<Vec<EncodedEvent>, Error> {
        self.close()?;
        Ok(self.out)
    }
}

/// A `serde_json` failure while encoding: a host bug, since every patch type serialises.
pub(crate) fn encode_error(err: &serde_json::Error) -> Error {
    Error::Internal(format!("encoding a host event failed: {err}"))
}

fn out_of_range(what: &str) -> Error {
    Error::Internal(format!("batch encoder: {what} out of range"))
}

#[cfg(test)]
mod tests;
