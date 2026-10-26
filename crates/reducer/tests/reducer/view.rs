//! What a subscriber is shown: the view snapshot and pages of older entries.

use clauseo_protocol::{PageLimit, Seq, StreamHeader, ViewEntry};
use clauseo_reducer::{VIEW_SNAPSHOT_ENTRIES, page_entries, view_snapshot};

use crate::fixtures::long_thread;
use crate::helpers::fold;

#[cfg(test)]
fn orders(entries: &[ViewEntry]) -> Vec<Seq> {
    entries.iter().map(ViewEntry::order).collect()
}

#[test]
fn view_snapshot_has_last_200() {
    let state = fold(&long_thread(800).unwrap());
    let all: Vec<Seq> = state.entries().keys().copied().collect();
    assert!(all.len() > VIEW_SNAPSHOT_ENTRIES, "{}", all.len());
    let window = view_snapshot(&state);
    assert_eq!(VIEW_SNAPSHOT_ENTRIES, 200);
    assert_eq!(window.entries.len(), 200);
    assert_eq!(orders(&window.entries), all[all.len() - 200..]);
    assert!(window.has_older);
    let header = state.as_thread().unwrap().header().unwrap().clone();
    assert_eq!(window.header, Some(StreamHeader::Thread(header)));

    // A short thread: every entry, nothing older.
    let short = fold(&long_thread(40).unwrap());
    let window = view_snapshot(&short);
    assert_eq!(window.entries.len(), short.entries().len());
    assert!(!window.has_older);
}

#[test]
fn page_entries_before_order() {
    let state = fold(&long_thread(300).unwrap());
    let all: Vec<Seq> = state.entries().keys().copied().collect();
    let before = all[40];
    let page = page_entries(&state, Some(before), PageLimit::new(25).unwrap());
    assert_eq!(orders(&page), all[15..40]);
    assert!(page.iter().all(|entry| entry.order() < before));
    // `before` need not be an entry's order: one past the 40th entry gives the same page plus it.
    let after = Seq::new(before.get() + 1).unwrap();
    let page = page_entries(&state, Some(after), PageLimit::new(25).unwrap());
    assert_eq!(orders(&page), all[16..41]);
    // Near the start the page is shorter than the limit.
    let page = page_entries(&state, Some(all[5]), PageLimit::new(25).unwrap());
    assert_eq!(orders(&page), all[..5]);
    let page = page_entries(&state, Some(all[0]), PageLimit::new(25).unwrap());
    assert!(page.is_empty(), "{page:?}");
}

#[test]
fn page_entries_newest_when_none() {
    let state = fold(&long_thread(300).unwrap());
    let all: Vec<Seq> = state.entries().keys().copied().collect();
    let page = page_entries(&state, None, PageLimit::new(30).unwrap());
    assert_eq!(orders(&page), all[all.len() - 30..]);
    let entries: Vec<ViewEntry> = state.entries().values().cloned().collect();
    assert_eq!(page, entries[entries.len() - 30..]);
    let limit = PageLimit::new(u32::try_from(all.len() + 10).unwrap()).unwrap();
    assert_eq!(page_entries(&state, None, limit).len(), all.len());
}
