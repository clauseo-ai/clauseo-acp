//! Reads what `stetdoc view --all --json` prints for a redline into a [`WordBlackline`]: pure,
//! no I/O. The format was taken from stetdoc 0.9.0's and 0.9.3's output (the release pin is
//! v0.9.3), from real samples (`tests/fixtures/blackline/`), and from stetdoc's design notes on
//! tables (`{+}` and the other cell tags), not from a written grammar.
//!
//! It answers:
//! - which format the view is in: its first line, the header, must be `{stet 1 …}`; another id
//!   (a `stet 2` from a newer stetdoc) or no header gives no blackline
//!   ([`ViewFailure::FormatUnknown`]), since its lines may mean something else;
//! - what a line is: a physical line that ends in an odd number of backslashes goes on in the
//!   next (stetdoc wraps a paragraph past about 1,500 characters so), and the two are one line;
//! - which lines are blocks: `<ref>` or `<ref> <content>`, where a `p` ref is a paragraph (a
//!   table cell's included), a `t` ref opens a table (`{table cols=2}`) and an `r` ref a row
//!   (`{row}`, `{row inserted #16 C}`; another row marker is noted as unknown); a line that
//!   starts with `{` is the view's own (`{stet ...}`, `{summary: ...}`, `{warning ...}`,
//!   `{section 1 ends: ...}`, `{position: ...}`), and everything after `{appendix}` (the
//!   notes and comments the page cites) is skipped;
//! - what a tracked row or cell is: `{row inserted #11 R}` or `{row deleted …}` on a row,
//!   and `{cell inserted …}` or `{cell deleted …}` on a cell's first block, whose paragraphs
//!   show no change of their own: each of them is `added` (all its text inserted) or `removed`
//!   (all deleted), in nested tables too; another row or cell word (`{cell merged …}`) gives
//!   [`BlacklineNotice::MayDiffer`];
//! - how tables nest: a table inside a cell is a `t` line inside the outer table, tagged `{+}`
//!   when it is not the cell's first block, with its own rows and its own `{/table}`. Each
//!   `{/table}` closes the innermost open table. A cell's paragraphs are paragraphs of the
//!   blackline, in order, at any depth up to [`MAX_TABLE_DEPTH`]; past it a table's lines are
//!   left out;
//! - what each paragraph reads as (`line.rs`);
//! - what is not shown: an `{opaque …}` line (content stetdoc collapses, such as a table of
//!   contents) is left out; and when stetdoc's summary counts more tracked changes than the
//!   body's lines name (`count.rs`), the others are in headers, footers, notes or comments,
//!   which the body's view does not mark, so the blackline says so
//!   ([`BlacklineNotice::ChangesOutsideText`]) until the notes and headers are read
//!   (`story.rs`, which counts their lines' ids too);
//! - what a line of the view's own that the reader does not know means: nothing it can show, so
//!   it is skipped with [`BlacklineNotice::MayDiffer`], as a table's format change is.
//!
//! A line that breaks a rule (any other `{…}` not escaped, any other escape, any other ref, a
//! row or `{/table}` outside a table) is left out, never guessed at: a paragraph `not_shown`
//! stands in its place (one for lines left out one after another), the rest of the view is read,
//! and the blackline says part of it is not shown ([`BlacklineNotice::PartNotShown`]). Only a
//! view that is not a redline, or one with lines and none of them readable, gives no blackline
//! ([`ViewFailure`]). A marker a newer stetdoc adds is left out with
//! [`BlacklineNotice::MayDiffer`] (`line.rs`). stetdoc's `warnings` become notices by their
//! codes ([`notice_of`]); the log gets the counts and codes, never a line's text or a warning's
//! message.

use std::fmt;

use std::collections::BTreeSet;

use clauseo_protocol::{BlacklineNotice, ParagraphChange, WordBlackline};
use serde::Deserialize;
use serde::de::IgnoredAny;

use line::marker;
pub(crate) use text::read_text;

mod count;
pub(crate) mod ledger;
mod line;
pub(crate) mod receipt;
pub(crate) mod sides;
pub(crate) mod story;
mod text;

/// The deepest table the reader shows, counting a top-level table as 1. stetdoc reads no XML
/// deeper than 256 elements and each level of a table is three (`w:tbl`, `w:tr`, `w:tc`), so it
/// prints at most 85 levels; a view past this cap is not stetdoc's, and its too-deep tables are
/// left out with [`BlacklineNotice::PartNotShown`].
pub(crate) const MAX_TABLE_DEPTH: usize = 96;

/// Why a line of the view could not be read: its line (0 for the JSON around it) and the rule
/// it broke. It holds no document text, so it can go in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unreadable {
    /// The line of the view's text, from 1; 0 for the JSON around it.
    pub(crate) line: usize,
    /// The rule the line broke.
    pub(crate) why: &'static str,
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.why)
    }
}

