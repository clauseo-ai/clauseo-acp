//! Reads the Markdown the docs check needs, by hand: fenced and indented code blocks, inline code
//! spans, links (inline, reference definitions, and HTML `<a>`/`<img>`), with block quotes and
//! list items stripped. No Markdown parser is among our dependencies, so this follows `CommonMark`
//! for those constructs and no further. Tables, footnotes and raw HTML blocks are read as text.

mod containers;
mod definition;
mod inline;

use crate::checks;
use containers::{Containers, Content, strip_quotes};

/// What `scan` found in one Markdown file, outside the constructs the check does not read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Scan {
    /// Inline code spans outside code blocks, in file order. HTML comments are not skipped.
    pub spans: Vec<Span>,
    /// Inline links, HTML link attributes and reference definitions, in file order.
    pub links: Vec<Link>,
    /// The lines of fenced and indented code blocks, in file order, fence lines left out.
    pub code: Vec<Line>,
}

/// An inline code span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Span {
    /// The 1-based line of the opening backticks.
    pub line: u32,
    /// The content, with each line break as a space and the ends trimmed.
    pub text: String,
}

/// A link destination as the file writes it, anchor and all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Link {
    /// The 1-based line of the link's `[`, `<`, or of the definition.
    pub line: u32,
    pub target: String,
}

/// One line of a code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Line {
    /// The 1-based line in the file.
    pub line: u32,
    /// The line as written.
    pub text: String,
}

/// Reads `text`, the contents of one Markdown file.
pub(super) fn scan(text: &str) -> Scan {
    let mut scan = Scan::default();
    let mut block: Vec<(u32, &str)> = Vec::new();
    let mut fence: Option<Fence> = None;
    let mut indented = false;
    let mut containers = Containers::default();
    for (index, line) in text.lines().enumerate() {
        let number = checks::line_at(index);
        if let Some(open) = fence {
            let text = if open.quoted {
                strip_quotes(line)
            } else {
                line
            };
            if open.closes(text) {
                fence = None;
            } else {
                scan.code.push(code_line(number, line));
            }
            continue;
        }
        let Some(content) = containers.strip(line) else {
            if !indented {
                read_block(&block, &mut scan);
                block.clear();
            }
            continue;
        };
        if indented {
            if content.indent >= 4 {
                scan.code.push(code_line(number, line));
                continue;
            }
            indented = false;
        }
        if let Some(opened) = opens_fence(&content) {
            read_block(&block, &mut scan);
            block.clear();
            fence = Some(Fence {
                quoted: strip_quotes(line).len() != line.len(),
                ..opened
            });
        } else if content.indent >= 4 && block.is_empty() {
            indented = true;
            scan.code.push(code_line(number, line));
        } else if content.indent <= 3 && is_heading(content.body) {
            read_block(&block, &mut scan);
            block.clear();
            read_block(&[(number, content.body)], &mut scan);
        } else {
            block.push((number, content.body));
        }
    }
    read_block(&block, &mut scan);
    scan
}

/// The fence that `content` opens: a fence indented at most three spaces past its container.
fn opens_fence(content: &Content<'_>) -> Option<Fence> {
    if content.indent > 3 {
        return None;
    }
    Fence::opens(content.body)
}

/// The line of a code block, as written.
fn code_line(number: u32, line: &str) -> Line {
    Line {
        line: number,
        text: line.to_owned(),
    }
}

/// A fence that opens a fenced code block: its marker and the length of its run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fence {
    /// `` ` `` or `~`.
    marker: u8,
    /// How many markers open the block; the closing run must be at least as long.
    len: usize,
    /// True when the fence opens inside a block quote, so its closing line may carry the quote's
    /// marker too.
    quoted: bool,
}

impl Fence {
    /// The fence `body` opens: three or more backticks or tildes. A backtick fence's info string
    /// holds no backtick, as `CommonMark` requires.
    fn opens(body: &str) -> Option<Fence> {
        let marker = *body.as_bytes().first()?;
        if !matches!(marker, b'`' | b'~') {
            return None;
        }
        let len = body.bytes().take_while(|byte| *byte == marker).count();
        let info = body.get(len..)?;
        let backtick_in_info = marker == b'`' && info.contains('`');
        (len >= 3 && !backtick_in_info).then_some(Fence {
            marker,
            len,
            quoted: false,
        })
    }

    /// True when `line` closes this fence: the same marker, at least as many times as it
    /// opened, and nothing else on the line. Any indent closes, which checks more text than
    /// `CommonMark` would.
    fn closes(self, line: &str) -> bool {
        let body = line.trim_start();
        let len = body.bytes().take_while(|byte| *byte == self.marker).count();
        len >= self.len && body.get(len..).is_some_and(|rest| rest.trim().is_empty())
    }
}

/// Reads one block of paragraph text (no blank line, fence, heading or code inside): its
/// reference definitions, then its code spans and links. A span or link may run over several
/// lines of the block.
fn read_block(block: &[(u32, &str)], scan: &mut Scan) {
    let (links, taken) = definition::leading(block);
    scan.links.extend(links);
    let text = block.get(taken..).unwrap_or_default();
    if text.is_empty() {
        return;
    }
    let mut source = String::new();
    let mut starts: Vec<(usize, u32)> = Vec::new();
    for &(number, line) in text {
        starts.push((source.len(), number));
        source.push_str(line);
        source.push('\n');
    }
    let line_of = |offset: usize| {
        let next = starts.partition_point(|(start, _)| *start <= offset);
        starts
            .get(next.saturating_sub(1))
            .map_or(1, |(_, number)| *number)
    };
    inline::read(&source, &line_of, scan);
}

/// True for an ATX heading: one to six `#`, then a space or the end of the line.
fn is_heading(body: &str) -> bool {
    let hashes = body.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes)
        && body
            .get(hashes..)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
}

#[cfg(test)]
mod tests;
