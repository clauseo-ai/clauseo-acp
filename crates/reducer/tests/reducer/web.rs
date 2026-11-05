//! The web records folded into their tool call (0041): a search sets the call's `web.search`,
//! pages append in order up to 50, a record for a call the thread does not hold changes nothing,
//! and a replay from the log gives the live view.

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentId, DomainEvent, EntryId, PageVia, PageVisited, ToolCallEntry, ToolCallId, ToolCallStatus,
    TurnId, ViewEntry, WebResult, WebSearchRecorded,
};
use clauseo_reducer::{Applied, StreamState, ViewChange, apply, view_snapshot};

use crate::events::{tool_started, tool_status};
use crate::fixtures::{Log, start_turn, thread_created};
use crate::helpers::{apply_all, fold, round_trip};

/// Most pages a call keeps (0041); the reducer's `MAX_PAGES_PER_CALL`.
const PAGE_CAP: usize = 50;

/// A thread with one turn and one running tool call: the log, the turn and the call.
#[cfg(test)]
fn running_call(n: u64) -> (Log, TurnId, ToolCallId) {
    let mut log = Log::thread(n).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Find the limitation period.").unwrap();
    let call: ToolCallId = log.fresh().unwrap();
    log.push(tool_started(turn, call, 1).unwrap()).unwrap();
    (log, turn, call)
}

/// A search for `query` whose one result carries a secret the log redacts.
#[cfg(test)]
fn search(turn: TurnId, call: ToolCallId, query: &str) -> WebSearchRecorded {
    WebSearchRecorded::new(
        turn,
        call,
        AgentId::new("claude-acp").unwrap(),
        query,
        vec![WebResult::new(
            "Limitations Act, 2002",
            "https://www.ontario.ca/laws/statute/02l24?token=abc",
        )],
    )
}

/// A visit to `https://example.com/<n>`.
#[cfg(test)]
fn visit(turn: TurnId, call: ToolCallId, url: &str) -> PageVisited {
    PageVisited::new(
        turn,
        call,
        url,
        Some(format!("Page {url}")),
        PageVia::Browser,
    )
}

/// Logs `event`, applies it to `state` as the host does, and returns what the apply gave.
#[cfg(test)]
fn push_apply(log: &mut Log, state: &mut StreamState, event: impl Into<DomainEvent>) -> Applied {
    let envelope = log.push(event).unwrap();
    apply(state, &envelope).unwrap()
}

/// The entry of `call` in `state`.
#[cfg(test)]
fn call_entry(state: &StreamState, call: ToolCallId) -> ToolCallEntry {
    state
        .entries()
        .values()
        .find_map(|entry| match entry {
            ViewEntry::ToolCall(found) if found.tool_call == call => Some(found.as_ref().clone()),
            _ => None,
        })
        .expect("the state holds the tool call")
}

/// The one change of `applied`, which must be the tool call's `Upsert`, as its entry.
#[cfg(test)]
fn sent_call(applied: &Applied) -> ToolCallEntry {
    let [ViewChange::Upsert(entry)] = applied.changes.as_slice() else {
        panic!("one upsert expected: {applied:?}");
    };
    match entry.as_ref() {
        ViewEntry::ToolCall(found) => found.as_ref().clone(),
        other => panic!("not a tool call: {other:?}"),
    }
}

/// A webview's entries by id, fed the changes the fold sent it, as `Model` in the property tests.
#[cfg(test)]
fn feed(view: &mut BTreeMap<EntryId, ViewEntry>, changes: &[ViewChange]) {
    for change in changes {
        match change {
            ViewChange::Upsert(entry) => {
                view.insert(entry.id(), (**entry).clone());
            }
            ViewChange::Remove(id) => {
                view.remove(id);
            }
            ViewChange::AppendText { .. } => {}
        }
    }
}

#[test]
fn a_search_fills_the_web_search_of_its_row() {
    let (mut log, turn, call) = running_call(60);
    let mut state = fold(log.events());
    let applied = push_apply(
        &mut log,
        &mut state,
        search(turn, call, "limitation period"),
    );
    // The row is sent again with its search; the URL went through the log's rule.
    let sent = sent_call(&applied);
    let web = sent.web.as_deref().expect("the row has its web");
    let found = web.search.as_ref().expect("the row has its search");
    assert_eq!(found.query, "limitation period");
    assert_eq!(
        found.results,
        vec![WebResult {
            title: "Limitations Act, 2002".to_owned(),
            url: "https://www.ontario.ca/laws/statute/02l24?token=REDACTED".to_owned(),
        }]
    );
    assert_eq!(web.pages, Vec::new());
    assert_eq!(call_entry(&state, call), sent);
}

