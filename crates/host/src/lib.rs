//! `clauseo-host`: the session host. One thread runs matters, threads, turns and agent
//! sessions, and serves commands, queries and subscriptions to every caller.
//!
//! # Owns
//!
//! - Its own thread, `clauseo-host`, and its executor: sessions, commands and their receipts,
//!   permission requests and elicitations, text coalescing and the rate cap, subscriptions, and
//!   the checkpoints before and after each turn. `docs/decisions/0011-host-turn-lifecycle.md`
//!   records the turn order and the reason for each choice.
//! - The next `seq` of every stream, the patch window and epoch of every loaded stream, and the
//!   sent view of what is in flight to the store.
//! - One `clauseo-matter` thread per open matter and one `clauseo-snapshot` thread.
//! - The Scratch matter: a first start creates it.
//! - The profile hook: [`ProfileHook`], with [`NoProfile`] as the default.
//!
//! # Interface
//!
//! - [`Host::start`] with a [`HostConfig`] ([`HostConfig::new`] takes the settings and the
//!   [`Launcher`](clauseo_process::Launcher); [`HostConfig::with_profile_hook`],
//!   [`HostConfig::with_refused_folders`], [`HostConfig::with_clock`] and
//!   [`HostConfig::with_ids`] set the rest). It returns once start-up is done: the store is
//!   open in `Effective::data_dir()`, the Scratch matter exists, and start-up reconciliation has
//!   committed. A second start on the same data folder fails with `store.locked`.
//! - [`HostHandle`], `Clone` and `Send`: [`HostHandle::dispatch`], [`HostHandle::query`],
//!   [`HostHandle::subscribe`], [`HostHandle::subscribe_events`], [`HostHandle::ack`],
//!   [`HostHandle::register_folder`], [`HostHandle::register_file`] and
//!   [`HostHandle::shutdown`]. Each call is one request over a `bounded(256)` channel with a
//!   `futures` oneshot for the reply. [`HostJoin::join`] waits for the thread to end.
//! - [`Subscription`] yields [`EncodedEvent`]s: each `HostEvent` serialised once and shared by
//!   every subscriber of the stream; [`EncodedEvent::json`] is sent on as it is and
//!   [`EncodedEvent::decode`] gives the type back. [`EventSubscription`] yields typed
//!   `HostEvent`s: the replay, one `CaughtUp`, then live `Events`.
//! - [`Error`] wraps the errors of `protocol`, `store`, `matters`, `process`, `acp`, `reducer`
//!   and `settings`, plus `Closed` and `Internal`; [`Error::code`] gives its `ErrorCode`, and
//!   every public call returns it as a `HostError`. After shutdown or a host panic, calls and
//!   `join` return `internal`.
//! - [`Clock`] ([`SystemClock`], [`Now`]) and [`IdSource`] ([`RandomV7`]) are the host's only
//!   sources of time and ids. The `test-support` feature adds `ManualClock`, `SequentialIds`,
//!   `HostConfig::with_patch_window_bytes`, `EncodedEvent::ptr_eq` and
//!   `HostHandle::abort_for_test`, which ends the thread with no flush and no graceful stop.
//! - Stage B's `limits` plugs in as a new actor input and publishes the existing
//!   `HostEvent::AccountLimitsChanged`.
//!
//! # Invariants
//!
//! - One actor on a `futures` `LocalPool`. Its loop awaits nothing but its input channel
//!   (requests, agent events, timers, matter, snapshot and store results); every wait runs as a
//!   spawned task that posts its result back. async-io's timers and readiness run on async-io's
//!   own thread.
//! - Committed before published: an event reaches a subscriber, and a command its answer, only
//!   after the commit holding it returned `Ok`. Text deltas are the one exception: they reach
//!   view subscribers in 16 ms display batches and are committed in 250 ms durable batches.
//! - `seq` is gapless per stream. Commits are pipelined (each takes `pending + 1` and is sent
//!   without waiting for the ones before it) and applied in `seq` order. After a failed commit
//!   nothing more is sent for that stream until its in-flight commits have returned, and no
//!   `seq` is reused.
//! - No clock read outside `SystemClock::now`; every delay runs on the host's [`Clock`].
//! - Agents start only through the [`Launcher`](clauseo_process::Launcher) in the config.
//! - No `tokio` and no `tauri`. Matter work runs on the `clauseo-matter` threads and zstd on
//!   `clauseo-snapshot`, never on the actor.
//!
//! # Depends on
//!
//! `protocol`, `reducer`, `store`, `matters`, `process`, `acp`, `settings`, `async-channel`,
//! `async-io`, `futures`, `uuid`, `serde_json`, `tracing` and `thiserror`. Never `tauri`,
//! `tokio` or `agents`.
//!
//! # Start reading
//!
//! `handle.rs` (the public calls and the subscription streams), `actor.rs` (the thread, the
//! loop and the methods the rest call), then `actor/`: `input.rs`, `startup.rs`, `commands.rs`,
//! `streams.rs`, `subs.rs` and the turn path in `turn.rs`. Then the leaf modules: `pipeline.rs`
//! and `sent.rs` (seq and the sent view), `view.rs`, `window.rs`, `encode.rs`, `coalesce.rs`,
//! `rate.rs`, `build.rs`, `agent.rs`, `matter_worker.rs`, `snapshot.rs`, `config.rs`,
//! `clock.rs`, `ids.rs` and `error.rs`.

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
mod file_link;
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
