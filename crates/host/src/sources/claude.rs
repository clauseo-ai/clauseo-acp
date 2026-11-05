//! Claude Code's own web tools (S26): `WebSearch`, which searches, and `WebFetch`, which reads a
//! page. The adapter names both (claude-agent-acp 0.89.1, `cacp:tool-calls/renderer.js:76-101`,
//! `cacp:tool-calls/reporters/web.js:1-44`); each is a `fetch` tool call.
//!
//! - A search's query is `rawInput.query` (`{query, allowed_domains?, blocked_domains?}`). Its
//!   results are the `Links: [{title, url}, ...]` arrays in `rawOutput`, one per backend search,
//!   in order. When no array parses, they are the `Title (url)` lines of `content`, split at the
//!   last ` (`, so a title with parentheses stays whole.
//! - A fetch's page is `rawInput.url` (`{url, prompt, offset?}`), with no title.
//!
//! The tool is named by the call's `name` (`ToolCall.tool_name`); a call with no name is matched
//! by its title, as [`tool`] says.

use std::collections::BTreeSet;

use clauseo_protocol::{MAX_SEARCH_RESULTS, PageVia, ToolKind, WebResult};
use serde_json::Value;

use super::browser::web_url;
use super::{Found, clean_title, output_text, query_of};

/// The name of Claude Code's search tool.
pub(crate) const SEARCH: &str = "WebSearch";

/// The name of Claude Code's fetch tool.
pub(crate) const FETCH: &str = "WebFetch";

/// A search's title before its query is known is `Web search`; once known it is `Search "<q>"`.
const SEARCH_TITLE: &str = "Search \"";

/// A fetch's title is `Fetch <url>`.
const FETCH_TITLE: &str = "Fetch ";

/// The prefix of a result line in `rawOutput`.
const LINKS: &str = "Links:";

/// Which of Claude's web tools a call is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tool {
    /// `WebSearch`.
    Search,
    /// `WebFetch`.
    Fetch,
}

/// Which Claude web tool a tool call is. By its `name`: `WebSearch` or `WebFetch`, and any other
/// name is no web tool. With no name, a `fetch` call whose title is `Search "<q>"` and whose input
/// holds a `query`, or `Fetch <url>` with a `url`, is one (the adapter's titles, `web.js:20`).
pub(crate) fn tool(
    kind: ToolKind,
    name: Option<&str>,
    title: &str,
    input: Option<&Value>,
) -> Option<Tool> {
    match name {
        Some(SEARCH) => Some(Tool::Search),
        Some(FETCH) => Some(Tool::Fetch),
        Some(_) => None,
        None if kind != ToolKind::Fetch => None,
        None => {
            let input = input?;
            if title.starts_with(SEARCH_TITLE) && input.get("query").is_some() {
                Some(Tool::Search)
            } else if title.starts_with(FETCH_TITLE) && input.get("url").is_some() {
                Some(Tool::Fetch)
            } else {
                None
            }
        }
    }
}

/// A finished `WebSearch`: its query and results. None when the input names no query.
pub(crate) fn search(
    input: Option<&Value>,
    output: Option<&Value>,
    text: &[String],
) -> Option<Found> {
    let query = query_of(input?)?;
    let links = output.and_then(links_in).unwrap_or_else(|| lines_in(text));
    Some(Found::Search {
        query,
        results: results(links),
    })
}

/// A finished `WebFetch`: the page its input names, with no title. None when the URL is not a web
/// URL.
pub(crate) fn fetch(input: Option<&Value>) -> Option<Found> {
    let url = web_url(input?.get("url")?.as_str()?)?;
    Some(Found::Page {
        url,
        title: None,
        via: PageVia::WebFetch,
    })
}

/// One search result as the agent sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Link {
    title: String,
    url: String,
}

/// The links of every `Links:` array in `output`, in order. None when no such array parses, so
/// the caller can fall back on `content`; an empty array is a parse that found no links.
fn links_in(output: &Value) -> Option<Vec<Link>> {
    let text = output_text(output);
    let mut found: Option<Vec<Link>> = None;
    for line in text.lines() {
        let Some(json) = line.trim_start().strip_prefix(LINKS) else {
            continue;
        };
        let Ok(items) = serde_json::from_str::<Vec<Value>>(json.trim()) else {
            continue;
        };
        found
            .get_or_insert_with(Vec::new)
            .extend(items.iter().filter_map(link_of));
    }
    found
}

/// A `{title, url}` object; one without a string `url` is no link. A missing title is empty.
fn link_of(value: &Value) -> Option<Link> {
    let url = value.get("url")?.as_str()?.to_owned();
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Some(Link { title, url })
}

/// The links in the `Title (url)` lines of `content`, each split at its last ` (`. A line that
/// does not end in `)` is no link.
fn lines_in(text: &[String]) -> Vec<Link> {
    text.iter()
        .flat_map(|block| block.lines())
        .filter_map(|line| {
            let line = line.trim();
            let inner = line.strip_suffix(')')?;
            let (title, url) = inner.rsplit_once(" (")?;
            Some(Link {
                title: title.to_owned(),
                url: url.to_owned(),
            })
        })
        .collect()
}

/// The results the log keeps from `links`, in order: a link whose URL is over
/// [`MAX_RESULT_URL_CHARS`](super::MAX_RESULT_URL_CHARS) characters or is no web URL is dropped,
/// as is a repeat of an earlier URL (as logged). A title is cleaned, and an empty one is the URL.
/// At most [`MAX_SEARCH_RESULTS`] are kept.
fn results(links: Vec<Link>) -> Vec<WebResult> {
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    for link in links {
        if results.len() >= MAX_SEARCH_RESULTS {
            break;
        }
        if link.url.chars().count() > super::MAX_RESULT_URL_CHARS {
            continue;
        }
        let Some(url) = web_url(&link.url) else {
            continue;
        };
        let title = clean_title(&link.title);
        let title = if title.is_empty() { url.clone() } else { title };
        let result = WebResult::new(title, &url);
        if seen.insert(result.url.clone()) {
            results.push(result);
        }
    }
    results
}

#[cfg(test)]
mod tests;
