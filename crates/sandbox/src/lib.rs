//! `clauseo-sandbox`: the agent jail. What an agent may read, write and reach, how strong a jail
//! this machine can give, and what the jail refused.
//!
//! # Owns
//!
//! - The policy ([`Policy`], built by [`Policy::builder`]): the matter and state folders, the
//!   runtime, read-only and allowed paths, the notes folder, the credential link, Claude's login
//!   store, hidden paths, loopback ports, the Keychain switch, and what is write-denied in the
//!   matter ([`PROTECTED_NAMES`], every `.git`, over-cap files). Every path in it is canonical
//!   and every refusal of [`PolicyBuilder::build`] holds. R17 decides which paths go in.
//! - The walk of the matter ([`scan_matter`], [`scan_links`]): links that lead outside it and
//!   every `.git`, one walk per launch.
//! - Tier detection ([`detect`], cached; [`probe`], uncached): [`Tier`] and [`NoSandbox`].
//! - The denial types every stage C task uses ([`Denial`], [`Access`], [`PathClass`],
//!   [`HostRefusal`], [`Reachability`]), the per-launch [`DenialSink`], and the rate cap
//!   ([`CappedSink`]: at most [`DENIALS_PER_WINDOW`] per [`DENIAL_WINDOW`], the excess folded
//!   into one [`Denial::Dropped`]).
//! - [`Error`], with [`Error::code`] mapping each variant to a `clauseo_protocol::ErrorCode`.
//!
//! # Interface
//!
//! - [`Policy::builder`]`(matter, state)`, then [`PolicyBuilder`]'s `runtime`, `read`,
//!   `allowed_folder`, `read_only_in_state`, `notes`, `credential_link`, `claude_store`,
//!   `hidden`, `proxy_port` (once per loopback listener), `loopback` ([`Loopback`]),
//!   `keychain`, `over_cap`, `links`, `scan` (a [`MatterScan`] the caller already has),
//!   `home`, and `build`. Accessors carry the builder's names, plus [`Policy::tmpdir`] and
//!   [`Policy::read_only_in_matter`] ([`PathRule`]s); [`Policy::claude_store`] gives a
//!   [`ClaudeStore`] with its allowed names ([`CLAUDE_STORE_FILES`],
//!   [`CLAUDE_STORE_LOCK_FOLDERS`], [`CLAUDE_STORE_BESIDE_LOCK_SUFFIX`]).
//! - [`Policy::check_allowed_folder`]: R18 runs it before a card; `build` runs it on each
//!   allowed folder. [`Policy::classify`]: the [`PathClass`] of a refused path, for a watcher.
//! - [`scan_links`]`(matter) -> Vec<`[`LinkFinding`]`>` ([`LinkKind`]), and [`scan_matter`],
//!   which also lists every `.git` and the folders `.git` files name.
//! - [`detect`] and [`probe`] return a [`Detection`]: tier A on macOS when Seatbelt applies a
//!   profile, C with [`NoSandbox::NotBuilt`] on Windows until S15, C with
//!   [`NoSandbox::UnsupportedOs`] elsewhere.
//! - [`DenialSink`]: `denied` for each denial, `upstream` for the proxy's reachability. `Arc` of
//!   a sink is a sink. [`CappedSink::new`] wraps one on the real clock, [`CappedSink::with_clock`]
//!   on any [`Clock`]; [`CappedSink::tick`] delivers a held count from the owner's timer.
//! - [`SeatbeltProfile::new`]`(policy, tag)` (every target): the profile text and its `-D`
//!   parameters; every policy path is a parameter, and the text holds only rules, port numbers
//!   and the tag. Rules: `src/macos/profile.rs` and `src/macos/base.sb`.
//! - macOS: [`SeatbeltLauncher::new`]`(policy, opts, denials)` is a `process::Launcher`. Each
//!   launch runs `/usr/bin/sandbox-exec -p <profile> -D NAME=path ... <program> <args>` through
//!   `spawn_in_tree`, with the agent's variables plus `TMPDIR=<state>/tmp` (made if missing).
//!   Denials reach `denials` through a [`CappedSink`]. The first launch registers the launcher
//!   with the one host-wide watcher (`/usr/bin/log stream`, started with the first launcher
//!   and stopped with the last); dropping the launcher unregisters it, so the host keeps the
//!   launcher alive while its agent runs. The watcher reports file-data reads and writes as
//!   [`Denial::Path`] and only logs network denials. A watcher that fails to start or dies is
//!   logged and started again at most once a minute; launches run either way.
//!
//! # Invariants
//!
//! - A built [`Policy`] holds only canonical paths, and none of its refusals apply: no
//!   relative path; the matter is not `/` or the home folder and meets no hidden path; the
//!   state folder and the matter do not nest; the notes folder and the credential link are not
//!   in the matter; each allowed folder passes [`Policy::check_allowed_folder`].
//! - The credential link and the Claude store's names are never resolved: only the link path
//!   enters the policy, never the lawyer's login path behind it.
//! - The walk never follows a link and never goes into a `.git` folder.
//! - Time is an input: the rate cap reads only its [`Clock`]; [`SystemClock::new`] is the one
//!   read of the real clock. A sink gets every denial that passes the cap, with no dedupe; the
//!   cap calls the inner sink with no lock held.
//! - Detection runs its probe at most once per process; the probe is the one process this
//!   crate starts outside a launcher.
//!
//! # Depends on
//!
//! `protocol` (error codes), `process` (launch errors), `serde` (denials and findings as JSON),
//! `tracing` (skipped folders, a failed probe), `dunce` (canonical paths without `\\?\`).
//!
//! # Start reading
//!
//! `policy.rs` (the builder and accessors), `policy/build.rs` (canonicalising and the
//! refusals), `links.rs`, `detect.rs`, `denial.rs`, then `error.rs`.

#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod denial;
mod detect;
mod error;
mod links;
mod macos;
mod policy;
#[cfg(test)]
mod test_support;

pub use denial::{
    Access, CappedSink, Clock, DENIAL_WINDOW, DENIALS_PER_WINDOW, Denial, DenialSink, HostRefusal,
    PathClass, Reachability, SystemClock,
};
pub use detect::{Detection, NoSandbox, Tier, detect, probe};
pub use error::Error;
pub use links::{LinkFinding, LinkKind, MatterScan, scan_links, scan_matter};
#[cfg(target_os = "macos")]
pub use macos::SeatbeltLauncher;
pub use macos::SeatbeltProfile;
pub use policy::{
    CLAUDE_STORE_BESIDE_LOCK_SUFFIX, CLAUDE_STORE_FILES, CLAUDE_STORE_LOCK_FOLDERS, ClaudeStore,
    INSTRUCTIONS_AT_ANY_DEPTH, LINK_RULES_MAX, Loopback, PROTECTED_NAMES, PathRule, Policy,
    PolicyBuilder,
};
