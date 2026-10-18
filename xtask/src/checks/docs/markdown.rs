//! Reads the Markdown the docs check needs, by hand: fenced code blocks, inline code spans, inline
//! link destinations and reference definitions. No Markdown parser is among our dependencies, so
//! this follows `CommonMark` for those constructs and no further. Block quotes and list markers are
//! read as text. An indented code block is read as text too, so a code span in one is checked.

use crate::checks;

/// What `scan` found in one Markdown file, outside the constructs the check does not read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Scan {
    /// Inline code spans outside fenced blocks, in file order. HTML comments are not skipped.
    pub spans: Vec<Span>,
    /// Inline link destinations and reference definitions, in file order.
    pub links: Vec<Link>,
    /// The lines inside fenced code blocks, in file order, without the fence lines.
    pub fenced: Vec<Line>,
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
    /// The 1-based line of the link's `[`, or of the definition.
    pub line: u32,
    pub target: String,
}

/// One line of a fenced code block.
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
    for (index, line) in text.lines().enumerate() {
        let number = checks::line_at(index);
        if let Some(open) = fence {
            if open.closes(line) {
                fence = None;
            } else {
                scan.fenced.push(Line {
                    line: number,
                    text: line.to_owned(),
                });
            }
            continue;
        }
        if let Some(opened) = Fence::opens(line) {
            read_block(&block, &mut scan);
            block.clear();
            fence = Some(opened);
        } else if is_heading(line) {
            read_block(&block, &mut scan);
            read_block(&[(number, line)], &mut scan);
            block.clear();
        } else if line.trim().is_empty() {
            read_block(&block, &mut scan);
            block.clear();
        } else {
            block.push((number, line));
        }
    }
    read_block(&block, &mut scan);
    scan
}

/// A fence that opens a fenced code block: its marker and the length of its run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fence {
    /// `` ` `` or `~`.
    marker: u8,
    /// How many markers open the block; the closing run must be at least as long.
    len: usize,
}

impl Fence {
    /// The fence `line` opens: three or more backticks or tildes after any indent. A backtick
    /// fence's info string holds no backtick, as `CommonMark` requires.
    fn opens(line: &str) -> Option<Fence> {
        let body = line.trim_start();
        let marker = *body.as_bytes().first()?;
        if !matches!(marker, b'`' | b'~') {
            return None;
        }
        let len = body.bytes().take_while(|byte| *byte == marker).count();
        let info = body.get(len..)?;
        let backtick_in_info = marker == b'`' && info.contains('`');
        (len >= 3 && !backtick_in_info).then_some(Fence { marker, len })
    }

    /// True when `line` closes this fence: the same marker, at least as many times as it
    /// opened, and nothing else on the line.
    fn closes(self, line: &str) -> bool {
        let body = line.trim_start();
        let len = body.bytes().take_while(|byte| *byte == self.marker).count();
        len >= self.len && body.get(len..).is_some_and(|rest| rest.trim().is_empty())
    }
}

/// Reads one block of text lines (no blank line or fence inside): its code spans, its links and
/// its reference definitions. A span or link may run over several lines of the block.
fn read_block(block: &[(u32, &str)], scan: &mut Scan) {
    if block.is_empty() {
        return;
    }
    let mut source = String::new();
    let mut starts: Vec<(usize, u32)> = Vec::new();
    // Reference definitions open a block; a line of other text ends them, as in CommonMark.
    let mut defining = true;
    for &(number, line) in block {
        match definition(line).filter(|_| defining) {
            Some(target) => scan.links.push(Link {
                line: number,
                target,
            }),
            None => defining = false,
        }
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
    read_inline(&source, &line_of, scan);
}

/// Reads the inline code spans and inline links of `source`. A backslash escapes the ASCII
/// punctuation after it. Code spans bind tighter than brackets, so a bracket inside a span is
/// not a link.
fn read_inline<F: Fn(usize) -> u32>(source: &str, line_of: &F, scan: &mut Scan) {
    let bytes = source.as_bytes();
    let mut openers: Vec<usize> = Vec::new();
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'\\' && bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
            i += 2;
            continue;
        }
        match byte {
            b'`' => {
                let run = run_len(bytes, i);
                if let Some(close) = closing_run(bytes, i + run, run) {
                    scan.spans.push(Span {
                        line: line_of(i),
                        text: span_text(&source[i + run..close]),
                    });
                    i = close + run;
                } else {
                    i += run;
                }
            }
            b'[' => {
                openers.push(i);
                i += 1;
            }
            b']' => match (openers.pop(), inline_target(source, i + 1)) {
                (Some(open), Some((target, end))) => {
                    scan.links.push(Link {
                        line: line_of(open),
                        target,
                    });
                    i = end;
                }
                _ => i += 1,
            },
            _ => i += 1,
        }
    }
}

/// How many backticks start at `start`.
fn run_len(bytes: &[u8], start: usize) -> usize {
    bytes
        .get(start..)
        .unwrap_or_default()
        .iter()
        .take_while(|byte| **byte == b'`')
        .count()
}

