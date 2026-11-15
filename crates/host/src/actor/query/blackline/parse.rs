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
//! - how tables nest: a table inside a cell is a `t` line inside the outer table, tagged `{+}`
//!   when it is not the cell's first block, with its own rows and its own `{/table}`. Each
//!   `{/table}` closes the innermost open table. A cell's paragraphs are paragraphs of the
//!   blackline, in order, at any depth up to [`MAX_TABLE_DEPTH`]; past it a table's lines are
//!   left out;
//! - what each paragraph reads as (`line.rs`);
//! - what is not shown: an `{opaque …}` line (content stetdoc collapses, such as a table of
//!   contents) is left out; and when stetdoc's summary counts more tracked changes than the
//!   body's lines name (`count.rs`), the others are in headers, footers, notes or comments,
//!   which the view does not mark, so the blackline says so
//!   ([`BlacklineNotice::ChangesOutsideText`]);
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

use clauseo_protocol::{BlacklineNotice, BlacklineParagraph, ParagraphChange, WordBlackline};
use serde::Deserialize;
use serde::de::IgnoredAny;

use line::{Seen, marker, paragraph};

mod count;
mod line;

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
    if text.seen.unknown {
        found.push(BlacklineNotice::MayDiffer);
    }
    let truncated = view.position.shown < view.position.total || view.next.is_some();
    // stetdoc's count covers every story; a page cut short shows fewer of the body's too.
    if !truncated && text.counted > text.ids.len() {
        found.push(BlacklineNotice::ChangesOutsideText);
    }
    let warnings: Vec<String> = view.warnings.iter().map(|w| code_word(&w.code)).collect();
    found.extend(warnings.iter().filter_map(|code| notice_of(code)));
    let notices = BlacklineNotice::ALL
        .into_iter()
        .filter(|notice| found.contains(notice))
        .collect();
    Ok(Read {
        blackline: WordBlackline {
            paragraphs: text.paragraphs,
            truncated,
            notices,
        },
        skipped: text.skipped,
        first_skip: text.first_skip,
        warnings,
    })
}

