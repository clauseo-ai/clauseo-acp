//! Reads what `stetdoc view --story notes --all --json` and `--story headers` print for a
//! redline into the blackline's parts ([`BlacklinePart`]): pure, no I/O. The format was taken
//! from stetdoc 0.9.3's output (`tests/fixtures/blackline/partial/story-*.json`) and its source
//! (`render_structure.rs`, `unit_line`).
//!
//! It answers:
//! - where a part starts: a line of the view's own that names it, and every line up to the
//!   next such line is its own:
//!   - `{footnote 3}` and `{endnote 1}`, the note's number, with ` mark=<n>` after it when Word
//!     shows another number in that view (the number read is the note's own);
//!   - `{header default sections=1,2}`, `{footer first sections=2}`: a header or footer, then for
//!     each kind of page it is on (`default`, `first`, `even`), the sections; one part may be on
//!     several kinds (`{header default sections=1 first sections=2}`), or on none (`{header}`);
//! - what a part's lines read as: the body's rules (`text.rs`, through `line.rs`), tables and
//!   all;
//! - which parts the blackline holds: those that show a change (a paragraph not `same`, or a
//!   line left out); the others are left out ([`StoryRead::parts`]);
//! - what is not shown: a line that names a part in a form the reader does not know, and every
//!   line up to the next part, is left out with [`BlacklineNotice::PartNotShown`] (its changes'
//!   ids are not counted as shown, so stetdoc's list of changes names them as outside what is
//!   shown); and so is a block line before the first part;
//! - what else the lawyer is told: the body's rules again (a line left out, a change a line
//!   names that no paragraph shows, a marker the reader does not know, a warning by its code),
//!   and hidden text in a part that is shown.
//!
//! [`add_parts`] puts both views' parts into the blackline in their order: the footnotes by
//! number, the endnotes by number, then the headers and the footers, each by the first section
//! it is on.

use std::collections::BTreeSet;

use clauseo_protocol::{
    BlacklineNotice, BlacklineParagraph, BlacklinePart, BlacklinePartKind, HeaderSlot, HeaderUse,
    ParagraphChange, RunKind,
};

use super::super::tables::squeezed;
use super::text::read_lines;
use super::{
    Read, Unreadable, View, ViewFailure, code_word, logical_lines, notice_of, order, stet_1,
};

/// Which of the two story views a read is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Story {
    /// `--story notes`: the footnotes and endnotes.
    Notes,
    /// `--story headers`: the headers and footers.
    Headers,
}

impl Story {
    /// The view's own word for it, as `--story` takes it and the header line names it.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Notes => "notes",
            Self::Headers => "headers",
        }
    }
}

/// One story view read: the parts that show a change, the ids its lines name, its notices, and
/// for the log what was left out and what stetdoc warned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StoryRead {
    /// The parts that show a change, in the view's order.
    pub(crate) parts: Vec<BlacklinePart>,
    /// The ids of the changes the lines of the parts it read name.
    pub(crate) ids: BTreeSet<u64>,
    /// What the lawyer should know.
    pub(crate) notices: Vec<BlacklineNotice>,
    /// How many lines were left out.
    pub(crate) skipped: usize,
    /// stetdoc's warnings by code alone.
    pub(crate) warnings: Vec<String>,
    /// The older and the newer text of each header and footer kept in `parts` ([`part_texts`]).
    pub(crate) header_texts: Vec<String>,
}

