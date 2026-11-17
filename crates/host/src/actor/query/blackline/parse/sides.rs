//! What each version's own view shows that a redline does not mark, for `run.rs`: the address
//! behind each link, the text of each comment and the text in each text box. stetdoc 0.9.3's
//! compare marks no change when only a link's address or a comment's text changes, and shows a
//! changed text box as its old one deleted with no text, so the blackline compares the two
//! versions' views itself.
//!
//! It answers:
//! - what a version's view is: `stetdoc view --all --json --mode final <version>.docx`, read as
//!   the redline's is (`parse.rs`: a `stet 1` header, wrapped lines joined), the whole document
//!   on one page;
//! - what a link is: `{link "address" …}text{/link}` on a body line, its address every quoted
//!   string of the marker (a word such as `->p57`, a place in the document, is left out, since
//!   it moves when a paragraph is added before it), its text what the line shows between them;
//! - what a comment and a text box are: a comment, the text after its `{c0 …}` marker on a line
//!   after `{appendix}` (stetdoc lists the comments the page cites there); a text box, the text
//!   a `{textbox …}` marker shows;
//! - which links changed: a link whose web address one version has more times than the other
//!   (counted with multiplicity, so a link moved with its text is not a change, and a link whose
//!   text and address both change is), a text whose links' addresses come in another order (two
//!   links of one text swapping addresses), or a link that is on one side only around text the
//!   other side has (a link added to kept text, or taken off it);
//! - what is compared: links, comments and text boxes of the body only; those in headers and
//!   notes are left to the next stetdoc, whose compare marks them.
//!
//! A view that does not read gives an error naming the rule, never the text.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde::de::IgnoredAny;

use super::line::{Seen, escape, inline_marker, marker, quoted};
use super::{block_ref, logical_lines, stet_1};

/// The longest link text the blackline names, in characters; a longer one is cut with `…`.
pub(crate) const LINK_TEXT_MAX: usize = 200;

/// What one version's view shows that the comparison needs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Side {
    /// Each link's text (trimmed) and address, in order.
    pub(crate) links: Vec<(String, String)>,
    /// Each comment's text, in order.
    pub(crate) comments: Vec<String>,
    /// Each text box's text, in order.
    pub(crate) boxes: Vec<String>,
    /// The body's text, a line for each paragraph.
    pub(crate) text: String,
}

/// What differs between two versions' views.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Crossed {
    /// The texts of the links whose address changed, each once, the newer version's first.
    pub(crate) links: Vec<String>,
    /// Whether a link with no text changed, which can't be named.
    pub(crate) unnamed_link: bool,
    /// Whether a comment's text changed, or a comment came or went.
    pub(crate) comments: bool,
    /// Whether a text box's text changed, or a text box came or went.
    pub(crate) boxes: bool,
}

/// The fields of the view's JSON the read needs.
#[derive(Deserialize)]
struct View {
    mode: String,
    text: String,
    #[serde(default)]
    next: Option<IgnoredAny>,
}

/// One version's view (`json`), read.
pub(crate) fn read_side(json: &str) -> Result<Side, &'static str> {
    let view: View = serde_json::from_str(json).map_err(|_bad| "the JSON is not a view")?;
    if view.mode != "final" {
        return Err("the view is not of the final text");
    }
    if view.next.is_some() {
        return Err("the view is cut short");
    }
    if !stet_1(&view.text) {
        return Err("the view is not in the stet 1 format");
    }
    let mut side = Side::default();
    let mut appendix = false;
    for (_, line) in logical_lines(&view.text) {
        if line == "{appendix}" {
            appendix = true;
        } else if appendix {
            if let Some(text) = comment(&line) {
                side.comments.push(text?);
            }
        } else if let Some((_, content)) = block_ref(&line) {
            scan(content, &mut side)?;
            side.text.push('\n');
        }
    }
    Ok(side)
}

/// The text of the comment an appendix line lists (`{c0 P 2026-11-01 on p2 "…"} Text.`); `None`
/// for a line that lists something else (a note).
fn comment(line: &str) -> Option<Result<String, &'static str>> {
    let (body, after) = marker(line).ok()?;
    let first = body.split(' ').next()?;
    let number = first.strip_prefix('c')?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut text = String::new();
    let mut rest = after.strip_prefix(' ').unwrap_or(after);
    while let Some(c) = rest.chars().next() {
        if c == '\\' {
            match escape(rest) {
                Ok((c, after)) => {
                    text.push(c);
                    rest = after;
                }
                Err(why) => return Some(Err(why)),
            }
        } else {
            text.push(c);
            rest = rest.get(c.len_utf8()..).unwrap_or_default();
        }
    }
    Some(Ok(text))
}

