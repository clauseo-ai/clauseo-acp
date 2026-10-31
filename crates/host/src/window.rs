//! The patch window of one loaded stream: the most recent patches as the `serde_json` text the
//! encoder wrote for them, capped in bytes.
//!
//! A subscriber whose cursor is still inside gets the tail from here, spliced into batches with no
//! encoding; one whose cursor fell out gets a `Reset`. Each patch costs its `serde_json` length,
//! taken from the batch encode that wrote it. Past the cap the oldest patches drop; a single
//! patch over the cap empties the window and is not kept. There is no count cap: the byte cap
//! bounds memory, and a count would only drop small patches early.
//!
//! The text lives in one byte ring (`VecDeque<u8>`) with each patch's length beside it, so
//! dropping old patches moves the ring's head and allocates nothing, and the window holds its
//! cap in bytes plus eight bytes per patch.

use std::collections::VecDeque;

use clauseo_protocol::Cursor;

/// The most serialised patch bytes a stream's window holds.
pub(crate) const PATCH_WINDOW_BYTES: usize = 256 * 1024;

/// A stream's most recent patches, at most `cap` bytes of their `serde_json` text.
#[derive(Debug)]
pub(crate) struct PatchWindow {
    cap: usize,
    /// The patches' text, oldest first, back to back.
    bytes: VecDeque<u8>,
    /// Where each patch's text ends, oldest first, as a byte count since the window began; a
    /// tail finds where it starts in one step.
    ends: VecDeque<u64>,
    /// The byte count since the window began at the front of `bytes`.
    front: u64,
    /// The cursor just before the oldest kept patch; the window holds `base + 1 ..= base + n`.
    base: u64,
}

impl PatchWindow {
    /// An empty window at cursor 0 holding at most `cap` bytes.
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            cap,
            bytes: VecDeque::new(),
            ends: VecDeque::new(),
            front: 0,
            base: 0,
        }
    }

    /// Keeps `json`, the patch at the cursor after the window's last, dropping the oldest patches
    /// to stay within the cap. A patch over the cap empties the window and is not kept.
    pub(crate) fn push(&mut self, json: &[u8]) {
        let len = json.len();
        if len > self.cap {
            self.base += self.ends.len() as u64 + 1;
            self.front += self.bytes.len() as u64;
            self.bytes.clear();
            self.ends.clear();
            return;
        }
        let mut held = self.bytes.len();
        let mut dropped = 0u64;
        while held + len > self.cap {
            let Some(end) = self.ends.pop_front() else {
                break;
            };
            let old = end - self.front - dropped;
            held = held.saturating_sub(usize::try_from(old).unwrap_or(usize::MAX));
            dropped += old;
            self.base += 1;
        }
        self.bytes.drain(..self.bytes.len() - held);
        self.front += dropped;
        // Grow by doubling but never past the cap, so a full window holds the cap and no more.
        let want = held + len;
        if want > self.bytes.capacity() {
            let grow = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .min(self.cap)
                .max(want);
            self.bytes.reserve_exact(grow - held);
        }
        self.bytes.extend(json);
        self.ends.push_back(self.front + self.bytes.len() as u64);
    }

    /// The serialised bytes held: never more than the cap.
    #[cfg(test)]
    pub(crate) fn bytes_held(&self) -> usize {
        self.bytes.len()
    }

    /// Calls `each` with the text of every patch after `after`, oldest first, when the window
    /// still holds all of them; returns `false` (and calls nothing) when `after` fell out of the
    /// window or is past its end. A cursor at the end has an empty tail.
    pub(crate) fn tail<E>(
        &self,
        after: Cursor,
        mut each: impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<bool, E> {
        let last = self.base + self.ends.len() as u64;
        let after = after.get();
        if after < self.base || after > last {
            return Ok(false);
        }
        let Ok(skip) = usize::try_from(after - self.base) else {
            return Ok(false);
        };
        let offset = |end: u64| usize::try_from(end - self.front).unwrap_or(usize::MAX);
        let mut at = match skip.checked_sub(1) {
            Some(before) => self.ends.get(before).map_or(0, |end| offset(*end)),
            None => 0,
        };
        let (front, back) = self.bytes.as_slices();
        // A patch can straddle the ring's two halves; it is joined in `joined` then.
        let mut joined = Vec::new();
        for end in self.ends.iter().skip(skip) {
            let end = offset(*end);
            if let Some(json) = front.get(at..end) {
                each(json)?;
            } else if let Some(json) = at
                .checked_sub(front.len())
                .and_then(|from| back.get(from..end - front.len()))
            {
                each(json)?;
            } else {
                joined.clear();
                joined.extend_from_slice(front.get(at..).unwrap_or_default());
                joined.extend_from_slice(back.get(..end - front.len()).unwrap_or_default());
                each(&joined)?;
            }
            at = end;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
