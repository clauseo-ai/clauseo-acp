//! `clauseo-counts`: the anonymous usage counts of research 37. The closed schema, the install
//! id, the day's cells, the weekly tally, the queue and the sender.
//!
//! # Owns
//!
//! - Schema 1 ([`schema`]): the day report, the weekly tally, the 18 events and every value they
//!   may hold. No I/O, no clock; it builds for `wasm32`, so S23's Worker validates with these
//!   types ([`schema::parse_batch`], [`schema::parse_tally`]) with `default-features = false`.
//! - Behind the default feature `client`: the install id, the open day's cells, the queue of
//!   closed days, the weekly tallies, their files under `<data_dir>/counts/`, the first-act
//!   marker `<data_dir>/first_act_done`, and the sender's schedule and bodies.
//!
//! # Interface
//!
//! - [`schema::Event`], one variant per event with its fields as closed types, and the payload
//!   types, each [`schema::Closed`].
//! - [`Counts`] (client): `open`, `bump`, `set_foreground`, `tick`, `due`, `sent`,
//!   `set_enabled`, `first_act`, `set_about`, and `consented`, `mode`, `today`,
//!   `queued_days`, `install_id`; with [`About`], [`Mode`], [`Post`], [`PostId`], [`Outcome`],
//!   and [`FIRST_ACT`], the marker's name.
//! - [`Upload`], [`UploadPath`], [`UploadError`] and [`send_due`] (client): the network seam,
//!   which desktop implements with `reqwest` and S22 reuses; [`os_seed`] seeds the send delays.
//!
//! # Invariants
//!
//! - No free text can reach a payload: every field is `Closed`, a sealed trait only this
//!   crate's enums, capped integers, buckets, days, weeks, versions, ids and error codes
//!   implement, and every read refuses an unknown field, name or number.
//! - Nothing is saved or sent before the first act: until then cells live in memory, and
//!   `<data_dir>/counts/` does not exist.
//! - Off (the switch or policy) deletes `<data_dir>/counts/`, forgets memory, and cancels a POST in
//!   flight without retrying it. On again makes a new id. The day report carries the install id;
//!   the tally never does, and has its own random id.
//! - Time and randomness are inputs: the crate reads no clock, and its send delays come from the
//!   seed given to `open`. Ids come from the OS's random source.
//! - Files change only through `write.rs`, and only `<data_dir>/first_act_done` and
//!   `<data_dir>/counts/`; never while the state's lock is held, so `bump` never waits on the
//!   disk, and an off deletes the folder after any write under way (`counts.rs`).
//!
//! # Depends on
//!
//! `protocol` (`ErrorCode`), `serde`, `serde_json`, `uuid`; with `client`, `flate2` (gzip) and
//! `futures` (the `Upload` future and the cancel signal). Never `matters`, `acp`, `reducer`,
//! `store`, `host`, `tauri`, `tokio` or `reqwest`.
//!
//! # Start reading
//!
//! `schema.rs` (with `schema/closed.rs`, `schema/events.rs` and `schema/report.rs`), then
//! `counts.rs`, `state.rs` (the day and the week), `sender.rs` (the schedule) and `upload.rs`.

pub mod schema;

#[cfg(feature = "client")]
mod about;
#[cfg(feature = "client")]
mod counts;
#[cfg(feature = "client")]
mod random;
#[cfg(feature = "client")]
mod sender;
#[cfg(feature = "client")]
mod state;
#[cfg(feature = "client")]
mod store;
#[cfg(feature = "client")]
mod upload;
#[cfg(feature = "client")]
mod write;

#[cfg(feature = "client")]
pub use about::About;
#[cfg(feature = "client")]
pub use counts::{Counts, Mode};
#[cfg(feature = "client")]
pub use random::os_seed;
#[cfg(feature = "client")]
pub use sender::{Cancelled, Outcome, Post, PostId};
#[cfg(feature = "client")]
pub use store::FIRST_ACT;
#[cfg(feature = "client")]
pub use upload::{Upload, UploadError, UploadPath, send_due};