/// Reads a body line's content into `side`: its text, links and text boxes.
fn scan(content: &str, side: &mut Side) -> Result<(), &'static str> {
    let mut rest = content;
    // The open link's address and text.
    let mut link: Option<(String, String)> = None;
    let mut seen = Seen::default();
    while let Some(c) = rest.chars().next() {
        let shown = match c {
            '\\' => {
                let (c, after) = escape(rest)?;
                rest = after;
                c.to_string()
            }
            '{' => {
                let (body, after) = marker(rest)?;
                rest = after;
                let first = body.split(' ').next().unwrap_or_default();
                match first {
                    "link" => {
                        close(&mut link, side);
                        link = Some((address(body)?, String::new()));
                        String::new()
                    }
                    "/link" => {
                        close(&mut link, side);
                        String::new()
                    }
                    _ => {
                        let (shown, _) = inline_marker(body, &mut seen)?;
                        if first == "textbox" {
                            side.boxes.push(shown.clone());
                        }
                        shown
                    }
                }
            }
            '}' => return Err("a closing brace that is not escaped"),
            c => {
                rest = rest.get(c.len_utf8()..).unwrap_or_default();
                c.to_string()
            }
        };
        side.text.push_str(&shown);
        if let Some((_, text)) = link.as_mut() {
            text.push_str(&shown);
        }
    }
    // A link stetdoc leaves open at the line's end ends with it.
    close(&mut link, side);
    Ok(())
}

/// Ends the open link, if any, adding it to `side`.
fn close(link: &mut Option<(String, String)>, side: &mut Side) {
    if let Some((address, text)) = link.take() {
        side.links.push((text.trim().to_owned(), address));
    }
}

/// A link marker's address: its quoted strings, joined by a space.
fn address(body: &str) -> Result<String, &'static str> {
    let mut parts = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find('"') {
        let (text, after) = quoted(rest.get(at..).unwrap_or_default())?;
        parts.push(text);
        rest = after;
    }
    Ok(parts.join(" "))
}

/// What differs between the older version's view (`old`) and the newer one's (`new`).
pub(crate) fn cross(old: &Side, new: &Side) -> Crossed {
    let mut crossed = Crossed {
        comments: old.comments != new.comments,
        boxes: old.boxes != new.boxes,
        ..Crossed::default()
    };
    let (was, is) = (counts(old), counts(new));
    let mut changed = surplus(new, &is, &was);
    changed.extend(surplus(old, &was, &is));
    // The same addresses in another order among links of one text (two `here`s swapping
    // addresses); links that move with their text keep each text's order.
    let (before, after) = (by_text(old), by_text(new));
    for (text, _) in new.links.iter().chain(&old.links) {
        let (was, is) = (before.get(text.as_str()), after.get(text.as_str()));
        let moved = if let (Some(was), Some(is)) = (was, is) {
            was != is
        } else {
            // A link on one side only is a change of its own when the other side has its text
            // (Word marks a link added to kept text no more than one whose address changed).
            let other = if was.is_none() { old } else { new };
            !text.is_empty() && other.text.contains(text.as_str())
        };
        if moved {
            changed.push(text.as_str());
        }
    }
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for text in changed {
        if !done.insert(text) {
            continue;
        }
        if text.is_empty() {
            crossed.unnamed_link = true;
        } else {
            crossed.links.push(cut(text));
        }
    }
    crossed
}

/// Each web address's count in `side`; a place in the document (no address) is left out.
fn counts(side: &Side) -> BTreeMap<&str, usize> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, address) in &side.links {
        if !address.is_empty() {
            *counts.entry(address.as_str()).or_default() += 1;
        }
    }
    counts
}

/// Each link text of `side` with its links' addresses, in order.
fn by_text(side: &Side) -> BTreeMap<&str, Vec<&str>> {
    let mut texts: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (text, address) in &side.links {
        texts
            .entry(text.as_str())
            .or_default()
            .push(address.as_str());
    }
    texts
}

/// The texts of `side`'s links whose address the other version has fewer times (`mine` and
/// `theirs`, each address's count on the two sides): new, gone or changed, whatever its text
/// did (`account A` at one address becoming `account B` at another).
fn surplus<'a>(
    side: &'a Side,
    mine: &BTreeMap<&str, usize>,
    theirs: &BTreeMap<&str, usize>,
) -> Vec<&'a str> {
    let mut left: BTreeMap<&str, usize> = BTreeMap::new();
    for (address, count) in mine {
        let extra = count.saturating_sub(theirs.get(address).copied().unwrap_or(0));
        if extra > 0 {
            left.insert(address, extra);
        }
    }
    let mut texts = Vec::new();
    for (text, address) in &side.links {
        if let Some(extra) = left.get_mut(address.as_str()).filter(|extra| **extra > 0) {
            *extra -= 1;
            texts.push(text.as_str());
        }
    }
    texts
}

/// `text` cut to [`LINK_TEXT_MAX`] characters, with `…` where it was cut.
fn cut(text: &str) -> String {
    match text.char_indices().nth(LINK_TEXT_MAX) {
        Some((at, _)) => format!("{}…", text.get(..at).unwrap_or_default()),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests;
