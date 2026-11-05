//! Search inside matters and conversations: `HostQuery::Search` and its answer,
//! `QueryResult::Search`. The host answers it from its search index (decision 0043), never
//! from the log.

use serde::{Deserialize, Serialize};

use crate::open_enum::open_unit_enum;
use crate::{Count, MatterId, Seq, ThreadId, Timestamp};

/// Hits a page holds when the request names no `limit`.
pub const DEFAULT_SEARCH_LIMIT: u16 = 20;

/// Most hits one page holds; a larger `limit` is cut to it.
pub const MAX_SEARCH_LIMIT: u16 = 50;

/// One search, as the lawyer typed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SearchRequest {
    /// What the lawyer typed. While its last character is not whitespace the last word is
    /// still being typed: hits come newest first and that word matches as a prefix. Otherwise
    /// hits are ranked.
    pub query: String,
    /// Every matter, or one.
    pub scope: SearchScope,
    /// Whether archived conversations' hits are included.
    pub include_archived: bool,
    /// Only these kinds; `None` (missing in JSON) is every kind. An empty list finds nothing.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub kinds: Option<Vec<SearchKind>>,
    /// Hits per page: [`DEFAULT_SEARCH_LIMIT`] when `None`, at most [`MAX_SEARCH_LIMIT`]
    /// (larger is cut to it, 0 is read as 1).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub limit: Option<u16>,
    /// The `next` of the page before, for the page after it; `None` for the first page. A
    /// cursor the host cannot read, or one made for another query, gives the first page.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cursor: Option<String>,
    /// The requesting window's count of searches, larger for each newer one. A newer search
    /// from the same window stops this one: it is answered `Cancelled`.
    pub generation: Count,
}

/// Where a search looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SearchScope {
    /// Every matter on the list. JSON: `"all"`.
    All,
    /// One matter. JSON: `{"matter": "<id>"}`.
    Matter(MatterId),
}

open_unit_enum! {
    /// What a hit matched.
    pub enum SearchKind {
        /// A matter's name.
        MatterTitle,
        /// A conversation's title.
        ThreadTitle,
        /// The lawyer's prompt.
        Prompt,
        /// One agent message of kind answer, whole once its turn ended.
        Answer,
        /// A tool call's title.
        ToolTitle,
        /// A document's path in the matter folder.
        FileName,
    }
    test mod search_kind;
}

/// A search's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SearchAnswer {
    /// The hits.
    Page(SearchPage),
    /// A newer search from the same window came first; this one has no hits. JSON:
    /// `"cancelled"`.
    Cancelled,
}

/// One page of hits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SearchPage {
    /// The hits, best first when ranked, newest first otherwise.
    pub hits: Vec<SearchHit>,
    /// The cursor for the next page, when there may be one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub next: Option<String>,
    /// How the hits are ordered.
    pub mode: SearchMode,
    /// While the index is being built, how far it is: the hits come from what is indexed so
    /// far.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub indexing: Option<SearchProgress>,
}

/// How a page's hits are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SearchMode {
    /// Newest first: the last word was still being typed.
    NewestFirst,
    /// Best first.
    Ranked,
}

/// A build of the search index, in conversations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SearchProgress {
    /// Conversations indexed so far.
    pub done: u32,
    /// Conversations to index.
    pub total: u32,
}

/// One hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SearchHit {
    /// What matched.
    pub kind: SearchKind,
    /// The matter it belongs to.
    pub matter: MatterId,
    /// The matter's name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub matter_title: Option<String>,
    /// The conversation, for every kind but a matter title and a file name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thread: Option<ThreadId>,
    /// The conversation's title, when it has one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thread_title: Option<String>,
    /// For a prompt, an answer or a tool title: the `order` of its entry in the conversation's
    /// view (`ViewEntry`), which `PageEntries` pages by.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub order: Option<Seq>,
    /// For a file name: the document's path in the matter folder.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<String>,
    /// When the text was written.
    pub at: Timestamp,
    /// The text around the match, in runs; joined in order they are the snippet.
    pub snippet: Vec<SnippetPart>,
}

/// A run of a hit's snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SnippetPart {
    /// The text.
    pub text: String,
    /// Whether it matched a word of the query.
    pub hit: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_reads_with_its_defaults() {
        let request: SearchRequest = serde_json::from_str(
            r#"{"query":"lease ","scope":"all","include_archived":false,"generation":3}"#,
        )
        .unwrap();
        assert_eq!(request.scope, SearchScope::All);
        assert_eq!(request.kinds, None);
        assert_eq!(request.limit, None);
        assert_eq!(request.generation.get(), 3);
    }

    #[test]
    fn an_unknown_field_is_refused() {
        let refused = serde_json::from_str::<SearchRequest>(
            r#"{"query":"x","scope":"all","include_archived":false,"generation":1,"page":2}"#,
        );
        assert!(refused.is_err());
    }

    #[test]
    fn cancelled_is_a_bare_string() {
        assert_eq!(
            serde_json::to_string(&SearchAnswer::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }
}
