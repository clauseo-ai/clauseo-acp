//! The web records of a thread's log, folded into the tool call they belong to (0041): a
//! `WebSearchRecorded` sets the call's `web.search`, and a `PageVisited` appends to its
//! `web.pages`. Each is matched by `tool_call`. A record whose call has no entry is ignored: it
//! changes nothing and is no anomaly, as a log may hold a record before its call's start.

use clauseo_protocol::{
    PageVisited, ToolCallId, ToolCallPage, ToolCallSearch, ToolCallWeb, ViewEntry,
    WebSearchRecorded,
};

use crate::Applied;
use crate::change::ViewChange;
use crate::state::ThreadState;

/// Most pages a tool call keeps. A page past it is ignored; the view lists the first this many.
pub(crate) const MAX_PAGES_PER_CALL: usize = 50;

/// A search the call ran: it replaces the call's earlier search, if any.
pub(super) fn search_recorded(
    state: &mut ThreadState,
    event: &WebSearchRecorded,
    out: &mut Applied,
) {
    update_call(state, event.tool_call, out, |web| {
        let search = ToolCallSearch {
            query: event.query.clone(),
            results: event.results.clone(),
        };
        super::set_some(&mut web.search, &search)
    });
}

/// A page the call opened: appended after the pages before it, unless the call has
/// [`MAX_PAGES_PER_CALL`] already.
pub(super) fn page_visited(state: &mut ThreadState, event: &PageVisited, out: &mut Applied) {
    update_call(state, event.tool_call, out, |web| {
        if web.pages.len() >= MAX_PAGES_PER_CALL {
            return false;
        }
        web.pages.push(ToolCallPage {
            url: event.url.clone(),
            title: event.title.clone(),
            via: event.via,
        });
        true
    });
}

/// Applies `change` to the call's web and emits the call's `Upsert` when it changed. A call with
/// no entry is left alone. `change` says whether it changed anything.
fn update_call(
    state: &mut ThreadState,
    tool_call: ToolCallId,
    out: &mut Applied,
    change: impl FnOnce(&mut ToolCallWeb) -> bool,
) {
    let found = state.index.tool_calls.get(&tool_call).copied();
    let Some(ViewEntry::ToolCall(entry)) = found.and_then(|order| state.entries.get_mut(&order))
    else {
        return;
    };
    // A call with no web yet gets one only when the change sets something, so a no-op leaves
    // the entry as it was.
    let changed = if let Some(web) = entry.web.as_deref_mut() {
        change(web)
    } else {
        let mut web = ToolCallWeb::default();
        let changed = change(&mut web);
        if changed {
            entry.web = Some(Box::new(web));
        }
        changed
    };
    if changed {
        out.changes
            .push(ViewChange::Upsert(Box::new(ViewEntry::ToolCall(
                entry.clone(),
            ))));
    }
}
