//! `clauseo-host`: the session host. Contract to be written by the lead; see the R11 design notes.

#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod actor;
mod agent;
mod blob_read;
mod build;
mod clock;
mod coalesce;
mod config;
mod encode;
mod error;
mod handle;
mod ids;
mod matter_worker;
mod pipeline;
mod principal;
mod rate;
mod sent;
mod snapshot;
mod undo_note;
mod view;
mod window;
mod write;

#[cfg(feature = "test-support")]
pub use clock::ManualClock;
pub use clock::{Clock, Now, SystemClock};
pub use config::{HostConfig, NoProfile, ProfileHook};
pub use encode::EncodedEvent;
pub use error::Error;
pub use handle::{EventSubscription, Host, HostHandle, HostJoin, Subscription};
#[cfg(feature = "test-support")]
pub use ids::SequentialIds;
pub use ids::{IdSource, RandomV7};
