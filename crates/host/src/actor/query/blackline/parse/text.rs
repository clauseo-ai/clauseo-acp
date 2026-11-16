//! The body's lines of a redline view read into paragraphs ([`read_text`]): the rules of
//! `parse.rs`'s contract for blocks, tables, rows, cells and the view's own lines, applied line
//! by line. Pure, no I/O.

use std::collections::BTreeSet;

use clauseo_protocol::{BlacklineParagraph, ParagraphChange};

use super::line::{self, Mark, Seen, marker, paragraph};
use super::{MAX_TABLE_DEPTH, Unreadable, block_ref, count, logical_lines, table};

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
    /// The ids of the changes a paragraph shows as changed (its line's, and those of the rows and
    /// cells that mark it), or a placeholder stands for: every id in `ids` should be here.
    pub(crate) covered: BTreeSet<u64>,
    /// Whether the view has a summary line, whose count the ids are checked against.
    pub(crate) summary_seen: bool,
    /// The ids the line being read names.
    line_ids: BTreeSet<u64>,
    /// Whether the last block line was left out, so the next one left out shares its place.
    last_skipped: bool,
}

impl Text {
    /// Leaves out the line `at`, with a paragraph `not_shown` in its place; lines left out one
    /// after another share one.
    fn skip(&mut self, at: Unreadable) {
        self.skipped += 1;
        self.first_skip.get_or_insert(at);
        self.covered.extend(self.line_ids.iter().copied());
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
    // The open tables, outermost first, each with its current row's and cell's change, to the
    // depth cap; `past` counts the tables open past it.
    let mut levels: Vec<Level> = Vec::new();
    let mut past = 0_usize;
    for (number, line) in logical_lines(text) {
        let line = line.as_str();
        let at = |why| Unreadable { line: number, why };
        if line.is_empty() {
            continue;
        }
        if line == "{appendix}" {
            break;
        }
        match count::summary_count(line) {
            count::Summary::Count(counted) => {
                read.counted = counted;
                read.summary_seen = true;
                continue;
            }
            // A summary whose count this reader can't read: the check can't be made, so say so.
            count::Summary::Unreadable => {
                read.seen.unknown = true;
                read.summary_seen = true;
                continue;
            }
            count::Summary::Other => {}
        }
        read.line_ids.clear();
        if !HEAD.iter().any(|head| line.starts_with(head)) {
            read.seen.unknown |= count::ids_in(line, &mut read.line_ids);
            read.ids.extend(read.line_ids.iter().copied());
        }
        if line == "{/table}" {
            if past > 0 {
                past -= 1;
            } else if levels.pop().is_none() {
                read.skip(at("a table end with no table open"));
            }
            continue;
        }
        if line.starts_with("{opaque ") {
            read.opaque = true;
            continue;
        }
        if line.starts_with('{') {
            // A line of the view's own this reader does not know (a newer stetdoc's), and a
            // section's tracked change, which no paragraph shows, are noted: its mark
            // (`{section 1 ends: next-page mark inserted #3 C peer -}`) or a format change such as
            // its page size or margins (`{section 1 ends: next-page rev section #4 C}`).
            let section_mark = line.starts_with("{section ")
                && line
                    .split([' ', '}'])
                    .any(|word| word == "mark" || word == "rev");
            if section_mark || !STRUCTURE.iter().any(|known| line.starts_with(known)) {
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
            if past > 0 || levels.len() >= MAX_TABLE_DEPTH {
                past += 1;
                read.skip(at("a table nested past the depth cap"));
                continue;
            }
            // A table that starts a cell carries the cell's tags.
            enter_cell(&mut levels, content, &read.line_ids, &mut read.seen);
            levels.push(Level::default());
            match table(content) {
                Ok(formatted) => read.seen.unknown |= formatted,
                Err(why) => read.skip(at(why)),
            }
            continue;
        }
        if past > 0 {
            read.skip(at("a block of a table nested past the depth cap"));
            continue;
        }
        match letters {
            "p" => {
                // What the paragraph held counts only once it reads.
                let mut seen = read.seen;
                enter_cell(&mut levels, content, &read.line_ids, &mut seen);
                match paragraph(content, &mut seen, whole(&levels)) {
                    Ok(paragraph) => {
                        read.seen = seen;
                        if paragraph.change != ParagraphChange::Same {
                            read.covered.extend(read.line_ids.iter().copied());
                            for level in &levels {
                                read.covered.extend(level.row_ids.iter().copied());
                                read.covered.extend(level.cell_ids.iter().copied());
                            }
                        }
                        read.paragraphs.push(paragraph);
                        read.last_skipped = false;
                    }
                    Err(why) => read.skip(at(why)),
                }
            }
            // A row holds no text, but its change marks its paragraphs; a row word a newer
            // stetdoc adds, and a row's format change after its marker, are noted.
            "r" => match (levels.last_mut(), marker(content)) {
                (None, _) => read.skip(at("a row outside a table")),
                (Some(level), Ok((body, after))) => {
                    let (mark, unknown) = line::row_of(body);
                    *level = Level {
                        row: mark,
                        cell: Mark::None,
                        row_ids: marked_ids(mark, &read.line_ids),
                        cell_ids: Vec::new(),
                    };
                    read.seen.unknown |= unknown || !after.trim().is_empty();
                }
                (Some(_), Err(_)) => read.skip(at("a table or row line without its marker")),
            },
            _ => read.skip(at("a block of a kind the reader does not know")),
        }
    }
    read
}

/// One open table: the change of its current row and of its current cell, with the ids of the
/// lines that marked them (none when unmarked).
#[derive(Debug, Clone)]
struct Level {
    row: Mark,
    cell: Mark,
    row_ids: Vec<u64>,
    cell_ids: Vec<u64>,
}

impl Default for Level {
    fn default() -> Self {
        Self {
            row: Mark::None,
            cell: Mark::None,
            row_ids: Vec::new(),
            cell_ids: Vec::new(),
        }
    }
}

/// `ids` when `mark` is a change, else none.
fn marked_ids(mark: Mark, ids: &BTreeSet<u64>) -> Vec<u64> {
    if mark == Mark::None {
        Vec::new()
    } else {
        ids.iter().copied().collect()
    }
}

/// A block line inside a table: unless it goes on its cell (`{+}`), it starts a new cell, whose
/// change its tags give; a cell tag this reader does not know is noted in `seen`.
fn enter_cell(levels: &mut [Level], content: &str, ids: &BTreeSet<u64>, seen: &mut Seen) {
    let Some(level) = levels.last_mut() else {
        return;
    };
    let tags = line::cell_tags(content);
    if !tags.continues {
        level.cell = tags.mark;
        level.cell_ids = marked_ids(tags.mark, ids);
    }
    seen.unknown |= tags.unknown;
}

/// The change a paragraph takes from the rows and cells it stands in: deleted when any of them
/// is deleted (its content is gone in the newer version, whatever was inserted inside it), else
/// inserted when any is inserted; `None` when none is marked.
fn whole(levels: &[Level]) -> Mark {
    let marks = || levels.iter().flat_map(|level| [level.cell, level.row]);
    if marks().any(|mark| mark == Mark::Deleted) {
        Mark::Deleted
    } else if marks().any(|mark| mark == Mark::Inserted) {
        Mark::Inserted
    } else {
        Mark::None
    }
}
