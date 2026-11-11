//! The text form of a case-law result: the live Indian Kanoon server's (fitted to its replies at
//! server main 921725f, the `ik-*.json` fixtures), which answers `search_law` and
//! `read_document` with `content[]` text blocks only.
//!
//! - **Lines.** A search hit, and each document a read opens, is a line that starts with
//!   `d<digits> "` (no indent): `d143202244 "Tofan Singh v. The State Of Tamil Nadu" · SC 2020 ·
//!   …`. The title runs from that `"` to the `"` just before the line's first ` · ` (or, with no
//!   ` · `, to the line's last `"`), so it may hold quotes of its own. Never read: indented
//!   lines (a hit's snippet), lines inside code fences, opened and closed by a line starting with
//!   three backticks (a read's paragraphs), and anything mid-line; so the documents a snippet or
//!   paragraph cites are not sources.
//! - **JSON.** With `response_format: "json"`, the first text block is an object: a search's
//!   `hits[]` or a read's `documents[]` (keyed on the tool, as the structured path is), each
//!   entry `{id, title, link}`. A `link` is used when it is https on `data.clauseo.chat` under
//!   `/d/`; otherwise the link is built from the `id`. A read's `source_url` and
//!   `unresolved[]` are never sources, and a `{"id": …, "pending": true}` entry (no title, no
//!   link yet) is skipped. An object without the tool's list leaves the remaining blocks to the
//!   line form.
//! - **Links.** The text names none per document (only the `{id}` template), so a link is built
//!   from the id, `d` included: `https://data.clauseo.chat/d/d143202244`.
//!
//! Read leniently and within bounds: only `text` blocks, at most [`MAX_SCAN_BYTES`] of their raw
//! text in all (a longer text is cut between characters, never mid-escape); a malformed line or
//! hit is skipped; a document named twice is one link; at most `MAX_SEARCH_RESULTS` documents.
//! `resource_link` blocks, and URLs in the text, are never sources. Each link then passes the
//! structured path's checks ([`super::found_link`]): a title cleaned and capped, a link https
//! and one a record keeps.

use clauseo_protocol::MAX_SEARCH_RESULTS;
use serde::Deserialize;
use serde_json::value::RawValue;

use super::super::reply::{object, present};
use super::{CaseLawTool, FoundLink, MAX_TEXT_BYTES, found_link, text as string};

/// The most raw text read from one result, across its blocks. A search's fifty hits fit well
/// inside it; a read's documents are named at their tops.
const MAX_SCAN_BYTES: usize = 512 * 1024;

/// A document link's origin, slash included.
const DOC_ORIGIN: &str = "https://data.clauseo.chat/";

/// A document's link, before its id; also what a JSON hit's `link` must start with.
const DOC_LINK: &str = "https://data.clauseo.chat/d/";

/// The most digits a document id has.
const MAX_ID_DIGITS: usize = 20;

/// What closes a title: the first field separator on the line.
const SEPARATOR: &str = " · ";

/// One `content[]` block, raw.
#[derive(Deserialize)]
struct Block<'a> {
    #[serde(rename = "type", borrow, default, deserialize_with = "present")]
    kind: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    text: Option<&'a RawValue>,
}

/// The JSON form's object: a search's hits and a read's documents, raw.
#[derive(Deserialize)]
struct Lists<'a> {
    #[serde(borrow, default, deserialize_with = "present")]
    hits: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    documents: Option<&'a RawValue>,
}

/// One JSON hit or document, raw.
#[derive(Deserialize)]
struct Hit<'a> {
    #[serde(borrow, default, deserialize_with = "present")]
    id: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    title: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    link: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    pending: Option<&'a RawValue>,
}