#[test]
fn a_later_search_for_the_call_replaces_the_first_and_keeps_the_pages() {
    let (mut log, turn, call) = running_call(61);
    let mut state = fold(log.events());
    push_apply(&mut log, &mut state, search(turn, call, "first"));
    push_apply(
        &mut log,
        &mut state,
        visit(turn, call, "https://example.com/a"),
    );
    let applied = push_apply(&mut log, &mut state, search(turn, call, "second"));
    let sent = sent_call(&applied);
    let web = sent.web.as_deref().unwrap();
    assert_eq!(web.search.as_ref().unwrap().query, "second");
    assert_eq!(web.pages.len(), 1, "the page stays");
    assert_eq!(call_entry(&state, call), sent);
}

#[test]
fn pages_append_in_order_and_the_fifty_first_is_ignored() {
    let (mut log, turn, call) = running_call(62);
    let mut state = fold(log.events());
    // A page opened twice is listed twice, so the duplicate `0` is kept, in its place.
    let numbers: Vec<usize> = [0, 1, 0].into_iter().chain(3..PAGE_CAP).collect();
    let urls: Vec<String> = numbers
        .iter()
        .map(|n| format!("https://example.com/{n}"))
        .collect();
    for (i, url) in urls.iter().enumerate() {
        let applied = push_apply(&mut log, &mut state, visit(turn, call, url));
        assert_eq!(sent_call(&applied).web.unwrap().pages.len(), i + 1);
    }
    let before = call_entry(&state, call);
    let pages = &before.web.as_deref().unwrap().pages;
    assert_eq!(pages.len(), PAGE_CAP);
    let read: Vec<&str> = pages.iter().map(|page| page.url.as_str()).collect();
    let expected: Vec<&str> = urls.iter().map(String::as_str).collect();
    assert_eq!(read, expected);
    assert_eq!(
        pages[0].title.as_deref(),
        Some("Page https://example.com/0")
    );
    assert_eq!(pages[0].via, PageVia::Browser);

    // The 51st page changes nothing: no change, no entry change.
    let applied = push_apply(
        &mut log,
        &mut state,
        visit(turn, call, "https://example.com/99"),
    );
    assert_eq!(applied, Applied::default());
    assert_eq!(call_entry(&state, call), before);
}

#[test]
fn a_record_for_a_call_the_thread_does_not_hold_changes_nothing() {
    let (mut log, turn, _call) = running_call(63);
    let mut state = fold(log.events());
    let before = state.entries().clone();
    let stranger: ToolCallId = log.fresh().unwrap();
    let applied = push_apply(&mut log, &mut state, search(turn, stranger, "nobody"));
    assert_eq!(applied, Applied::default());
    let applied = push_apply(
        &mut log,
        &mut state,
        visit(turn, stranger, "https://example.com/x"),
    );
    assert_eq!(applied, Applied::default());
    assert_eq!(state.entries(), &before);
}

#[test]
fn a_replay_from_the_log_gives_the_live_view() {
    let (mut log, turn, call) = running_call(64);
    // The webview is fed every change from the thread's first event on.
    let mut state = StreamState::new(log.stream());
    let mut view = BTreeMap::new();
    for applied in apply_all(&mut state, log.events()) {
        feed(&mut view, &applied.changes);
    }
    let mut live = |log: &mut Log, state: &mut StreamState, event: DomainEvent| {
        let applied = push_apply(log, state, event);
        feed(&mut view, &applied.changes);
    };
    live(
        &mut log,
        &mut state,
        search(turn, call, "limitation period").into(),
    );
    live(
        &mut log,
        &mut state,
        visit(turn, call, "https://example.com/a").into(),
    );
    live(
        &mut log,
        &mut state,
        tool_status(call, ToolCallStatus::Completed),
    );
    live(
        &mut log,
        &mut state,
        visit(turn, call, "https://example.com/a").into(),
    );
    live(&mut log, &mut state, search(turn, call, "second").into());

    // Replaying the whole log from scratch gives the same state as the live fold, and the same
    // entries as the webview that was fed the changes.
    let replayed = fold(log.events());
    assert_eq!(replayed, state);
    assert_eq!(view_snapshot(&replayed), view_snapshot(&state));
    let entries = replayed.entries();
    assert_eq!(view.len(), entries.len() + 1, "every entry, and the header");
    for entry in entries.values() {
        assert_eq!(view.get(&entry.id()), Some(entry), "{:?}", entry.id());
    }
    let web = call_entry(&replayed, call).web.unwrap();
    assert_eq!(web.search.unwrap().query, "second");
    assert_eq!(web.pages.len(), 2);
    // A snapshot of the state reads back equal, web and all.
    assert_eq!(round_trip(&state), state);
}
