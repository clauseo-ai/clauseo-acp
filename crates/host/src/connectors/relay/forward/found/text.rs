//! The text form of a case-law result: the live Indian Kanoon server's, which answers with
//! `content[]` text blocks only. A document is written `d123 "Title"` (a `d`, 1 to 20 digits,
//! optional spaces or tabs, then the title in double quotes, closed on the same line; `\"`
//! inside it is a quote). Its link is `https://data.clauseo.chat/d/<id>`, built from the id as
//! written, unless the text carries an explicit `https://data.clauseo.chat/d/<id>` link for the
//! same document (the id in it with or without the `d`), which is used instead.
//!
//! Read leniently and within bounds: only `text` blocks are read, and of them at most
//! [`MAX_SCAN_BYTES`] in all (a longer text is cut at a character, never mid-escape); an id
//! with no closed title is skipped; a document mentioned twice is one link; at most
//! `MAX_SEARCH_RESULTS` documents for a search, and only the first for a read (a judgment's
//! text may name the documents it cites). Each link then passes the structured path's checks
//! ([`super::found_link`]), so a title is cleaned and capped and a link is https and one a
//! record keeps.

use clauseo_protocol::MAX_SEARCH_RESULTS;
use serde::Deserialize;
use serde_json::value::RawValue;

use super::super::reply::{object, present};
use super::{MAX_TEXT_BYTES, found_link};
use crate::connectors::{CaseLawTool, FoundLink};

/// The most raw text read from one result, across its blocks. A search's fifty entries fit
/// well inside it; a read's document is named at its top.
const MAX_SCAN_BYTES: usize = 512 * 1024;

/// The origin (with its slash) a document's link has, in any case.
const DOC_ORIGIN: &str = "https://data.clauseo.chat/";

/// The origin and path a document's link has, and the one an explicit link must have.
const DOC_LINK: &str = "https://data.clauseo.chat/d/";

/// The most digits a document id has.
const MAX_ID_DIGITS: usize = 20;

/// One `content[]` block, raw.
#[derive(Deserialize)]
struct Block<'a> {
    #[serde(rename = "type", borrow, default, deserialize_with = "present")]
    kind: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    text: Option<&'a RawValue>,
}

/// A document named in the text: its id as written, its digits, its raw title.
struct Entry {
    id: String,
    digits: String,
    title: Option<String>,
}

/// The links a result's raw `content` names, in its text form.
pub(super) fn links(tool: CaseLawTool, content: &RawValue) -> Vec<FoundLink> {
    let most = match tool {
        CaseLawTool::SearchLaw => MAX_SEARCH_RESULTS,
        CaseLawTool::ReadDocument => 1,
    };
    let texts = texts(content);
    let mut entries: Vec<Entry> = Vec::new();
    for text in &texts {
        for entry in entries_in(text) {
            if entries.len() >= most {
                break;
            }
            if !entries.iter().any(|seen| seen.digits == entry.digits) {
                entries.push(entry);
            }
        }
    }
    let mut explicit: Vec<Option<String>> = vec![None; entries.len()];
    for text in &texts {
        for (digits, url) in doc_links_in(text) {
            let at = entries.iter().position(|entry| entry.digits == digits);
            if let Some(slot) = at.and_then(|at| explicit.get_mut(at))
                && slot.is_none()
            {
                *slot = Some(url.to_owned());
            }
        }
    }
    entries
        .into_iter()
        .zip(explicit)
        .filter_map(|(entry, explicit)| {
            let url = explicit.unwrap_or_else(|| format!("{DOC_LINK}{}", entry.id));
            found_link(&url, entry.title.as_deref())
        })
        .collect()
}

/// The text of each `text` block, at most [`MAX_SCAN_BYTES`] of raw text in all.
fn texts(content: &RawValue) -> Vec<String> {
    if !content.get().starts_with('[') {
        return Vec::new();
    }
    let Ok(blocks) = serde_json::from_str::<Vec<&RawValue>>(content.get()) else {
        return Vec::new();
    };
    let mut budget = MAX_SCAN_BYTES;
    let mut texts = Vec::new();
    for block in blocks {
        if budget == 0 {
            break;
        }
        let Some(block) = object::<Block<'_>>(block) else {
            continue;
        };
        if block.kind.map(RawValue::get) != Some("\"text\"") {
            continue;
        }
        let Some(raw) = block.text else {
            continue;
        };
        let left = budget;
        budget = budget.saturating_sub(raw.get().len());
        if let Some(text) = prefix(raw, left) {
            texts.push(text);
        }
    }
    texts
}

