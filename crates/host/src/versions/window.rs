//! A page of text: from one version's file as it streams out of the history ([`Window`]), or
//! from stetdoc's text of a Word file ([`page_of`]). A page starts at the first character
//! boundary at or after the asked offset and ends at the last one within [`PAGE_BYTES`], so it
//! is always whole UTF-8.

use std::io::{self, Write};

use super::PAGE_BYTES;

/// One page and where the next starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Page {
    /// The page's text.
    pub(super) text: String,
    /// Where it starts, in bytes.
    pub(super) offset: u64,
    /// Where the next page starts; `None` at the end.
    pub(super) next_offset: Option<u64>,
}

/// The page of `text` from `offset`, `None` when `offset` is past its end.
pub(super) fn page_of(text: &str, offset: u64) -> Option<Page> {
    let mut start = usize::try_from(offset)
        .ok()
        .filter(|start| *start <= text.len())?;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = text.len().min(start.saturating_add(PAGE_BYTES));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Some(Page {
        text: text.get(start..end)?.to_owned(),
        offset: start as u64,
        next_offset: (end < text.len()).then_some(end as u64),
    })
}

/// A writer that keeps the bytes of one page's reach from `offset` (a page and the three bytes
/// a character may run past it) and checks the whole stream is text: UTF-8 with no NUL byte.
#[derive(Debug)]
pub(super) struct Window {
    offset: u64,
    /// Bytes seen so far.
    seen: u64,
    kept: Vec<u8>,
    /// An unfinished character at the end of the last write.
    carry: Vec<u8>,
    text: bool,
}

impl Window {
    /// A window from `offset`.
    pub(super) fn new(offset: u64) -> Self {
        Self {
            offset,
            seen: 0,
            kept: Vec::new(),
            carry: Vec::new(),
            text: true,
        }
    }

    /// The page from `offset` once the whole file was written: `Ok(None)` when the file is not
    /// text, [`PastEnd`] when `offset` is past its end (at the end itself, an empty page).
    pub(super) fn page(self) -> Result<Option<Page>, PastEnd> {
        if !self.text || !self.carry.is_empty() {
            return Ok(None);
        }
        if self.offset > self.seen {
            return Err(PastEnd);
        }
        let mut start = 0;
        while self
            .kept
            .get(start)
            .is_some_and(|byte| is_continuation(*byte))
        {
            start += 1;
        }
        let mut end = self.kept.len().min(start + PAGE_BYTES);
        while self
            .kept
            .get(end)
            .is_some_and(|byte| is_continuation(*byte))
        {
            end -= 1;
        }
        let bytes = self.kept.get(start..end).unwrap_or_default();
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(None);
        };
        let first = self.offset + start as u64;
        let last = first + bytes.len() as u64;
        Ok(Some(Page {
            text: text.to_owned(),
            offset: first,
            next_offset: (last < self.seen).then_some(last),
        }))
    }

    /// Checks `bytes`, which follow what came before, as text.
    fn check(&mut self, mut bytes: &[u8]) {
        if !self.text {
            return;
        }
        if bytes.contains(&0) {
            self.text = false;
            return;
        }
        if let Some(&lead) = self.carry.first() {
            let full = char_len(lead);
            let take = full.saturating_sub(self.carry.len()).min(bytes.len());
            let (head, rest) = bytes.split_at(take);
            self.carry.extend_from_slice(head);
            bytes = rest;
            if self.carry.len() < full {
                return;
            }
            if std::str::from_utf8(&self.carry).is_err() {
                self.text = false;
                return;
            }
            self.carry.clear();
        }
        match std::str::from_utf8(bytes) {
            Ok(_) => {}
            Err(error) if error.error_len().is_none() => {
                let (_, tail) = bytes.split_at(error.valid_up_to());
                self.carry.extend_from_slice(tail);
            }
            Err(_not_text) => self.text = false,
        }
    }

    /// Keeps what of `bytes` (at `self.seen`) falls in the page's reach.
    fn keep(&mut self, bytes: &[u8]) {
        let reach_end = self.offset.saturating_add(PAGE_BYTES as u64 + 3);
        let at = self.seen;
        let end = at + bytes.len() as u64;
        let from = self.offset.max(at);
        let to = reach_end.min(end);
        if from < to {
            let (from, to) = (usize::try_from(from - at), usize::try_from(to - at));
            if let (Ok(from), Ok(to)) = (from, to)
                && let Some(part) = bytes.get(from..to)
            {
                self.kept.extend_from_slice(part);
            }
        }
    }
}

impl Write for Window {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.check(buf);
        self.keep(buf);
        self.seen += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The asked offset is past the file's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PastEnd;

const fn is_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

/// How many bytes the character led by `lead` takes; 1 for a byte that leads none, which
/// `from_utf8` then refuses.
const fn char_len(lead: u8) -> usize {
    match lead {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    }
}