/// Why a view gives no blackline at all: nothing in it can be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewFailure {
    /// The JSON is not a redline view (`line` is 0).
    NotAView(Unreadable),
    /// The view's text is in a format other than `stet 1`, or has no header.
    FormatUnknown,
    /// The view has block lines and not one of them could be read; the first one's rule.
    NothingRead {
        /// How many lines were left out.
        skipped: usize,
        /// The first of them.
        first: Unreadable,
    },
}

impl fmt::Display for ViewFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAView(unreadable) => write!(f, "its output could not be read at {unreadable}"),
            Self::FormatUnknown => f.write_str("its view is not in the stet 1 format"),
            Self::NothingRead { skipped, first } => {
                write!(
                    f,
                    "none of its {skipped} lines could be read; the first at {first}"
                )
            }
        }
    }
}

/// A view read: the blackline, and for the log what was left out and what stetdoc warned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Read {
    /// What the caller gets.
    pub(crate) blackline: WordBlackline,
    /// How many lines were left out.
    pub(crate) skipped: usize,
    /// The first line left out, if any: its line and rule, no text.
    pub(crate) first_skip: Option<Unreadable>,
    /// stetdoc's warnings by code alone (`other` for a code that is not a plain word).
    pub(crate) warnings: Vec<String>,
    /// The ids of the changes the body's lines name, and the lines of every part the story views
    /// read (`story.rs`; a part read and not shown names ids only when a line names a change no
    /// paragraph shows, which says the blackline may differ), for the check against stetdoc's
    /// own list (`ledger.rs`).
    pub(crate) ids: BTreeSet<u64>,
    /// How many tracked changes stetdoc's summary counts, in every story.
    pub(crate) counted: usize,
}

/// The fields of the view's JSON the read needs; the others are ignored.
#[derive(Deserialize)]
struct View {
    mode: String,
    text: String,
    #[serde(default)]
    next: Option<IgnoredAny>,
    position: Position,
    #[serde(default)]
    warnings: Vec<Warning>,
}

/// One of stetdoc's warnings; only its code is read.
#[derive(Deserialize)]
struct Warning {
    #[serde(default)]
    code: String,
}

/// How much of the document the page shows, in blocks.
#[derive(Deserialize)]
struct Position {
    shown: u64,
    total: u64,
}

/// The blackline in `json`, the stdout of `stetdoc view --all --json` on a redline.
/// `truncated` is stetdoc's own word: a page that shows fewer blocks than there are, or names a
/// next page. A line that does not read is left out, and a warning becomes a notice.
pub(crate) fn read_view(json: &str) -> Result<Read, ViewFailure> {
    let json_error = |why| ViewFailure::NotAView(Unreadable { line: 0, why });
    let view: View =
        serde_json::from_str(json).map_err(|_bad| json_error("the JSON is not a view"))?;
    if view.mode != "redline" {
        return Err(json_error("the view is not a redline"));
    }
    if !stet_1(&view.text) {
        return Err(ViewFailure::FormatUnknown);
    }
    let text = read_text(&view.text);
    let none_shown = text
        .paragraphs
        .iter()
        .all(|p| p.change == ParagraphChange::NotShown);
    if let (true, Some(first)) = (none_shown, text.first_skip) {
        return Err(ViewFailure::NothingRead {
            skipped: text.skipped,
            first,
        });
    }
    let mut found = Vec::new();
    if text.skipped > 0 || text.opaque {
        found.push(BlacklineNotice::PartNotShown);
    }
    if text.seen.hidden {
        found.push(BlacklineNotice::HiddenText);
    }
    // A view with no summary can't be checked; and a change a line names that no paragraph
    // shows as changed (a marker this reader drops, a format it does not model) may be missing.
    let uncovered = !text.ids.is_subset(&text.covered);
    if text.seen.unknown || !text.summary_seen || uncovered {
        found.push(BlacklineNotice::MayDiffer);
    }
    let truncated = view.position.shown < view.position.total || view.next.is_some();
    // stetdoc's count covers every story; a page cut short shows fewer of the body's too.
    if !truncated && text.counted > text.ids.len() {
        found.push(BlacklineNotice::ChangesOutsideText);
    }
    let warnings: Vec<String> = view.warnings.iter().map(|w| code_word(&w.code)).collect();
    found.extend(warnings.iter().filter_map(|code| notice_of(code)));
    order(&mut found);
    Ok(Read {
        blackline: WordBlackline {
            paragraphs: text.paragraphs,
            truncated,
            notices: found,
            changed_links: Vec::new(),
            parts: Vec::new(),
            changed_tables: Vec::new(),
        },
        skipped: text.skipped,
        first_skip: text.first_skip,
        warnings,
        ids: text.ids,
        counted: text.counted,
    })
}