/// A raw JSON string as text, cut to the characters within its first `budget` raw bytes when
/// it is longer: the cut falls between characters or escapes, never inside one (nor between
/// the halves of a surrogate pair), so the prefix still reads.
fn prefix(raw: &RawValue, budget: usize) -> Option<String> {
    let all = raw.get();
    let inner = all.strip_prefix('"')?.strip_suffix('"')?;
    if inner.len() <= budget {
        return serde_json::from_str(all).ok();
    }
    let bytes = inner.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let len = token_len(bytes, at);
        if at + len > budget {
            break;
        }
        at += len;
    }
    serde_json::from_str(&format!("\"{}\"", inner.get(..at)?)).ok()
}

/// The length of the character or escape at `at` in a JSON string's raw inside.
fn token_len(bytes: &[u8], at: usize) -> usize {
    match bytes.get(at).copied().unwrap_or(0) {
        b'\\' if bytes.get(at + 1) == Some(&b'u') => {
            let high = bytes
                .get(at + 2..at + 6)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u16::from_str_radix(hex, 16).ok())
                .is_some_and(|unit| (0xD800..0xDC00).contains(&unit));
            let paired = bytes.get(at + 6..at + 8) == Some(b"\\u".as_slice());
            if high && paired { 12 } else { 6 }
        }
        b'\\' => 2,
        lead if lead < 0x80 => 1,
        lead if lead < 0xE0 => 2,
        lead if lead < 0xF0 => 3,
        _ => 4,
    }
}

/// The documents `text` names, `d123 "Title"`, in order; a malformed one is skipped.
fn entries_in(text: &str) -> impl Iterator<Item = Entry> + '_ {
    text.match_indices('d')
        .filter_map(move |(at, _)| entry_at(text, at))
}

/// The document named at `at`, a `d`, when it starts one.
fn entry_at(text: &str, at: usize) -> Option<Entry> {
    let before = text.get(..at)?.chars().next_back();
    if before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '/' | '-' | '.')) {
        return None;
    }
    let rest = text.get(at + 1..)?;
    let digits_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=MAX_ID_DIGITS).contains(&digits_len) {
        return None;
    }
    let digits = rest.get(..digits_len)?;
    let rest = rest.get(digits_len..)?.trim_start_matches([' ', '\t']);
    let title = rest.strip_prefix('"')?;
    let close = closing_quote(title)?;
    let raw_title = title.get(..close)?;
    Some(Entry {
        id: format!("d{digits}"),
        digits: digits.to_owned(),
        title: (raw_title.len() <= MAX_TEXT_BYTES).then(|| raw_title.replace("\\\"", "\"")),
    })
}

/// Where a title closes: the first `"` not after a `\`, on the same line.
fn closing_quote(title: &str) -> Option<usize> {
    let mut escaped = false;
    for (at, c) in title.char_indices() {
        match c {
            '\n' | '\r' => return None,
            '"' if !escaped => return Some(at),
            '\\' => {
                escaped = !escaped;
                continue;
            }
            _ => {}
        }
        escaped = false;
    }
    None
}

/// The explicit document links in `text`, `https://data.clauseo.chat/d/<id>` (scheme and host
/// in any case, the id with or without its `d`), each with its id's digits and the link as
/// written up to the end of its id.
fn doc_links_in(text: &str) -> impl Iterator<Item = (&str, &str)> + '_ {
    text.match_indices(['h', 'H']).filter_map(move |(at, _)| {
        let found = text.as_bytes().get(at..at + DOC_LINK.len())?;
        let (host, path) = found.split_at_checked(DOC_ORIGIN.len())?;
        if !host.eq_ignore_ascii_case(DOC_ORIGIN.as_bytes()) || path != b"d/" {
            return None;
        }
        let id_at = at + DOC_LINK.len();
        let rest = text.get(id_at..)?;
        let skip = usize::from(rest.starts_with('d'));
        let digits_len = rest
            .get(skip..)?
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if !(1..=MAX_ID_DIGITS).contains(&digits_len) {
            return None;
        }
        let end = id_at + skip + digits_len;
        let next = text.get(end..)?.chars().next();
        if next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        Some((text.get(id_at + skip..end)?, text.get(at..end)?))
    })
}

#[cfg(test)]
mod tests;