/// The parts in `json`, the stdout of `stetdoc view --story <story> --all --json` on a redline.
pub(crate) fn read_story(json: &str, story: Story) -> Result<StoryRead, ViewFailure> {
    let json_error = |why| ViewFailure::NotAView(Unreadable { line: 0, why });
    let view: View =
        serde_json::from_str(json).map_err(|_bad| json_error("the JSON is not a view"))?;
    if view.mode != "redline" {
        return Err(json_error("the view is not a redline"));
    }
    if !stet_1(&view.text) {
        return Err(ViewFailure::FormatUnknown);
    }
    let named = format!("view={}", story.word());
    let header = view.text.lines().find(|line| !line.is_empty());
    if !header.is_some_and(|line| line.split([' ', '}']).any(|word| word == named)) {
        return Err(json_error("the view is not of the story asked for"));
    }
    let mut read = StoryRead::default();
    let mut found = Vec::new();
    // The part being read, and its lines.
    let mut open: Option<(Opened, Lines)> = None;
    for (number, line) in logical_lines(&view.text) {
        if line == "{appendix}" {
            break;
        }
        if let Some(kind) = opener(&line) {
            if let Some((kind, lines)) = open.take() {
                close(&mut read, &mut found, kind, lines);
            }
            open = Some((kind, Vec::new()));
            continue;
        }
        match open.as_mut() {
            Some((_, lines)) => lines.push((number, line)),
            // The head's lines are the view's own; a block line before the first part is in
            // none.
            None if line.is_empty() || line.starts_with('{') => {}
            None => {
                read.skipped += 1;
                found.push(BlacklineNotice::PartNotShown);
            }
        }
    }
    if let Some((kind, lines)) = open.take() {
        close(&mut read, &mut found, kind, lines);
    }
    if view.position.shown < view.position.total || view.next.is_some() {
        found.push(BlacklineNotice::PartNotShown);
    }
    read.warnings = view.warnings.iter().map(|w| code_word(&w.code)).collect();
    found.extend(read.warnings.iter().filter_map(|code| notice_of(code)));
    order(&mut found);
    read.notices = found;
    Ok(read)
}

/// A part's lines, each with its number.
type Lines = Vec<(usize, String)>;

/// What a line that opens a part names.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Opened {
    /// A part the reader knows.
    Part(BlacklinePartKind),
    /// A part named in a form the reader does not know.
    Unreadable,
}

/// Reads one part's `lines` into `read` (and its notices into `found`): kept when it shows a
/// change; one whose line did not read is left out, not shown.
fn close(read: &mut StoryRead, found: &mut Vec<BlacklineNotice>, opened: Opened, lines: Lines) {
    let Opened::Part(kind) = opened else {
        read.skipped += lines.len();
        found.push(BlacklineNotice::PartNotShown);
        return;
    };
    let text = read_lines(lines);
    read.skipped += text.skipped;
    if text.skipped > 0 || text.opaque {
        found.push(BlacklineNotice::PartNotShown);
    }
    if text.seen.unknown || !text.ids.is_subset(&text.covered) {
        found.push(BlacklineNotice::MayDiffer);
    }
    read.ids.extend(text.ids.iter().copied());
    let changed = text
        .paragraphs
        .iter()
        .any(|paragraph| paragraph.change != ParagraphChange::Same);
    if changed {
        if text.seen.hidden {
            found.push(BlacklineNotice::HiddenText);
        }
        if matches!(
            kind,
            BlacklinePartKind::Header { .. } | BlacklinePartKind::Footer { .. }
        ) {
            let (older, newer) = part_texts(&text.paragraphs, &text.labels);
            read.header_texts.push(older);
            read.header_texts.push(newer);
        }
        read.parts.push(BlacklinePart {
            kind,
            paragraphs: text.paragraphs,
        });
    }
}

/// The older and the newer text of a part's `paragraphs` as its `w:t` holds it: white space
/// taken out, and each list number in `labels` left out (a paragraph's index and the number's
/// length in bytes from the start of its runs), since the view writes the number into the runs
/// and the part's own text does not hold it. Nothing else is left out.
pub(crate) fn part_texts(
    paragraphs: &[BlacklineParagraph],
    labels: &[(usize, usize)],
) -> (String, String) {
    let (mut older, mut newer) = (String::new(), String::new());
    let mut labels = labels.iter().peekable();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let mut skip = 0;
        while let Some((at, len)) = labels.next_if(|(at, _)| *at <= index) {
            if *at == index {
                skip = *len;
            }
        }
        for run in &paragraph.runs {
            let cut = skip.min(run.text.len());
            skip -= cut;
            let rest = run.text.get(cut..).unwrap_or_default();
            if matches!(run.kind, RunKind::Same | RunKind::Del) {
                older.push_str(rest);
            }
            if matches!(run.kind, RunKind::Same | RunKind::Ins) {
                newer.push_str(rest);
            }
        }
    }
    (squeezed(&older), squeezed(&newer))
}