/// Puts `notices` in the order of [`BlacklineNotice::ALL`], each once.
pub(crate) fn order(notices: &mut Vec<BlacklineNotice>) {
    let found = std::mem::take(notices);
    notices.extend(
        BlacklineNotice::ALL
            .into_iter()
            .filter(|notice| found.contains(notice)),
    );
}

/// Whether the view's text starts with a `stet 1` header (`{stet 1 view=body … mode=redline}`),
/// blank lines before it aside.
pub(crate) fn stet_1(text: &str) -> bool {
    let header = text
        .lines()
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let mut words = header.split(' ');
    words.next() == Some("{stet") && matches!(words.next(), Some("1" | "1}"))
}

/// The notice a warning of stetdoc's (by its code) gives, if any. Warnings about what a view
/// does not show give [`BlacklineNotice::PartNotShown`]; about hidden text, `HiddenText`; about
/// numbers, `NumbersMayDiffer`. An edit that found no change or fell at a wrap (`no_change`,
/// `split_at_wrap`), a rewrite hint and a note no reference points at give none. Any other code,
/// `own_text_removed` and `property_change_folded` among them, gives `MayDiffer`: when in doubt,
/// the lawyer is told.
fn notice_of(code: &str) -> Option<BlacklineNotice> {
    match code {
        "read_anomaly" | "opaque_content" => Some(BlacklineNotice::PartNotShown),
        "hidden_text" | "looks_hidden" => Some(BlacklineNotice::HiddenText),
        "numbering_approximate" | "duplicate_number" => Some(BlacklineNotice::NumbersMayDiffer),
        "unreferenced_note" | "looks_like_rewrite" | "split_at_wrap" | "no_change" => None,
        _ => Some(BlacklineNotice::MayDiffer),
    }
}

/// A warning's code as the log may hold it: a short word of stetdoc's, or `other` for anything
/// else, so no document text reaches the log.
fn code_word(code: &str) -> String {
    let word = code.len() <= 64
        && !code.is_empty()
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit());
    if word {
        code.to_owned()
    } else {
        "other".to_owned()
    }
}

/// The view's lines, each with the number of its first physical line (from 1): a line that ends
/// in an odd number of backslashes has its last one taken off and the next line joined on.
pub(crate) fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut lines = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (index, physical) in text.lines().enumerate() {
        let (number, mut line) = open.take().unwrap_or((index + 1, String::new()));
        line.push_str(physical);
        // Counted on the physical line, as the format says: on the joined one, a run of lines
        // of backslashes would cost the square of their number.
        let slashes = physical.bytes().rev().take_while(|&b| b == b'\\').count();
        if slashes % 2 == 1 {
            line.pop();
            open = Some((number, line));
        } else {
            lines.push((number, line));
        }
    }
    // A last line that says it goes on, with nothing after: as it is, its backslash taken off.
    lines.extend(open);
    lines
}

/// Checks a `t` line's content: its cell tags (`{+}` for a table that is a later block of its
/// cell, or the cell's own tags when it is the first), `{table …}`, and format changes
/// (`{rev table #3 C}`, `{rev grid #4 C}`) before it or after. Whether it has a format change
/// (or markup after the table's own that a newer stetdoc adds), which no paragraph shows, so
/// the caller says the blackline may differ.
fn table(content: &str) -> Result<bool, &'static str> {
    let mut rest = content;
    let mut formatted = false;
    let mut opened = false;
    // The cell's tags and format changes come before `{table …}` or after it (stetdoc 0.9.3
    // prints `t1 {table cols=2} {rev table #23 R} {rev grid #24 R}`).
    while !rest.is_empty() {
        let Ok((body, after)) = marker(rest) else {
            // Markup after the table's own that does not read: a newer stetdoc's, noted.
            if opened {
                return Ok(true);
            }
            return Err("a table or row line without its marker");
        };
        if body == "table" || body.starts_with("table ") {
            opened = true;
        } else if body.starts_with("rev ") || opened {
            // A format change, or a word a newer stetdoc adds after the table's own.
            formatted = true;
        } else if !line::cell_tag(body) {
            return Err("a table or row line without its marker");
        }
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    if opened {
        Ok(formatted)
    } else {
        Err("a table or row line without its marker")
    }
}

/// A block line's ref letters and its content: `p12 text` is `("p", "text")`, and a bare
/// `p12` has empty content. `None` when the line does not start with a ref.
pub(crate) fn block_ref(line: &str) -> Option<(&str, &str)> {
    let letters = line.find(|c: char| !c.is_ascii_lowercase())?;
    let after = line.get(letters..)?;
    let digits = after
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after.len());
    if letters == 0 || digits == 0 {
        return None;
    }
    let content = match after.get(digits..)? {
        "" => "",
        rest => rest.strip_prefix(' ')?,
    };
    Some((line.get(..letters)?, content))
}

#[cfg(test)]
mod tests;
