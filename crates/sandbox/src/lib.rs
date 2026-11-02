//! `clauseo-sandbox`: the agent jail. What an agent may read, write and reach, how strong a jail
//! this machine can give, and what the jail refused.
//!
//! # Owns
//!
//! - The denial types every stage C task uses ([`Denial`], [`Access`], [`PathClass`],
//!   [`HostRefusal`], [`Reachability`]), the per-launch [`DenialSink`], and the rate cap
//!   ([`CappedSink`]: at most [`DENIALS_PER_WINDOW`] per [`DENIAL_WINDOW`], the excess folded
//!   into one [`Denial::Dropped`]).
//! - [`Error`], with [`Error::code`] mapping each variant to a `clauseo_protocol::ErrorCode`.
//!
//! # Interface
//!
//! - [`DenialSink`]: `denied` for each denial, `upstream` for the proxy's reachability. `Arc` of
//!   a sink is a sink. [`CappedSink::new`] wraps one on the real clock, [`CappedSink::with_clock`]
//!   on any [`Clock`]; [`CappedSink::tick`] delivers a held count from the owner's timer.
//!
//! # Invariants
//!
//! - Time is an input: the rate cap reads only its [`Clock`]; [`SystemClock::new`] is the one
//!   read of the real clock.
//! - A sink gets every denial that passes the cap, with no dedupe; the cap calls the inner sink
//!   with no lock held.
//!
//! # Depends on
//!
//! `protocol` (error codes), `process` (launch errors), `serde` (denials as JSON).
//!
//! # Start reading
//!
//! `denial.rs`, then `error.rs`.

#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod denial;
mod error;

pub use denial::{
    Access, CappedSink, Clock, DENIAL_WINDOW, DENIALS_PER_WINDOW, Denial, DenialSink, HostRefusal,
    PathClass, Reachability, SystemClock,
};
pub use error::Error;
