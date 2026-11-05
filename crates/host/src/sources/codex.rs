//! Codex's own web search (S26): codex-acp's `webSearch` item, reported as a `search` tool call
//! (codex-acp 2.2.2, `xacp:tool-calls/reporters/WebSearchReporter.ts:9-54`).
//!
//! The call's `rawInput` is the AIR shape `{query, action}`, which Clauseo gets because it
//! advertises `jetbrains.air` (`crates/acp/src/driver.rs`), or the standard shape
//! `{type: "webSearch", id, query, action}`. Both carry the same `action`, which is `null` or an
//! object tagged by `type` (`WebSearchReporter.ts:39-53`):
//! - `null`, or `search` (`query`, `queries`): a search, recorded with no results, since codex-acp
//!   sends none (`PageVisited` and `WebSearchRecorded` only).
//! - `openPage` (`url`) and `findInPage` (`url`, `pattern`): a page, `PageVia::CodexOpenPage`.
//! - `other`, or a type not known: nothing, with a warning in the host log.

use clauseo_protocol::{PageVia, ToolKind};
use serde_json::Value;

use super::{Found, query_of, record_url};

/// Whether a call is Codex's search, by its shape: kind `search`, and an input that has a
/// `query` (the start item's is empty, but present).
pub(crate) fn is_search(kind: ToolKind, input: Option<&Value>) -> bool {
    kind == ToolKind::Search && input.is_some_and(|input| input.get("query").is_some())
}

/// What a finished Codex web search found, from its input. None when it found nothing to record.
pub(crate) fn found(input: &Value) -> Option<Found> {
    let action = input.get("action").filter(|action| !action.is_null());
    match action
        .and_then(|action| action.get("type"))
        .and_then(Value::as_str)
    {
        None | Some("search") => {
            let query = query_of(input)?;
            Some(Found::Search {
                query,
                results: Vec::new(),
            })
        }
        Some("openPage" | "findInPage") => {
            let url = record_url(action?.get("url")?.as_str()?)?;
            Some(Found::Page {
                url,
                title: None,
                via: PageVia::CodexOpenPage,
            })
        }
        Some(other) => {
            tracing::warn!(action = other, "a Codex web action Clauseo does not record");
            None
        }
    }
}

#[cfg(test)]
mod tests;
