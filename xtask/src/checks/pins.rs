//! The `pins` check: every pinned tool version has one home, and a copy elsewhere either matches
//! it or is not there.
//!
//! - `Cargo.toml`'s `rust-version` (the root's and each member's) has the major.minor of
//!   `channel` in `rust-toolchain.toml`.
//! - A comment in `lefthook.yml` that names a lefthook version names the one `BINARIES` in
//!   `xtask/src/setup.rs` pins.
//! - The pnpm version and the minimum Node are read from `ui/package.json`, so a home in another
//!   form fails.
//!
//! It counts the files that can hold a copy and that it read: each manifest and `lefthook.yml`.
//!
//! `engines.node` in `ui/package.json` takes only `>=<major>.<minor>.<patch>`: one minimum Node.
//! This is the one place that grammar is written down for Rust; `engines_node_minimum` applies it
//! here and in `xtask/src/buildtime/web.rs`, and `minNode` in `ui/app/scripts/buildtime-lib.mjs`
//! applies it in the same words (`ENGINES_NODE_RULE`). Anything else fails as a home that cannot
//! be read.
//!
//! Adding a home: a version kept in another file is a `FileHome` variant, which touches its arms
//! in `impl FileHome` (the compiler lists them), its reader in `homes.rs`, and one sample in
//! each exhaustive test in `fix/tests.rs`.

mod cargo;
mod fix;
mod homes;
mod lefthook;
mod yaml;

pub(crate) use fix::Fix;

use crate::checks::{Inspection, Noun, StaticCheck};
use crate::setup::{BINARIES, Binary};
use crate::workspace::Context;
use std::fmt;

/// Fails on a copy of a pinned version that differs from its home.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "pins",
    counts: Noun {
        one: "file that can copy a pin",
        many: "files that can copy a pin",
    },
    place: || format!("in {CARGO_TOML} and the members' manifests, and {LEFTHOOK}"),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// The workspace manifest, whose `rust-version` copies the toolchain's major.minor.
const CARGO_TOML: &str = "Cargo.toml";
/// The home of the Rust version.
const TOOLCHAIN: &str = "rust-toolchain.toml";
/// The home of the pnpm version (`packageManager`) and of the minimum Node (`engines.node`).
const PACKAGE_JSON: &str = "ui/package.json";
/// The lefthook config, whose comments may name the lefthook version they were checked against.
const LEFTHOOK: &str = "lefthook.yml";
/// The home of the pinned cargo tools (`TOOLS`) and binaries (`BINARIES`).
const SETUP: &str = "xtask/src/setup.rs";

/// The one form `engines.node` takes, in the words each reader of it fails with.
pub(crate) const ENGINES_NODE_RULE: &str = "engines.node takes only \">=<major>.<minor>.<patch>\"";

/// The minimum Node in an `engines.node` of the one form it takes: `1.2.3` of `>=1.2.3`. `None`
/// for anything else (`^1.2.3`, `>= 1.2.3`, `>=1.2`, `>=1.2.3 <2`), which `ENGINES_NODE_RULE`
/// names.
pub(crate) fn engines_node_minimum(field: &str) -> Option<&str> {
    let version = field.strip_prefix(">=")?;
    let parts: Vec<&str> = version.split('.').collect();
    let numbers = parts
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    (parts.len() == 3 && numbers).then_some(version)
}

fn run(ctx: &Context) -> Inspection {
    inspect(ctx, BINARIES)
}

/// `run` with these `BINARIES`, so tests can pin versions of their own.
fn inspect(ctx: &Context, binaries: &[Binary]) -> Inspection {
    let mut found = Inspection::default();
    let homes = homes::read(&ctx.root, &mut found);
    cargo::check(ctx, homes.rust.as_deref(), &mut found);
    lefthook::check(&ctx.root, binaries, &mut found);
    found
}

/// A version kept in a file of the repo, which `homes.rs` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileHome {
    /// `channel` in `rust-toolchain.toml`.
    Rust,
    /// `packageManager` in `ui/package.json`.
    Pnpm,
    /// `engines.node` in `ui/package.json`, a minimum.
    Node,
}

impl FileHome {
    /// The version, as "the one home of ... is".
    fn noun(self) -> &'static str {
        match self {
            FileHome::Rust => "the Rust version",
            FileHome::Pnpm => "the pnpm version",
            FileHome::Node => "the minimum Node",
        }
    }

    /// The fix of a home value that could not be read.
    fn unread_fix(self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileHome::Rust => write!(
                f,
                "set `channel = \"<major>.<minor>.<patch>\"` under `[toolchain]` in {TOOLCHAIN}: \
                 it is the one home of the Rust version, and pins compares its copies with it"
            ),
            FileHome::Pnpm => write!(
                f,
                "set `\"packageManager\": \"pnpm@<major>.<minor>.<patch>\"` in {PACKAGE_JSON}: \
                 it is the one home of the pnpm version, and pins reads it there"
            ),
            FileHome::Node => write!(
                f,
                "set `\"engines\": {{ \"node\": \">=<major>.<minor>.<patch>\" }}` in \
                 {PACKAGE_JSON}: it is the one home of the minimum Node, and pins reads it there"
            ),
        }
    }
}

/// True for a version of two or three dot-separated numbers: `1.2` or `1.2.3`.
fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The first two parts of a version: `1.2` of `1.2.3`, or the text itself with fewer parts.
fn major_minor(version: &str) -> &str {
    match version.match_indices('.').nth(1) {
        Some((at, _)) => version.get(..at).unwrap_or(version),
        None => version,
    }
}

#[cfg(test)]
mod tests;
