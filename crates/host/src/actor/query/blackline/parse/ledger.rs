//! stetdoc's own list of a redline's tracked changes (`stetdoc check --json`), by id, against the
//! ids the body's view names, for `run.rs`.
//!
//! It answers:
//! - which changes there are: every id of `existing[].changes[].ids`, with its change's `kind`.
//!   The list is counted by id, never by entry or by the ledger's `{changes: N}`: a move is one
//!   entry with two ids, and so is a paragraph whose mark changed with its text;
//! - which changes the blackline shows: those whose ids the body's lines name, or the lines of
//!   a footnote, endnote, header or footer it reads (`story.rs`);
//! - what a change the blackline does not show means, by its kind: a cell's format change
//!   (`cell_format`) is one the view never marks ([`BlacklineNotice::CellFormatChanged`]); a
//!   section's (`section_format`) is the body's, so the blackline may differ
//!   ([`BlacklineNotice::MayDiffer`]); any other is in a comment, or in a part whose view did not
//!   run or read ([`BlacklineNotice::ChangesOutsideText`]), unless the page was cut short and
//!   says so already;
//! - what a change the blackline names and the list does not hold means: the view and the list
//!   disagree, so the blackline may differ.
//!
//! With the list, the summary's count (`count.rs`) is not needed, and its notice is replaced; a
//! check that does not run or read leaves the summary's.

use std::collections::BTreeSet;

use clauseo_protocol::BlacklineNotice;
use serde::Deserialize;

use super::Read;

/// The kind `stetdoc check` gives a table cell's tracked format change.
const CELL_FORMAT: &str = "cell_format";

/// The kind `stetdoc check` gives a section's tracked format change (its page size, margins).
const SECTION_FORMAT: &str = "section_format";

/// The fields of `check --json` the ledger needs; the others are ignored.
#[derive(Deserialize)]
struct Check {
    existing: Vec<ByAuthor>,
}

/// One author's changes.
#[derive(Deserialize)]
struct ByAuthor {
    changes: Vec<Change>,
}

/// One change: its ids and kind.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Change {
    pub(crate) ids: Vec<u64>,
    pub(crate) kind: String,
}

/// The tracked changes `stetdoc check --json` printed (`json`), every author's.
pub(crate) fn read_check(json: &str) -> Result<Vec<Change>, &'static str> {
    let check: Check =
        serde_json::from_str(json).map_err(|_bad| "the check's JSON is not a list of changes")?;
    Ok(check
        .existing
        .into_iter()
        .flat_map(|author| author.changes)
        .collect())
}

/// Checks `read`'s blackline against stetdoc's list of its changes (`changes`): its notice about
/// changes outside the main text is the list's, and a change the body does not name, or names
/// and the list does not hold, gives the notice its kind says (the module doc).
pub(crate) fn apply(read: &mut Read, changes: &[Change]) {
    let notices = &mut read.blackline.notices;
    notices.retain(|notice| *notice != BlacklineNotice::ChangesOutsideText);
    let listed: BTreeSet<u64> = changes
        .iter()
        .flat_map(|change| change.ids.iter().copied())
        .collect();
    if !read.ids.is_subset(&listed) {
        notices.push(BlacklineNotice::MayDiffer);
    }
    for change in changes {
        if change.ids.iter().all(|id| read.ids.contains(id)) {
            continue;
        }
        let notice = match change.kind.as_str() {
            CELL_FORMAT => BlacklineNotice::CellFormatChanged,
            SECTION_FORMAT => BlacklineNotice::MayDiffer,
            // A page cut short names fewer of the body's changes, and says so.
            _ if read.blackline.truncated => continue,
            _ => BlacklineNotice::ChangesOutsideText,
        };
        notices.push(notice);
    }
    super::order(notices);
}

#[cfg(test)]
mod tests;
