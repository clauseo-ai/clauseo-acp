//! Reference definitions: `[label]: destination "title"` at the start of a block, with the
//! destination on that line or on the next. A footnote definition (`[^note]:`) is not a link.

use super::Link;
use super::inline::unescape;

/// The definitions at the start of `block` (line numbers and their text), as links, and how many
/// lines of the block they take. A definition ends the block's leading definitions only when a
/// line is not one, as `CommonMark` has it: a definition cannot interrupt a paragraph.
pub(super) fn leading(block: &[(u32, &str)]) -> (Vec<Link>, usize) {
    let mut links = Vec::new();
    let mut index = 0;
    while let Some(&(number, line)) = block.get(index) {
        let next = block.get(index + 1).map(|&(_, text)| text);
        let Some(found) = definition(line, next) else {
            break;
        };
        links.push(Link {
            line: number,
            target: found.target,
        });
        index += if found.uses_next { 2 } else { 1 };
    }
    (links, index)
}

/// One definition: its destination, and whether that destination is on the line after it.
struct Definition {
    target: String,
    uses_next: bool,
}

/// The definition that `line` holds, with `next` the line after it when it has one.
fn definition(line: &str, next: Option<&str>) -> Option<Definition> {
    let rest = line.strip_prefix('[')?;
    if rest.starts_with('^') {
        return None;
    }
    let close = label_end(rest)?;
    let after = rest.get(close + 1..)?.strip_prefix(':')?;
    let here = after.trim_start_matches([' ', '\t']);
    let (text, uses_next) = if here.is_empty() {
        (next?.trim(), true)
    } else {
        (here, false)
    };
    let (target, tail) = destination(text)?;
    let tail = tail.trim();
    if !(tail.is_empty() || is_title(tail)) || target.is_empty() {
        return None;
    }
    Some(Definition {
        target: unescape(target),
        uses_next,
    })
}

/// The destination at the start of `text` and the text after it: `<...>`, or up to a space.
fn destination(text: &str) -> Option<(&str, &str)> {
    if let Some(inner) = text.strip_prefix('<') {
        let close = inner.find('>')?;
        Some((inner.get(..close)?, inner.get(close + 1..)?))
    } else {
        let end = text.find([' ', '\t']).unwrap_or(text.len());
        Some((text.get(..end)?, text.get(end..)?))
    }
}

/// True for a title a definition may end with: in quotes, or in parentheses.
fn is_title(text: &str) -> bool {
    let bytes = text.as_bytes();
    matches!(
        (bytes.first(), bytes.last()),
        (Some(b'"'), Some(b'"')) | (Some(b'\''), Some(b'\'')) | (Some(b'('), Some(b')'))
    ) && text.len() >= 2
}

/// The index of the `]` that closes a link label: the label holds text, and no unescaped bracket
/// other than that `]`.
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