/// The index of the first backtick run of exactly `len` backticks at or after `from`.
fn closing_run(bytes: &[u8], from: usize, len: usize) -> Option<usize> {
    let mut j = from;
    while let Some(&byte) = bytes.get(j) {
        if byte == b'`' {
            let run = run_len(bytes, j);
            if run == len {
                return Some(j);
            }
            j += run;
        } else {
            j += 1;
        }
    }
    None
}

/// A code span's content as `CommonMark` gives it to a reader: line breaks as spaces, ends trimmed.
fn span_text(raw: &str) -> String {
    raw.replace('\n', " ").trim().to_owned()
}

/// The destination of the inline link whose `(` is at `at`, and the index just after its `)`.
/// `None` when the text at `at` is not an inline link: no `(`, no `)`, or a broken title.
fn inline_target(source: &str, at: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    if bytes.get(at) != Some(&b'(') {
        return None;
    }
    let start = skip_space(bytes, at + 1);
    let (target, mut i) = if bytes.get(start) == Some(&b'<') {
        let inner = source.get(start + 1..)?;
        let close = inner.find('>')?;
        let target = inner.get(..close)?;
        if target.contains('\n') {
            return None;
        }
        (target.to_owned(), start + close + 2)
    } else {
        let end = bare_end(bytes, start);
        (unescape(source.get(start..end)?), end)
    };
    i = skip_space(bytes, i);
    if matches!(bytes.get(i), Some(b'"' | b'\'' | b'(')) {
        i = title_end(bytes, i)?;
        i = skip_space(bytes, i);
    }
    (bytes.get(i) == Some(&b')')).then_some((target, i + 1))
}

/// The index of the first byte at or after `from` that is not a space, tab or line break.
fn skip_space(bytes: &[u8], from: usize) -> usize {
    let rest = bytes.get(from..).unwrap_or_default();
    from + rest
        .iter()
        .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\n'))
        .count()
}

/// The end of a bare link destination that starts at `start`: before the first space, or the
/// `)` that is not closing a `(` inside the destination.
fn bare_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'\\' && bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
            i += 2;
            continue;
        }
        match byte {
            b'(' => depth += 1,
            b')' if depth == 0 => break,
            b')' => depth -= 1,
            b' ' | b'\t' | b'\n' => break,
            _ => {}
        }
        i += 1;
    }
    i
}

/// The index just after the title that opens at `open` (a quote or a `(`), `None` if it never
/// closes.
fn title_end(bytes: &[u8], open: usize) -> Option<usize> {
    let close = if bytes.get(open) == Some(&b'(') {
        b')'
    } else {
        *bytes.get(open)?
    };
    let mut i = open + 1;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'\\' && bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
            i += 2;
            continue;
        }
        if byte == close {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// `text` with each backslash before ASCII punctuation removed, as a destination reads it.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(next)) if next.is_ascii_punctuation() => {
                out.push(*next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// The destination of the reference definition `line` holds, `[label]: target`, or `None` when
/// the line is not one. A footnote definition (`[^note]:`) is not a link.
fn definition(line: &str) -> Option<String> {
    let indented = line.trim_start_matches(' ');
    if line.len() - indented.len() > 3 {
        return None;
    }
    let rest = indented.strip_prefix('[')?;
    if rest.starts_with('^') {
        return None;
    }
    let close = label_end(rest)?;
    let after = rest.get(close + 1..)?.strip_prefix(':')?;
    let dest = after.trim_start_matches([' ', '\t']);
    let (target, rest) = if let Some(inner) = dest.strip_prefix('<') {
        let close = inner.find('>')?;
        (inner.get(..close)?, inner.get(close + 1..)?)
    } else {
        let end = dest.find([' ', '\t']).unwrap_or(dest.len());
        (dest.get(..end)?, dest.get(end..)?)
    };
    let rest = rest.trim();
    let titled = rest.is_empty() || is_title(rest);
    (!target.is_empty() && titled).then(|| unescape(target))
}

/// True for a title that a definition may end with: in quotes, or in parentheses.
fn is_title(text: &str) -> bool {
    matches!(
        (text.as_bytes().first(), text.as_bytes().last()),
        (Some(b'"'), Some(b'"')) | (Some(b'\''), Some(b'\'')) | (Some(b'('), Some(b')'))
    ) && text.len() >= 2
}

/// True for an ATX heading line: one to six `#`, then a space or the end of the line.
fn is_heading(line: &str) -> bool {
    let body = line.trim_start();
    let hashes = body.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes)
        && body
            .get(hashes..)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
}

/// The index of the `]` that closes a link label, when the label holds text and no unescaped
/// bracket other than that `]`.
fn label_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        match byte {
            b'\\' => i += 1,
            b'[' => return None,
            b']' => {
                let label = text.get(..i)?;
                return (!label.trim().is_empty()).then_some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests;
