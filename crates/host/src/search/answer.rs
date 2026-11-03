//! A `SearchRequest` turned into the index's `Query`, and its `Page` into a `SearchPage`.

use clauseo_protocol::{
    DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT, SearchHit, SearchKind, SearchMode, SearchPage,
    SearchProgress, SearchRequest, SearchScope, SnippetPart, Timestamp,
};
use clauseo_search::{Cursor, Hit, Kind, Mode, Page, Query};

/// The query a request asks for, ranked from `now`. The mode follows the text's last character
/// (decision 0043); a cursor that does not decode is dropped, so the first page comes back.
pub(super) fn query(request: &SearchRequest, now: Timestamp) -> Query {
    let limit = request
        .limit
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .min(MAX_SEARCH_LIMIT);
    Query {
        text: request.query.clone(),
        matter: match request.scope {
            SearchScope::All => None,
            SearchScope::Matter(matter) => Some(matter),
        },
        kinds: request
            .kinds
            .as_ref()
            .map(|kinds| kinds.iter().filter_map(|kind| index_kind(*kind)).collect()),
        include_archived: request.include_archived,
        mode: Mode::for_input(&request.query),
        cursor: request
            .cursor
            .as_deref()
            .and_then(|text| Cursor::decode(text).ok()),
        limit: u8::try_from(limit).unwrap_or(u8::MAX),
        now,
    }
}

/// The page as the wire carries it.
pub(super) fn page(page: Page) -> SearchPage {
    SearchPage {
        hits: page.hits.into_iter().map(hit).collect(),
        next: page.next.map(|cursor| cursor.encode()),
        mode: match page.mode {
            Mode::Typing => SearchMode::NewestFirst,
            Mode::Ranked => SearchMode::Ranked,
        },
        indexing: page.indexing.map(|progress| SearchProgress {
            done: u32::try_from(progress.done).unwrap_or(u32::MAX),
            total: u32::try_from(progress.total).unwrap_or(u32::MAX),
        }),
    }
}

fn hit(hit: Hit) -> SearchHit {
    SearchHit {
        kind: wire_kind(hit.kind),
        matter: hit.matter,
        matter_title: hit.matter_title,
        thread: hit.thread,
        thread_title: hit.thread_title,
        order: hit.entry,
        path: hit.path,
        at: hit.at,
        snippet: hit
            .snippet
            .into_iter()
            .map(|part| SnippetPart {
                text: part.text,
                hit: part.hit,
            })
            .collect(),
    }
}

/// The index's kind for a wire kind; `None` for one this version does not know, which then
/// matches nothing.
const fn index_kind(kind: SearchKind) -> Option<Kind> {
    Some(match kind {
        SearchKind::MatterTitle => Kind::MatterTitle,
        SearchKind::ThreadTitle => Kind::ThreadTitle,
        SearchKind::Prompt => Kind::Prompt,
        SearchKind::Answer => Kind::Answer,
        SearchKind::ToolTitle => Kind::ToolTitle,
        SearchKind::FileName => Kind::FileName,
        SearchKind::Unknown => return None,
    })
}

/// The wire kind of a row; the reserved kinds are never indexed today.
const fn wire_kind(kind: Kind) -> SearchKind {
    match kind {
        Kind::MatterTitle => SearchKind::MatterTitle,
        Kind::ThreadTitle => SearchKind::ThreadTitle,
        Kind::Prompt => SearchKind::Prompt,
        Kind::Answer => SearchKind::Answer,
        Kind::ToolTitle => SearchKind::ToolTitle,
        Kind::FileName => SearchKind::FileName,
        Kind::PaperText | Kind::Thought => SearchKind::Unknown,
    }
}
