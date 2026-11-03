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
//!   every `.git`, one walk per launch, and the link rule budget ([`LINK_RULES_MAX`],
//!   [`Policy::link_folders`]).
//! - Tier detection ([`detect`], cached; [`probe`], uncached): [`Tier`] and [`NoSandbox`].
//! - The denial types every stage C task uses ([`Denial`], [`Access`], [`PathClass`],
//!   [`HostRefusal`], [`Reachability`]), the per-launch [`DenialSink`], and the rate cap
//!   ([`CappedSink`]: at most [`DENIALS_PER_WINDOW`] per [`DENIAL_WINDOW`], the excess folded
//!   into one [`Denial::Dropped`]).
//! - [`Error`], with [`Error::code`] mapping each variant to a `clauseo_protocol::ErrorCode`.
//! - The egress proxy, one per agent launch ([`start_proxy`], [`ProxyHandle`]): CONNECT to 443
//!   and plain `http://` to 80 under an [`Allowlist`], a token on the agent listener, address
//!   checks, the firm's upstream, refusals as [`Denial::Host`]. [`ports::bind_loopback`] binds
//!   every per-launch loopback listener.
//! - Unix: the browser's socket folder ([`BrowserSocketDir`]), `/tmp/clauseo-ab-<id>`, made
//!   by the host for one launch, removed on drop, and swept at host start.
//!
//! # Interface
//!
//! - [`Policy::builder`]`(matter, state)`, then [`PolicyBuilder`]'s `runtime`, `read`,
//!   `allowed_folder`, `read_only_in_state`, `notes`, `credential_link`, `claude_store`,
//!   `hidden`, `proxy_port` (once per loopback listener), `loopback` ([`Loopback`]),
//!   `keychain`, `over_cap`, `links`, `scan` (a [`MatterScan`] the caller already has),
//!   `home`, and `build`. Accessors carry the builder's names, plus [`Policy::tmpdir`],
//!   [`Policy::read_only_in_matter`] ([`PathRule`]s), [`Policy::link_folders`] and
//!   [`Policy::links_outside_link_folders`]; [`Policy::claude_store`] gives a
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
//!   `spawn_in_tree`, with the agent's variables plus `TMPDIR=<state>/tmp` (made if missing) and
//!   nothing else: the launcher always adds `SpawnOptions::clear_env` to the options it is
//!   given, so no variable of the host's environment reaches the jail.
//!   Denials reach `denials` through a [`CappedSink`]. The first launch registers the launcher
//!   with the one host-wide watcher (`/usr/bin/log stream`, started with the first launcher
//!   and stopped with the last); dropping the launcher unregisters it. The tag, the sink and the
//!   registration belong to the launcher, shared by all its launches, so R17 makes one launcher
//!   per agent launch and keeps it alive for exactly that launch. R17 builds the policy and calls
//!   `launch` off the host's actor thread: `build` walks the matter, and a host's first launch
//!   waits up to 3 s for the watcher to start, which it has once it sees a probe denial of its
//!   own (`log stream` prints its header before the kernel's lines reach it). The watcher
//!   reports file-data reads and writes as [`Denial::Path`], the log's folds of repeats as
//!   [`Denial::Dropped`], and only logs network and device denials. A watcher that fails to
//!   start or dies is logged and started again at most once a minute; launches run either way.
//! - [`start_proxy`] with a [`ProxyConfig`] (lists, [`Upstream`], sink); the handle's ports,
//!   `env()`, `allow` and `revoke`. [`builtin_allowlist`] per [`LoginKind`].
//!   [`upstream_for`] routes the host's own HTTPS.
//! - Unix: [`BrowserSocketDir::create`] (a `flock`ed lock file beside it, then one atomic
//!   `mkdir` at 0700, checked with `lstat`), its `path`, `id` and `canonical`, and
//!   [`BrowserSocketDir::sweep_stale`], which skips a folder whose lock a live process holds.
//!   Drop it only after the launch's process tree has exited.
//!
//! # Invariants
//!
//! - A built [`Policy`] holds only canonical paths, and none of its refusals apply: no
//!   relative path; the matter, state and notes folders are not symbolic links; the matter is
//!   not `/`, does not hold the home folder and meets no hidden path; the state folder and the
//!   matter do not nest, and the state folder holds no hidden path; the notes folder does not
//!   meet the matter or hold a hidden path; the credential link is inside the state folder;
//!   runtime and read paths are not `/` and hold neither the home folder, the matter nor a
//!   hidden path; each allowed folder passes [`Policy::check_allowed_folder`]; every link
//!   finding is inside the matter, and the link rules are at most [`LINK_RULES_MAX`].
//! - The profile denies removing or re-creating the matter, state and notes folders themselves,
//!   and writing `CLAUDE.md` at any depth of the matter.
//! - The credential link and the Claude store's names are never resolved: only the link path
//!   enters the policy, never the lawyer's login path behind it.
//! - The walk never follows a link, and inside a `.git` folder looks only for hard links.
//! - Time is an input: the rate cap reads only its [`Clock`]; [`SystemClock::new`] is the one
//!   read of the real clock. A sink gets every denial that passes the cap, with no dedupe; the
//!   cap calls the inner sink with no lock held.
//! - Detection runs its probe at most once per process. The probe and the watcher's
//!   `/usr/bin/log stream` are the only processes this crate starts outside a launcher.
//! - The proxy decides by host name and never looks inside TLS; it connects only to addresses
//!   it checked, never resolving a name twice; its token never reaches a log, a `Debug` or the
//!   upstream.
//!
//! # Depends on
//!
//! `protocol` (error codes), `process` (launches and their errors), `thiserror` (`Error`),
//! `serde` (denials and findings as JSON), `tracing` (skipped folders, a failed probe, the
//! watcher), `dunce` (canonical paths without `\\?\`); the proxy's `async-executor`,
//! `async-io`, `futures`, `blocking`, `httparse`, `url`, `psl`, `getrandom`, `base64` and
//! `subtle`; on macOS, for the watcher, `async-process` (`log stream`) and `serde_json` (its
//! lines); on Unix, `rustix` (the open-file limit, and the browser socket folder's effective
//! user id, lock file and `flock`); on Windows, `socket2` and `windows` (the proxy's
//! listeners).
//!
//! # Start reading
//!
//! `policy.rs` (the builder and accessors), `policy/build.rs` (canonicalising and the
//! refusals), `links.rs`, `detect.rs`, `denial.rs`, then `error.rs`; for the proxy, `proxy.rs`
//! then `proxy/server.rs`.

#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

#[cfg(unix)]
mod browser_dir;
mod denial;
mod detect;
mod error;
mod links;
mod macos;
mod policy;
pub mod ports;
pub mod proxy;
#[cfg(test)]
mod test_support;

#[cfg(unix)]
pub use browser_dir::BrowserSocketDir;
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
pub use proxy::{
    Allowlist, Exchange, Listener, LoginKind, ProxyConfig, ProxyHandle, Record, Route, Upstream,
    Verdict, builtin_allowlist, start_proxy, upstream_for,
};
