//! Clauseo's unsafe-site block list, v1 (ADR 0053): the published format's parse, the minisign
//! check against the compiled-in keys, host matching and the rollback rule. No I/O, no clock, no
//! network: the host fetches and stores the two files, and the egress proxy asks the list.
//!
//! Contract:
//! - [`load`] takes the two published files' bytes (`blocklist-v1.txt.gz` and its `.minisig`)
//!   and returns a [`BlockList`] only when the signature verifies against one of
//!   [`PUBLIC_KEYS`], the trusted comment is exactly `clauseo-blocklist v1 generated=<RFC 3339
//!   UTC, seconds> entries=<n>`, and the list parses: lowercase ASCII hosts, IP literals,
//!   sorted byte-wise by host with no duplicates, `<n>` lines, at most [`MAX_ENTRIES`] entries
//!   and [`MAX_TEXT`] bytes decompressed. Anything else is an [`Error`], and the caller keeps
//!   the list it has.
//! - [`BlockList::lookup`] matches a request host: equal to a listed host or under one, on
//!   label boundaries (`evil.example` covers `a.evil.example`, never `notevil.example`). An IP
//!   literal matches only its own entry, exactly; an IPv6 address that carries an IPv4 address
//!   (mapped, translated, compatible, NAT64, 6to4) also matches that IPv4 address's entry.
//!   It costs one hash probe per label and allocates only to lowercase or to print an address.
//! - [`accept`] is the rollback rule: a list replaces the one in use only when it is newer than
//!   it, and never when it is older than the bundled snapshot.
//! - [`Current`] is the list in use, shared read-only: readers clone an `Arc` under a read lock
//!   held for the clone only, and an update swaps the `Arc`, so a reader never waits on a parse
//!   and a swap never touches a request in flight.
//! - [`Allowed`] is one launch's set of hosts the lawyer allowed despite the list (PR 2),
//!   matched exactly and shared the same way; the proxy asks it before the list.
//!
//! Signature checking stays here and in the host; the proxy only receives a parsed list.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::wildcard_enum_match_arm
)]

mod allowed;
mod current;
mod error;
mod generated;
mod keys;
mod list;
mod parse;
mod verify;

pub use allowed::{Allowed, Watcher};
pub use current::Current;
pub use error::Error;
pub use generated::{Generated, accept};
pub use keys::{PUBLIC_KEYS, STAGE_F_KEY_FILE, key_line};
pub use list::{BlockList, Hit, Tag};
pub use parse::{MAX_ENTRIES, MAX_TEXT, parse_gz, parse_plain};
pub use verify::{Header, MAX_SIGNATURE, load, trusted_comment, verify};

/// The list's file name, as published and as stored.
pub const LIST_FILE: &str = "blocklist-v1.txt.gz";

/// Its signature's file name.
pub const SIGNATURE_FILE: &str = "blocklist-v1.txt.gz.minisig";
