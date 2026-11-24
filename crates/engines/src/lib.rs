//! Clauseo's engine list: `engines.json`, published with its Tauri updater signature
//! (`engines.json.sig`) on the GitHub release `engines`, names newer tested versions of the two
//! AI engines (npm packages). This crate decides whether a list is genuine and acceptable. No
//! I/O, no clock, no network: the app fetches and stores the two files, and keeps what it has
//! on any [`Error`].
//!
//! Contract: [`load`] returns an [`EngineList`] only when all of these hold, else the first
//! failing one's [`Error`] variant, checked in this order:
//! 1. `engines.json` is at most [`MAX_JSON`] bytes (before any parse) and the `.sig` at most
//!    [`MAX_SIGNATURE`].
//! 2. The `.sig` is what `tauri signer sign` writes: base64 of a minisign signature file, made
//!    prehashed (`ED`) over the exact `engines.json` bytes by the compiled-in updater key
//!    ([`updater_key`]; a release build's must be the release key, see [`RELEASE_KEY_ID`]);
//!    legacy (`Ed`) signatures are refused.
//! 3. Its trusted comment is exactly `timestamp:<digits>\tfile:engines.json`: the file name keeps
//!    an engines signature apart from the update archives the same key signs, and a comment with
//!    more (`--app-version` adds `\tversion:…`) is refused.
//! 4. The JSON has exactly the top-level fields of `clauseo-engines/1` (an unknown one is an
//!    error) with the right types; `generated` has the shape `YYYY-MM-DDTHH:MM:SSZ` (never
//!    compared with a clock); 1 ≤ `seq` ≤ [`MAX_SEQ`]; `apps` is non-empty; each `sizes` row
//!    names a known agent, OS and architecture with non-zero sizes, and no (agent, os, arch)
//!    repeats. A key repeated in a JSON object keeps its last value, as npm's `JSON.parse` does,
//!    and the caller stores the verified bytes, so both read the same list.
//! 5. Official names only: `package_json` has only the fields an engines pin file has (no
//!    `scripts`), its `dependencies` keys are exactly [`DEPENDENCIES`], its `overrides` keys
//!    are within [`OVERRIDES`], and every dependency and override is one plain semver version
//!    (no range, tag, `npm:` alias, git URL or `$` reference).
//! 6. The lock: `lockfileVersion` 3; its root's `dependencies` equal `package_json`'s; every
//!    other `packages` key is `node_modules/<name>(/node_modules/<name>)*` with valid npm
//!    names; no entry is a `link` or `inBundle`; each has exactly one sha512 `integrity` and a
//!    `resolved` that is exactly its own tarball on [`NPM_REGISTRY`]; each engine and override is
//!    installed at its pinned version.
//! 7. [`Context::app_version`] is one of `apps`, by exact string match (a dev build's
//!    `0.0.0-dev.<sha>` may be listed).
//! 8. Rollback: `seq` > max([`Context::bundled_seq`], [`Context::in_use_seq`]). A list equal to
//!    the bundle or to the list in use is [`Error::NotNewer`], as is an older one.
//! 9. Never older than the bundled pins: each of [`DEPENDENCIES`] and, when the bundle pins it,
//!    each of [`OVERRIDES`] is ≥ the one in [`Context::bundled_package_json`], which must itself
//!    be plain semver.
//!
//! No expiry: nothing here reads a clock, so an older signed list whose `seq` is still above
//! the caller's floor can be replayed (by whoever serves the release asset) until a newer list is
//! accepted; only accepting a new publish moves the floor. Rule 9 keeps such a replay from going
//! below what the app shipped with.
//!
//! `load_with_key` (cargo feature `test-support`, which only this crate's own dev-dependency
//! turns on) is the same check against a key the caller gives, for the tests.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::wildcard_enum_match_arm
)]

mod error;
mod keys;
mod list;
mod lock;
mod rules;
mod verify;

pub use error::Error;
pub use keys::{
    RELEASE_KEY_ID, RELEASE_MODE, TAURI_CONF, UPDATER_KEY_FROM_ENV, key_id, release_guard,
    updater_key,
};
pub use list::{Agent, Arch, EngineList, Os, Size};
pub use lock::NPM_REGISTRY;
pub use rules::{DEPENDENCIES, OVERRIDES};

/// The list's file name, as published, as stored and as its trusted comment names it.
pub const LIST_FILE: &str = "engines.json";

/// Its signature's file name.
pub const SIGNATURE_FILE: &str = "engines.json.sig";

/// The `format` field of this version of the list.
pub const FORMAT: &str = "clauseo-engines/1";

/// The largest `engines.json` read: 4 MiB, refused before parsing. A real one is about 70 KiB.
pub const MAX_JSON: usize = 4 * 1024 * 1024;

/// The largest `seq` accepted: 2^32. A publish adds one, so this is never reached; a larger one
/// is refused so a single list can't put the floor out of reach.
pub const MAX_SEQ: u64 = 1 << 32;

/// The largest `.sig` read: a real one is about 430 bytes.
pub const MAX_SIGNATURE: usize = 4096;

/// What the app knows when it checks a list.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The running app's version, as `tauri.conf.json` gives it.
    pub app_version: &'a str,
    /// The `seq` of the engines the app was built with (`crates/limits/tests/live/engines.seq`).
    pub bundled_seq: u64,
    /// The `package.json` the app was built with, holding the bundled pins.
    pub bundled_package_json: &'a serde_json::Value,
    /// The `seq` of the downloaded list in use, if one is.
    pub in_use_seq: Option<u64>,
}

/// Checks `json` (the `engines.json` bytes) and `sig` (the `engines.json.sig` bytes) against
/// the compiled-in updater key and the rules in the module docs.
pub fn load(json: &[u8], sig: &[u8], cx: &Context<'_>) -> Result<EngineList, Error> {
    let key = updater_key().ok_or(Error::NoKey)?;
    check(json, sig, cx, &key)
}

/// [`load`] against `key`, Tauri's form of a minisign public key (base64 of the `.pub` file,
/// what `tauri signer generate` writes to `<name>.key.pub`). Tests only.
#[cfg(feature = "test-support")]
pub fn load_with_key(
    json: &[u8],
    sig: &[u8],
    cx: &Context<'_>,
    key: &str,
) -> Result<EngineList, Error> {
    check(json, sig, cx, key)
}

/// The whole check against `key`.
fn check(json: &[u8], sig: &[u8], cx: &Context<'_>, key: &str) -> Result<EngineList, Error> {
    if json.len() > MAX_JSON {
        return Err(Error::TooBig);
    }
    verify::verify(json, sig, key)?;
    let list = list::parse(json)?;
    rules::check(&list, cx)?;
    Ok(list)
}
