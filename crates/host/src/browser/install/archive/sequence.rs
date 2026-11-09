//! The local headers in order: from offset 0, each local header, its data and its data
//! descriptor, then the next, up to the central directory. Each must be one the central
//! directory names, and the walk must meet every one it names and end exactly where the
//! directory starts. So an orphan local entry (one the directory does not list, which a tool
//! reading local headers would unpack) or a directory entry pointing into another entry's data
//! refuses the zip.

use std::collections::BTreeMap;
use std::fs::File;

use super::{LOCAL_SIG, bad, read_at, u16_at, u32_at};
use crate::browser::install::Failure;

/// A data descriptor's optional signature.
const DESCRIPTOR_SIG: u32 = 0x0807_4b50;

/// General purpose flag bit 3: the sizes follow the data, in a data descriptor.
const HAS_DESCRIPTOR: u16 = 1 << 3;

/// What the central directory says of one entry's local span.
struct Span {
    /// Its compressed size.
    packed: u64,
    /// Its flags.
    flags: u16,
    /// Whether its sizes are zip64 (a descriptor's sizes are then 8 bytes each).
    zip64: bool,
}

/// The local spans the central directory names, by offset.
pub(super) struct Spans(BTreeMap<u64, Span>);

impl Spans {
    pub(super) fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// One directory entry's local header at `offset`; two entries at one offset are refused.
    pub(super) fn add(
        &mut self,
        offset: u64,
        packed: u64,
        flags: u16,
        zip64: bool,
    ) -> Result<(), Failure> {
        let span = Span {
            packed,
            flags,
            zip64,
        };
        if self.0.insert(offset, span).is_some() {
            return Err(bad("two directory entries name one local header"));
        }
        Ok(())
    }

    /// Walks the local headers from offset 0 to `directory`, the central directory's offset.
    pub(super) fn check(&self, file: &mut File, directory: u64) -> Result<(), Failure> {
        let mut at = 0;
        let mut met = 0;
        while at < directory {
            let span = self.0.get(&at).ok_or_else(|| {
                bad("the zip holds a local entry its central directory does not list")
            })?;
            let header = read_at(file, at, 30)?;
            if u32_at(&header, 0)? != LOCAL_SIG {
                return Err(bad("a local header is missing"));
            }
            let names = u64::from(u16_at(&header, 26)?) + u64::from(u16_at(&header, 28)?);
            let mut next = at
                .checked_add(30 + names)
                .and_then(|end| end.checked_add(span.packed))
                .ok_or_else(|| bad("a local entry runs past the zip"))?;
            if span.flags & HAS_DESCRIPTOR != 0 {
                let signed =
                    next + 4 <= directory && u32_at(&read_at(file, next, 4)?, 0)? == DESCRIPTOR_SIG;
                let sizes = if span.zip64 { 16 } else { 8 };
                next += u64::from(signed) * 4 + 4 + sizes;
            }
            at = next;
            met += 1;
        }
        if at != directory {
            return Err(bad("a local entry runs into the central directory"));
        }
        if met != self.0.len() {
            return Err(bad(
                "a central directory entry names no local header in sequence",
            ));
        }
        Ok(())
    }
}