/// The links a result's raw `content` names, in its text form.
pub(super) fn links(tool: CaseLawTool, content: &RawValue) -> Vec<FoundLink> {
    let texts = texts(content);
    let mut found = Found::default();
    let mut rest = texts.as_slice();
    if let Some((first, after)) = texts.split_first()
        && first.starts_with('{')
        && let Ok(lists) = serde_json::from_str::<Lists<'_>>(first)
    {
        let list = match tool {
            CaseLawTool::SearchLaw => lists.hits,
            CaseLawTool::ReadDocument => lists.documents,
        };
        if let Some(list) = list {
            json_entries(list, &mut found);
            return found.links;
        }
        rest = after;
    }
    for text in rest {
        line_entries(text, &mut found);
    }
    found.links
}

/// The links found so far, at most `MAX_SEARCH_RESULTS`, each once.
#[derive(Default)]
struct Found {
    links: Vec<FoundLink>,
}

impl Found {
    fn full(&self) -> bool {
        self.links.len() >= MAX_SEARCH_RESULTS
    }

    /// Adds `url` with `title`, when it passes the checks and is new.
    fn add(&mut self, url: &str, title: Option<&str>) {
        if self.full() {
            return;
        }
        if let Some(link) = found_link(url, title)
            && !self.links.iter().any(|seen| seen.url == link.url)
        {
            self.links.push(link);
        }
    }
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

/// A document id, `d` and 1 to [`MAX_ID_DIGITS`] digits, at the start of `text`: its length.
fn id_len(text: &str) -> Option<usize> {
    let digits = text.strip_prefix('d')?;
    let len = digits.bytes().take_while(u8::is_ascii_digit).count();
    (1..=MAX_ID_DIGITS).contains(&len).then_some(1 + len)
}

/// The documents `text` names in its line form, outside code fences.
fn line_entries(text: &str, found: &mut Found) {
    let mut fenced = false;
    for line in text.lines() {
        if found.full() {
            return;
        }
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some((id, title)) = entry(line) {
            found.add(&format!("{DOC_LINK}{id}"), title);
        }
    }
}

/// A line that starts a document, `d123 "Title" · …`: its id, and its title when that is at
/// most [`MAX_TEXT_BYTES`].
fn entry(line: &str) -> Option<(&str, Option<&str>)> {
    let len = id_len(line)?;
    let (id, rest) = line.split_at_checked(len)?;
    let open = rest.strip_prefix(" \"")?;
    let end = match open.find(SEPARATOR) {
        Some(separator) => open.get(..separator)?.rfind('"')?,
        None => open.rfind('"')?,
    };
    let title = open.get(..end)?;
    Some((id, (title.len() <= MAX_TEXT_BYTES).then_some(title)))
}

/// The JSON form's hits or documents: each an object whose `link`, when it is a document's, is
/// used, and otherwise whose `id` builds one; its `title` when a string. A pending entry is
/// skipped.
fn json_entries(list: &RawValue, found: &mut Found) {
    if !list.get().starts_with('[') {
        return;
    }
    let Ok(entries) = serde_json::from_str::<Vec<&RawValue>>(list.get()) else {
        return;
    };
    for entry in entries {
        if found.full() {
            return;
        }
        let Some(hit) = object::<Hit<'_>>(entry) else {
            continue;
        };
        if hit.pending.map(RawValue::get) == Some("true") {
            continue;
        }
        let title = hit.title.and_then(string);
        let link = hit.link.and_then(string).filter(|link| is_doc_link(link));
        let url = link.or_else(|| {
            let id = hit.id.and_then(string)?;
            (id_len(&id) == Some(id.len())).then(|| format!("{DOC_LINK}{id}"))
        });
        if let Some(url) = url {
            found.add(&url, title.as_deref());
        }
    }
}

/// Whether `link` is a document's: https on `data.clauseo.chat` (either in any case), under
/// `/d/`, with something after it.
fn is_doc_link(link: &str) -> bool {
    link.len() > DOC_LINK.len()
        && link
            .get(..DOC_ORIGIN.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(DOC_ORIGIN))
        && link.get(DOC_ORIGIN.len()..DOC_LINK.len()) == Some("d/")
}

#[cfg(test)]
mod tests;
