//! The reducer's tests, in one test binary so an edit relinks one: the thread fold, message
//! text, the view, snapshots, and property tests over generated logs.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/events.rs"]
pub mod events;
#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;
#[cfg(test)]
#[path = "support/helpers.rs"]
pub mod helpers;

#[path = "reducer/asks_status.rs"]
mod asks_status;
#[path = "reducer/field_updates.rs"]
mod field_updates;
#[path = "reducer/fold.rs"]
mod fold;
#[path = "reducer/fold_edges.rs"]
mod fold_edges;
#[path = "reducer/orders.rs"]
mod orders;
#[path = "reducer/props.rs"]
mod props;
#[path = "reducer/queued.rs"]
mod queued;
#[path = "reducer/snapshot.rs"]
mod snapshot;
#[path = "reducer/stream_kinds.rs"]
mod stream_kinds;
#[path = "reducer/text.rs"]
mod text;
#[path = "reducer/view.rs"]
mod view;
