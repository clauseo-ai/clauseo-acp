//! The relay's half of the sources join (R24, PR6). For a `search_law` or `read_document` call,
//! the relay keeps the join key ([`Join`]: SHA-256 of the tool and the canonical JSON of its
//! arguments, never the arguments themselves), and when the call answers without an error it
//! reads the titles and links out of the raw result ([`found`]) and tells the host, which joins
//! them to the agent's own tool call (`sources/case_law.rs`).
//!
//! The paths read are `structuredContent.results[]` for a search and
//! `structuredContent.documents[]` for a read, the fake connector's; they are not checked
//! against the live connector, so every step is lenient: a missing or misshapen list gives no
//! links, an entry that is not an object or has no string `url` (or `link`) is skipped, as is
//! one whose link is not https or a record would drop, and a title that is not a string becomes
//! empty. At most `MAX_SEARCH_RESULTS` entries are read; the rest are skipped unparsed.

use std::fmt;

use clauseo_protocol::{MAX_SEARCH_RESULTS, MAX_WEB_TITLE_CHARS, logged_url};
use serde::Deserialize;
use serde::Deserializer as _;
use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde_json::value::RawValue;
use serde_json::{Map, Value};

use super::super::super::{CaseLawFound, CaseLawTool, FoundLink};
use super::reply::{ToolResult, object, present};
use crate::sources::case_law::join_key;
use crate::sources::{clean_title, record_url};

/// The largest raw title or link read as text; a larger one is passed over. Generous next to
/// the record's caps (300 characters of title, 2,000 of link), which [`link`] then applies.
const MAX_TEXT_BYTES: usize = 16 * 1024;

/// The one scheme a case-law result's link may have.
const HTTPS: &str = "https://";

/// What a case-law call's result is joined by.
pub(in super::super) struct Join {
    tool: CaseLawTool,
    key: [u8; 32],
}

impl Join {
    /// The join for a `tools/call` of `name` with `params`, when the tool's results become
    /// sources. Missing `arguments` are `{}`, as the connector reads them.
    pub(in super::super) fn of(name: &str, params: Option<&Value>) -> Option<Self> {
        let tool = CaseLawTool::of(name)?;
        let empty = Value::Object(Map::new());
        let arguments = params
            .and_then(|params| params.get("arguments"))
            .unwrap_or(&empty);
        Some(Self {
            tool,
            key: join_key(tool, arguments),
        })
    }
}

/// The two lists, raw.
#[derive(Deserialize)]
struct Lists<'a> {
    #[serde(borrow, default, deserialize_with = "present")]
    results: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    documents: Option<&'a RawValue>,
}

/// One entry of a list, raw.
#[derive(Deserialize)]
struct Item<'a> {
    #[serde(borrow, default, deserialize_with = "present")]
    title: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    url: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    link: Option<&'a RawValue>,
}

/// What the call found, when its result is no error. A result with no readable list still
/// counts (no links): the call ran, and its record names the query.
pub(super) fn found(join: &Join, result: &RawValue) -> Option<CaseLawFound> {
    let read = object::<ToolResult<'_>>(result)?;
    let is_error = read
        .is_error
        .is_some_and(|raw| serde_json::from_str::<bool>(raw.get()).unwrap_or(true));
    if is_error {
        return None;
    }
    let lists = read.structured.and_then(object::<Lists<'_>>);
    let list = lists.and_then(|lists| match join.tool {
        CaseLawTool::SearchLaw => lists.results,
        CaseLawTool::ReadDocument => lists.documents,
    });
    Some(CaseLawFound {
        tool: join.tool,
        key: join.key,
        links: list.map(links).unwrap_or_default(),
    })
}

/// The first `MAX_SEARCH_RESULTS` usable entries of a raw list.
fn links(list: &RawValue) -> Vec<FoundLink> {
    if !list.get().starts_with('[') {
        return Vec::new();
    }
    let mut deserializer = serde_json::Deserializer::from_str(list.get());
    deserializer.deserialize_seq(FirstLinks).unwrap_or_default()
}

/// Reads the first entries of a list and skips the rest.
struct FirstLinks;

impl<'de> Visitor<'de> for FirstLinks {
    type Value = Vec<FoundLink>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a list of results")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut links = Vec::new();
        let mut read = 0_usize;
        while read < MAX_SEARCH_RESULTS {
            let Some(raw) = seq.next_element::<&'de RawValue>()? else {
                return Ok(links);
            };
            read += 1;
            links.extend(link(raw));
        }
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(links)
    }
}

/// One entry as a link: an object with a string `url` (or `link`) that is `https://` (the
/// connector is https; the web sources' rule, which takes `http://` too, is left alone) and that
/// a record keeps ([`record_url`]: at most `MAX_RESULT_URL_CHARS` once logged), held in its
/// logged form ([`logged_url`]: no fragment, no userinfo, secret-looking values redacted), so
/// never longer than the cap; its `title` when a string, cleaned as the record cleans it
/// ([`clean_title`]) and cut to `MAX_WEB_TITLE_CHARS`. So what the thread holds is bounded by
/// the record's caps, not by [`MAX_TEXT_BYTES`].
fn link(raw: &RawValue) -> Option<FoundLink> {
    let item = object::<Item<'_>>(raw)?;
    let url = item.url.or(item.link).and_then(text)?;
    let https = url
        .get(..HTTPS.len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(HTTPS));
    if !https {
        return None;
    }
    let url = logged_url(&record_url(&url)?);
    let title = item
        .title
        .and_then(text)
        .map(|title| {
            clean_title(&title)
                .chars()
                .take(MAX_WEB_TITLE_CHARS)
                .collect()
        })
        .unwrap_or_default();
    Some(FoundLink { title, url })
}

/// A raw JSON string of at most [`MAX_TEXT_BYTES`] as text.
fn text(raw: &RawValue) -> Option<String> {
    if raw.get().len() > MAX_TEXT_BYTES {
        return None;
    }
    serde_json::from_str(raw.get()).ok()
}

#[cfg(test)]
mod tests;