/// Whether the view's text starts with a `stet 1` header (`{stet 1 view=body … mode=redline}`),
/// blank lines before it aside.
fn stet_1(text: &str) -> bool {
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

/// A redline view's text read: its paragraphs, what was left out, and what was seen.
#[derive(Debug, Default)]
pub(crate) struct Text {
    /// The paragraphs, in order.
    pub(crate) paragraphs: Vec<BlacklineParagraph>,
    /// How many lines were left out because they broke a rule.
    pub(crate) skipped: usize,
    /// The first of them.
    pub(crate) first_skip: Option<Unreadable>,
    /// Whether an `{opaque …}` line left content out.
    pub(crate) opaque: bool,
    /// What the paragraphs held that the lawyer should know of.
    pub(crate) seen: Seen,
    /// How many tracked changes stetdoc's summary counts, in every story.
    pub(crate) counted: usize,
    /// The ids of the changes the body's lines name (`count.rs`).
    pub(crate) ids: BTreeSet<u64>,
    /// Whether the last block line was left out, so the next one left out shares its place.
    last_skipped: bool,
}

impl Text {
    /// Leaves out the line `at`, with a paragraph `not_shown` in its place; lines left out one
    /// after another share one.
    fn skip(&mut self, at: Unreadable) {
        self.skipped += 1;
        self.first_skip.get_or_insert(at);
        if !self.last_skipped {
            self.paragraphs.push(BlacklineParagraph {
                change: ParagraphChange::NotShown,
                runs: Vec::new(),
                hidden: false,
            });
        }
        self.last_skipped = true;
    }
}

/// The view's own lines this reader knows, by how they start: the head block's, the end of a
/// table, a section's end, the position line and collapsed content (`{opaque …}`).
const STRUCTURE: [&str; 9] = [
    "{stet ",
    "{rules:",
    "{authors:",
    "{summary:",
    "{warning ",
    "{section ",
    "{position:",
    "{opaque ",
    "{/table}",
];

/// The head block's lines, whose words are stetdoc's, not the document's changes.
const HEAD: [&str; 5] = ["{stet ", "{rules:", "{authors:", "{summary:", "{warning "];

/// The paragraphs of a redline view's text, in order, and what was left out.
pub(crate) fn read_text(text: &str) -> Text {
    let mut read = Text::default();
    // How many tables are open: 0 outside any, 1 in a top-level table's rows.
    let mut depth = 0_usize;
    for (number, line) in logical_lines(text) {
        let line = line.as_str();
        let at = |why| Unreadable { line: number, why };
        if line.is_empty() {
            continue;
        }
        if line == "{appendix}" {
            break;
        }
        if let Some(counted) = count::summary_count(line) {
            read.counted = counted;
            continue;
        }
        if !HEAD.iter().any(|head| line.starts_with(head)) {
            count::ids_in(line, &mut read.ids);
        }
        if line == "{/table}" {
            match depth.checked_sub(1) {
                Some(outer) => depth = outer,
                None => read.skip(at("a table end with no table open")),
            }
            continue;
        }
        if line.starts_with("{opaque ") {
            read.opaque = true;
            continue;
        }
        if line.starts_with('{') {
            // A line of the view's own this reader does not know: a newer stetdoc's, noted.
            if !STRUCTURE.iter().any(|known| line.starts_with(known)) {
                read.seen.unknown = true;
            }
            continue;
        }
        let Some((letters, content)) = block_ref(line) else {
            read.skip(at("a line with no block ref"));
            continue;
        };
        if letters == "t" {
            // A `t` line opens a table whatever its markup, so its `{/table}` still closes it.
            depth = depth.saturating_add(1);
            if depth > MAX_TABLE_DEPTH {
                read.skip(at("a table nested past the depth cap"));
            } else {
                match table(content) {
                    Ok(formatted) => read.seen.unknown |= formatted,
                    Err(why) => read.skip(at(why)),
                }
            }
            continue;
        }
        if depth > MAX_TABLE_DEPTH {
            read.skip(at("a block of a table nested past the depth cap"));
            continue;
        }
        match letters {
            "p" => {
                // What the paragraph held counts only once it reads.
                let mut seen = read.seen;
                match paragraph(content, &mut seen) {
                    Ok(paragraph) => {
                        read.seen = seen;
                        read.paragraphs.push(paragraph);
                        read.last_skipped = false;
                    }
                    Err(why) => read.skip(at(why)),
                }
            }
            "r" if depth == 0 => read.skip(at("a row outside a table")),
            // A row holds no text; a row marker a newer stetdoc adds is noted, not guessed at.
            "r" => match marker(content) {
                Ok((body, _)) if body == "row" || body.starts_with("row ") => {}
                Ok(_) => read.seen.unknown = true,
                Err(_) => read.skip(at("a table or row line without its marker")),
            },
            _ => read.skip(at("a block of a kind the reader does not know")),
        }
    }
    read
}

/// The view's lines, each with the number of its first physical line (from 1): a line that ends
/// in an odd number of backslashes has its last one taken off and the next line joined on.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
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
/// cell, or the cell's own tags when it is the first), a format change (`{rev table #3 C}`),
/// then `{table …}`. Whether it has a format change, which no paragraph shows, so the caller
/// says the blackline may differ.
fn table(content: &str) -> Result<bool, &'static str> {
    let mut rest = content;
    let mut formatted = false;
    while !rest.starts_with("{table") {
        let (body, after) =
            marker(rest).map_err(|_no_marker| "a table or row line without its marker")?;
        if body.starts_with("rev ") {
            formatted = true;
        } else if !line::cell_tag(body) {
            return Err("a table or row line without its marker");
        }
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    Ok(formatted)
}

/// A block line's ref letters and its content: `p12 text` is `("p", "text")`, and a bare
/// `p12` has empty content. `None` when the line does not start with a ref.
fn block_ref(line: &str) -> Option<(&str, &str)> {
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
