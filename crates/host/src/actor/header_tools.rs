//! The tools a thread's header shows (`HeaderEntry::tools`, R21): web search and the browser,
//! each available or not and why. Laid over the header as it leaves the host, never logged: the
//! reducer builds the header from the log and knows nothing of it.
//!
//! Kept in this one module, with one entry point, [`show`], called at each session start
//! (`turn/profile_loads.rs`). The state lives here, per thread; the thread's loaded
//! `StreamView` holds a copy and lays it over every header upsert it publishes and every
//! `Reset` it builds ([`overlay`]), so a reconnecting window gets the current state. A stream
//! loaded later gets it from [`seed`].

use std::collections::BTreeMap;

use clauseo_protocol::{StreamHeader, StreamId, ThreadId, ToolState, ViewEntry, ViewPatchBody};
use clauseo_reducer::ViewChange;

use super::Actor;
use super::streams::StreamSlot;
use crate::view::StreamView;

/// Each thread's tool state, as last shown.
#[derive(Debug, Default)]
pub(crate) struct HeaderTools(BTreeMap<ThreadId, Vec<ToolState>>);

/// Shows `tools` in `thread`'s header: kept for the thread and, when its stream is loaded, laid
/// on its view and published to its subscribers as one header upsert. Nothing when it shows
/// that already.
pub(crate) fn show(actor: &mut Actor, thread: ThreadId, tools: Vec<ToolState>) {
    if actor.header_tools.0.get(&thread) == Some(&tools) {
        return;
    }
    actor.header_tools.0.insert(thread, tools.clone());
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
    let change = ViewChange::Upsert(Box::new(ViewEntry::Header(header)));
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
        && let Some(tools) = actor.header_tools.0.get(&thread)
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
