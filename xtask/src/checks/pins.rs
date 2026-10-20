//! The `pins` check: every pinned tool version has one home, and a copy elsewhere either matches
//! it or is not there.
//!
//! - `Cargo.toml`'s `rust-version` (the root's and each member's) has the major.minor of
//!   `channel` in `rust-toolchain.toml`.
//! - A comment in `lefthook.yml` that names a lefthook version names the one `BINARIES` in
//!   `xtask/src/setup.rs` pins.
//! - No workflow in `.github/workflows/` pins pnpm, Node or Rust: no `version` input to
//!   `pnpm/action-setup`, no `node-version` input to `actions/setup-node`, no toolchain input,
//!   ref or ref comment of a Rust toolchain action, and no value that holds a version a home pins.
//!
//! It counts the files that can hold a copy and that it read: each manifest, `lefthook.yml` and
//! each workflow file.

mod cargo;
mod homes;
mod lefthook;
mod workflows;
mod yaml;

use crate::checks::{Inspection, Noun, StaticCheck};
use crate::setup::{BINARIES, Binary, TOOLS, Tool};
use crate::workspace::Context;
use std::fmt;

/// Fails on a copy of a pinned version that differs from its home, or that a workflow holds.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "pins",
    counts: Noun {
        one: "file that can copy a pin",
        many: "files that can copy a pin",
    },
    place: || {
        format!(
            "in {CARGO_TOML} and the members' manifests, {LEFTHOOK} and {}/",
            workflows::DIR
        )
    },
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

fn run(ctx: &Context) -> Inspection {
    inspect(ctx, TOOLS, BINARIES)
}

/// `run` with these `TOOLS` and `BINARIES`, so tests can pin versions of their own.
fn inspect(ctx: &Context, tools: &[Tool], binaries: &[Binary]) -> Inspection {
    let mut found = Inspection::default();
    let homes = homes::read(&ctx.root, &mut found);
    cargo::check(ctx, homes.rust.as_deref(), &mut found);
    lefthook::check(&ctx.root, binaries, &mut found);
    workflows::check(&ctx.root, &homes, tools, binaries, &mut found);
    found
}

/// A pinned version and the home it is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Home {
    /// `channel` in `rust-toolchain.toml`.
    Rust,
    /// `packageManager` in `ui/package.json`.
    Pnpm,
    /// `engines.node` in `ui/package.json`, a minimum.
    Node,
    /// A cargo tool in `TOOLS`, by crate name.
    Tool(&'static str),
    /// A binary in `BINARIES`, by name.
    Binary(&'static str),
}

impl fmt::Display for Home {
    /// What the version is, as the end of `` `1.2.3` is ``.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Home::Rust => write!(f, "the Rust version `channel` in {TOOLCHAIN} pins"),
            Home::Pnpm => write!(
                f,
                "the pnpm version `packageManager` in {PACKAGE_JSON} pins"
            ),
            Home::Node => write!(f, "the minimum Node `engines.node` in {PACKAGE_JSON} sets"),
            Home::Tool(name) => write!(f, "the {name} version `TOOLS` in {SETUP} pins"),
            Home::Binary(name) => write!(f, "the {name} version `BINARIES` in {SETUP} pins"),
        }
    }
}

/// A home value `pins` could not read, so it compares no copy with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unread {
    /// `channel` in `rust-toolchain.toml`.
    Channel,
    /// `packageManager` in `ui/package.json`.
    PackageManager,
    /// `engines.node` in `ui/package.json`.
    EnginesNode,
}

/// The fix line of each failure `pins` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A `rust-version` whose major.minor is not the toolchain's.
    RustVersion { channel_major_minor: String },
    /// A `lefthook.yml` comment that names another lefthook version than `BINARIES`.
    LefthookComment { pinned: String },
    /// A `version` input to `pnpm/action-setup`.
    PnpmInput,
    /// A `node-version` input to `actions/setup-node`.
    NodeInput,
    /// A Rust toolchain action told which toolchain to install.
    RustToolchain,
    /// A workflow value that holds a version a home pins.
    Copy(Home),
    /// A home value that could not be read.
    Unread(Unread),
    /// A YAML file that does not parse.
    NotYaml,
    /// A manifest that does not parse.
    NotToml,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::RustVersion {
                channel_major_minor,
            } => write!(
                f,
                "set rust-version to \"{channel_major_minor}\", the major.minor of `channel` in \
                 {TOOLCHAIN}, the one home of the Rust version"
            ),
            Fix::LefthookComment { pinned } => write!(
                f,
                "check that the comment holds for lefthook {pinned}, then make it name {pinned}, \
                 the version `BINARIES` in {SETUP} pins"
            ),
            Fix::PnpmInput => write!(
                f,
                "remove `version` and set `package_json_file: {PACKAGE_JSON}`: pnpm/action-setup \
                 then installs the pnpm `packageManager` names there, its one home"
            ),
            Fix::NodeInput => write!(
                f,
                "remove `node-version` and set `node-version-file: {PACKAGE_JSON}`: \
                 actions/setup-node then installs a Node that `engines.node` there allows, its \
                 one home"
            ),
            Fix::RustToolchain => write!(
                f,
                "install Rust with a `run: rustup toolchain install` step instead: with no \
                 argument rustup installs the toolchain {TOOLCHAIN} names, the one home of the \
                 Rust version"
            ),
            Fix::Copy(home) => copy_fix(f, *home),
            Fix::Unread(unread) => unread_fix(f, *unread),
            Fix::NotYaml => write!(
                f,
                "make the file valid YAML at this line: pins parses it to find the versions it \
                 holds"
            ),
            Fix::NotToml => write!(
                f,
                "make the manifest valid TOML at this line: pins reads its rust-version there"
            ),
        }
    }
}

/// The fix of a workflow value that holds the version `home` pins.
fn copy_fix(f: &mut fmt::Formatter<'_>, home: Home) -> fmt::Result {
    let how = match home {
        Home::Rust => format!(
            "a `run: rustup toolchain install` step with no argument installs the toolchain \
             {TOOLCHAIN} names"
        ),
        Home::Pnpm => format!(
            "pnpm/action-setup with `package_json_file: {PACKAGE_JSON}` installs the pnpm \
             `packageManager` names"
        ),
        Home::Node => format!(
            "actions/setup-node with `node-version-file: {PACKAGE_JSON}` installs a Node that \
             `engines.node` allows"
        ),
        Home::Tool(_) | Home::Binary(_) => {
            format!("`cargo xtask setup --binaries` installs the versions {SETUP} pins")
        }
    };
    write!(f, "remove the version from the workflow: {how}")
}

/// The fix of a home value that could not be read.
fn unread_fix(f: &mut fmt::Formatter<'_>, unread: Unread) -> fmt::Result {
    match unread {
        Unread::Channel => write!(
            f,
            "set `channel = \"<major>.<minor>.<patch>\"` under `[toolchain]` in {TOOLCHAIN}: it \
             is the one home of the Rust version, and pins compares its copies with it"
        ),
        Unread::PackageManager => write!(
            f,
            "set `\"packageManager\": \"pnpm@<major>.<minor>.<patch>\"` in {PACKAGE_JSON}: it is \
             the one home of the pnpm version, and pins looks for copies of it"
        ),
        Unread::EnginesNode => write!(
            f,
            "set `\"engines\": {{ \"node\": \">=<major>.<minor>.<patch>\" }}` in {PACKAGE_JSON}: \
             it is the one home of the minimum Node, and pins looks for copies of it"
        ),
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
