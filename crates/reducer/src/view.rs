//! What a subscriber is shown on (re)subscribing, and older entries for scrolling back.

use clauseo_protocol::{PageLimit, Seq, StreamHeader, ViewEntry, ViewWindow};

use crate::StreamState;

/// How many entries [`view_snapshot`] returns besides the header.
pub const VIEW_SNAPSHOT_ENTRIES: usize = 200;

/// The header and the last [`VIEW_SNAPSHOT_ENTRIES`] entries by `order`, oldest first, with
/// un-durable text (the screen shows it): what a `Reset` carries. Walks only those entries.
pub fn view_snapshot(state: &StreamState) -> ViewWindow {
    let header = match state {
        StreamState::Thread(thread) => thread.header.clone().map(StreamHeader::Thread),
        StreamState::Matter(matter) => matter.header.clone().map(StreamHeader::Matter),
    };
    let all = state.entries();
    let mut entries: Vec<ViewEntry> = Vec::with_capacity(VIEW_SNAPSHOT_ENTRIES.min(all.len()));
    entries.extend(all.values().rev().take(VIEW_SNAPSHOT_ENTRIES).cloned());
    entries.reverse();
    ViewWindow {
        header,
        entries,
        has_older: all.len() > VIEW_SNAPSHOT_ENTRIES,
    }
}

/// Up to `limit` entries with `order < before_order`, or the newest when `before_order` is
/// `None`, newest last: what `PageEntries` returns. The header is never among them.
pub fn page_entries(
    state: &StreamState,
    before_order: Option<Seq>,
    limit: PageLimit,
) -> Vec<ViewEntry> {
    let all = state.entries();
    let limit = usize::try_from(limit.get()).unwrap_or(usize::MAX);
    let mut page: Vec<ViewEntry> = Vec::with_capacity(limit.min(all.len()));
    match before_order {
        Some(before) => page.extend(
            all.range(..before)
                .rev()
                .take(limit)
                .map(|(_, entry)| entry.clone()),
        ),
        None => page.extend(all.values().rev().take(limit).cloned()),
    }
    page.reverse();
    page
}
