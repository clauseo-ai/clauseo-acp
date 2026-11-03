//! The tools a thread's header shows (`HeaderEntry::tools`, R21): web search and the browser,
//! each available or not and why. Laid over the header as it leaves the host, never logged: the
//! reducer builds the header from the log and knows nothing of it.
//!
//! Kept in this one module. Each session start works out its tools and holds them ([`hold`],
//! `turn/profile_loads.rs`); they show once `session/new`, `session/load` or `session/resume`
//! answers ([`show_held`]), so a session that never starts shows none. An archived thread's
//! entries are dropped ([`forget`]). The state lives here, per thread; the thread's loaded
//! `StreamView` holds a copy and lays it over every header upsert it publishes and every
//! `Reset` it builds ([`overlay`]), so a reconnecting window gets the current state. A stream
//! loaded later gets it from [`seed`].

use std::collections::BTreeMap;

use clauseo_protocol::{StreamHeader, StreamId, ThreadId, ToolState, ViewEntry, ViewPatchBody};
use clauseo_reducer::ViewChange;

use super::Actor;
use super::streams::StreamSlot;
use crate::view::StreamView;

/// Each thread's tool state: as last shown, and as held for a session call not answered yet.
#[derive(Debug, Default)]
pub(crate) struct HeaderTools {
    shown: BTreeMap<ThreadId, Vec<ToolState>>,
    held: BTreeMap<ThreadId, Vec<ToolState>>,
}

impl HeaderTools {
    /// Drops both entries of `thread`.
    fn forget(&mut self, thread: ThreadId) {
        self.shown.remove(&thread);
        self.held.remove(&thread);
    }

    /// The threads with an entry, shown or held.
    #[cfg(test)]
    fn threads(&self) -> std::collections::BTreeSet<ThreadId> {
        self.shown.keys().chain(self.held.keys()).copied().collect()
    }
}

/// Holds `tools` for `thread`'s session call: shown by [`show_held`] when it answers.
pub(crate) fn hold(actor: &mut Actor, thread: ThreadId, tools: Vec<ToolState>) {
    actor.header_tools.held.insert(thread, tools);
}

/// `thread`'s session call answered: what [`hold`] kept shows in its header.
pub(crate) fn show_held(actor: &mut Actor, thread: ThreadId) {
    if let Some(tools) = actor.header_tools.held.remove(&thread) {
        show(actor, thread, tools);
    }
}

/// Drops what is kept for `thread`, once it is archived: its loaded view keeps its copy until
/// the stream is released.
pub(crate) fn forget(actor: &mut Actor, thread: ThreadId) {
    actor.header_tools.forget(thread);
}

/// Shows `tools` in `thread`'s header: kept for the thread and, when its stream is loaded, laid
/// on its view and published to its subscribers as one header upsert. Nothing when it shows
/// that already.
fn show(actor: &mut Actor, thread: ThreadId, tools: Vec<ToolState>) {
    if actor.header_tools.shown.get(&thread) == Some(&tools) {
        return;
    }
    actor.header_tools.shown.insert(thread, tools.clone());
    let Some(StreamSlot::Loaded(loaded)) = actor.streams.get_mut(&StreamId::Thread(thread)) else {
        return;
    };
    loaded.view.set_tools(tools);
    let Some(header) = loaded
        .state
        .as_thread()
        .and_then(|state| state.header())
        .cloned()
    else {
        return;
    };
    let change = ViewChange::Upsert(Box::new(ViewEntry::Header(Box::new(header))));
    match loaded.view.publish(vec![change], &mut actor.encoder) {
        Ok(events) => {
            for event in &events {
                loaded.views.retain_mut(|sub| sub.offer(event));
            }
        }
        Err(error) => tracing::warn!(%thread, %error, "the header's tools were not published"),
    }
}

/// Lays the tools kept for `stream` on a view just built for it.
pub(crate) fn seed(actor: &Actor, stream: StreamId, view: &mut StreamView) {
    if let StreamId::Thread(thread) = stream
        && let Some(tools) = actor.header_tools.shown.get(&thread)
    {
        view.set_tools(tools.clone());
    }
}

/// `tools` over a thread header in `body`: an upsert of the header, or a `Reset`'s header.
/// Every other body is left as it is.
pub(crate) fn overlay(tools: &[ToolState], body: &mut ViewPatchBody) {
    match body {
        ViewPatchBody::Upsert { entry } => {
            if let ViewEntry::Header(header) = entry.as_mut() {
                header.tools = tools.to_vec();
            }
        }
        ViewPatchBody::Reset { view_window } => {
            if let Some(StreamHeader::Thread(header)) = view_window.header.as_mut() {
                header.tools = tools.to_vec();
            }
        }
        ViewPatchBody::Remove { .. } | ViewPatchBody::AppendText { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use clauseo_protocol::{ClauseoTool, ThreadId, ToolAvailability, ToolState};

    use super::HeaderTools;
    use crate::pipeline::tests::support::id;

    fn thread(n: u64) -> ThreadId {
        id::<ThreadId>(n)
    }

    /// An archived thread leaves nothing behind, shown or held; another thread's entries stay.
    #[test]
    fn forgetting_a_thread_prunes_both_its_entries() {
        let tools = vec![ToolState {
            tool: ClauseoTool::WebSearch,
            availability: ToolAvailability::Available,
        }];
        let mut kept = HeaderTools::default();
        kept.shown.insert(thread(1), tools.clone());
        kept.held.insert(thread(1), tools.clone());
        kept.shown.insert(thread(2), tools.clone());
        kept.held.insert(thread(3), tools);
        kept.forget(thread(1));
        assert_eq!(kept.threads(), [thread(2), thread(3)].into());
        kept.forget(thread(2));
        kept.forget(thread(3));
        assert!(kept.threads().is_empty());
    }
}
