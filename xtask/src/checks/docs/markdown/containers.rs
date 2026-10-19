//! The container prefixes of a line: block-quote markers and list-item markers, and each line's
//! indent measured inside them. A list item's text starts at its content column, so a line indented
//! four or more past that column, with no paragraph open, is an indented code block rather than
//! text.

/// The open list items and whether the last line was blank: what decides where a line belongs.
#[derive(Debug, Default)]
pub(super) struct Containers {
    /// The content column of each open list item, outermost first.
    items: Vec<usize>,
    /// True after a blank line: a line not indented into the open items closes them.
    after_blank: bool,
}

/// A line after its block-quote markers and list-item marker are stripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Content<'a> {
    /// The spaces before the text, measured from the innermost open item's content column.
    pub indent: usize,
    /// The spaces before the text, measured from the margin (after any block-quote markers).
    pub column: usize,
    /// True when the line opens a list item, which always starts a new block.
    pub starts_item: bool,
    /// The text after the indent.
    pub body: &'a str,
}

/// A list-item marker: its width in bytes, and the spaces after it that the item's text starts
/// past (one when there are none, or five or more, as `CommonMark` has it).
struct Item {
    marker: usize,
    gap: usize,
}

impl Containers {
    /// Strips the block-quote markers and any list-item marker from `line` and updates the open
    /// items. `None` for a blank line, or for a line that holds only a list marker.
    pub(super) fn strip<'a>(&mut self, line: &'a str) -> Option<Content<'a>> {
        let rest = strip_quotes(line);
        let indent = leading_spaces(rest);
        let body = rest.get(indent..).unwrap_or_default();
        if body.is_empty() {
            self.after_blank = true;
            return None;
        }
        let base = self.items.last().copied().unwrap_or(0);
        if let Some(item) = list_item(body).filter(|_| indent.saturating_sub(base) <= 3) {
            return self.open_item(indent, &item, body);
        }
        if self.after_blank {
            self.close_items_above(indent);
        }
        self.after_blank = false;
        let base = self.items.last().copied().unwrap_or(0);
        Some(Content {
            indent: indent.saturating_sub(base),
            column: indent,
            starts_item: false,
            body,
        })
    }

    /// Records a blank line (`blank`) or a line of text that is not a container's, inside a fence
    /// whose lines the caller reads itself: a blank line still ends a lazy paragraph later.
    pub(super) fn skip(&mut self, blank: bool) {
        self.after_blank = blank;
    }

    /// The content column of the item a fence opened on a line at `column` belongs to, or 0
    /// outside any item. A line that does not open an item closes the items it is not inside
    /// first, since a fence cannot be a lazy continuation.
    pub(super) fn fence_base(&mut self, column: usize, starts_item: bool) -> usize {
        if !starts_item {
            self.close_items_above(column);
        }
        self.items.last().copied().unwrap_or(0)
    }

    /// Opens the item that `body` starts at `indent`, closing the items it is not inside, and
    /// returns its text. `None` when the item holds no text.
    fn open_item<'a>(&mut self, indent: usize, item: &Item, body: &'a str) -> Option<Content<'a>> {
        self.close_items_above(indent);
        self.items.push(indent + item.marker + item.gap);
        self.after_blank = false;
        let text = body.get(item.marker + item.gap..).unwrap_or_default();
        let inner = leading_spaces(text);
        let rest = text.get(inner..).unwrap_or_default();
        (!rest.is_empty()).then_some(Content {
            indent: inner,
            column: indent,
            starts_item: true,
            body: rest,
        })
    }

    /// Closes every open item whose content column is past `indent`.
    fn close_items_above(&mut self, indent: usize) {
        while self.items.last().is_some_and(|top| *top > indent) {
            self.items.pop();
        }
    }
}

/// `line` without its block-quote markers: any run of `>` (each with one optional space after it)
/// that starts within three spaces of the margin.
pub(super) fn strip_quotes(line: &str) -> &str {
    let mut rest = line;
    loop {
        let indent = leading_spaces(rest);
        let after = rest.get(indent..).and_then(|text| text.strip_prefix('>'));
        match after {
            Some(after) if indent <= 3 => rest = after.strip_prefix(' ').unwrap_or(after),
            _ => return rest,
        }
    }
}

/// The list-item marker that starts `body` (already past its indent): `-`, `*` or `+`, or one to
/// nine digits and `.` or `)`, followed by a space, a tab or the end of the line.
fn list_item(body: &str) -> Option<Item> {
    let bytes = body.as_bytes();
    let marker = match bytes.first()? {
        b'-' | b'*' | b'+' => 1,
        b'0'..=b'9' => {
            let digits = bytes
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if digits > 9 || !matches!(bytes.get(digits), Some(b'.' | b')')) {
                return None;
            }
            digits + 1
        }
        _ => return None,
    };
    let after = body.get(marker..)?;
    if !after.is_empty() && !after.starts_with([' ', '\t']) {
        return None;
    }
    let spaces = leading_spaces(after);
    let gap = if spaces == 0 || spaces >= 5 {
        1
    } else {
        spaces
    };
    Some(Item { marker, gap })
}

/// The number of spaces that `text` starts with.
pub(super) fn leading_spaces(text: &str) -> usize {
    text.len() - text.trim_start_matches(' ').len()
}

#[cfg(test)]
mod tests;