/// The part a line names, or `None` when the line names no part.
fn opener(line: &str) -> Option<Opened> {
    let inner = line.strip_prefix('{')?.strip_suffix('}')?;
    let mut words = inner.split(' ');
    let kind = words.next()?;
    let rest: Vec<&str> = words.collect();
    let part = match kind {
        "footnote" | "endnote" => note(kind, &rest),
        "header" | "footer" => uses(&rest).map(|uses| {
            if kind == "header" {
                BlacklinePartKind::Header { uses }
            } else {
                BlacklinePartKind::Footer { uses }
            }
        }),
        _ => return None,
    };
    Some(part.map_or(Opened::Unreadable, Opened::Part))
}

/// A note's kind from `{footnote 3}` or `{endnote 1 mark=2}`, its words after the first.
fn note(kind: &str, words: &[&str]) -> Option<BlacklinePartKind> {
    let (number, rest) = words.split_first()?;
    let number = number_of(number)?;
    let marked = match rest {
        [] => true,
        [mark] => mark.strip_prefix("mark=").and_then(number_of).is_some(),
        _ => false,
    };
    if !marked {
        return None;
    }
    Some(if kind == "footnote" {
        BlacklinePartKind::Footnote { number }
    } else {
        BlacklinePartKind::Endnote { number }
    })
}

/// A header's or footer's uses from its words after the first: pairs of a slot and
/// `sections=<n>,<n>`; none for none.
fn uses(words: &[&str]) -> Option<Vec<HeaderUse>> {
    let mut uses = Vec::new();
    for pair in words.chunks(2) {
        let [slot, sections] = pair else {
            return None;
        };
        let slot = match *slot {
            "default" => HeaderSlot::Default,
            "first" => HeaderSlot::FirstPage,
            "even" => HeaderSlot::EvenPages,
            _ => return None,
        };
        let sections = sections
            .strip_prefix("sections=")?
            .split(',')
            .map(number_of)
            .collect::<Option<Vec<u32>>>()?;
        uses.push(HeaderUse { slot, sections });
    }
    Some(uses)
}

/// A number of the view's: digits only, from 1.
fn number_of(word: &str) -> Option<u32> {
    if word.is_empty() || !word.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    word.parse().ok().filter(|&n| n > 0)
}

/// Puts the parts of `stories` into `read`'s blackline in their order, with their notices,
/// counts their ids as shown, and says again whether stetdoc's summary counts more changes
/// than the blackline shows (`ChangesOutsideText`, until stetdoc's list replaces it).
pub(crate) fn add_parts(read: &mut Read, stories: Vec<StoryRead>) {
    let blackline = &mut read.blackline;
    for story in stories {
        blackline.parts.extend(story.parts);
        blackline.notices.extend(story.notices);
        read.ids.extend(story.ids);
        read.header_texts.extend(story.header_texts);
        read.skipped += story.skipped;
        read.warnings.extend(story.warnings);
    }
    blackline.parts.sort_by_key(|part| rank(&part.kind));
    let notices = &mut blackline.notices;
    notices.retain(|notice| *notice != BlacklineNotice::ChangesOutsideText);
    if !blackline.truncated && read.counted > read.ids.len() {
        notices.push(BlacklineNotice::ChangesOutsideText);
    }
    order(notices);
}

/// Where a part goes: footnotes, endnotes, headers, footers, each by number or first section.
fn rank(kind: &BlacklinePartKind) -> (u8, u32) {
    let first = |uses: &[HeaderUse]| {
        uses.iter()
            .flat_map(|used| used.sections.iter().copied())
            .min()
            .unwrap_or(u32::MAX)
    };
    match kind {
        BlacklinePartKind::Footnote { number } => (0, *number),
        BlacklinePartKind::Endnote { number } => (1, *number),
        BlacklinePartKind::Header { uses } => (2, first(uses)),
        BlacklinePartKind::Footer { uses } => (3, first(uses)),
        BlacklinePartKind::Unknown { .. } => (4, 0),
    }
}

#[cfg(test)]
mod tests;
