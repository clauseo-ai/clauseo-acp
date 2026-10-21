//! The `pins` check: every pinned tool version has one home, and a copy elsewhere either matches
//! it or is not there.
//!
//! - `Cargo.toml`'s `rust-version` (the root's and each member's) has the major.minor of
//!   `channel` in `rust-toolchain.toml`.
//! - A comment in `lefthook.yml` that names a lefthook version names the one `BINARIES` in
//!   `xtask/src/setup.rs` pins.
//! - No workflow in `.github/workflows/` and no action in `.github/actions/` pins a version of
//!   pnpm, Node, Rust or a tool `xtask/src/setup.rs` pins: no version input to the actions that
//!   install pnpm, Node or Rust, and no version where a value names the tool (`pnpm@<v>`,
//!   `node:<v>`, `rustup toolchain install <v>`, ...; the list is in `forms.rs`). Any version
//!   there fails, the home's or another, so a copy and a drifted copy both fail.
//!
//! It counts the files that can hold a copy and that it read: each manifest, `lefthook.yml`, each
//! workflow file and each action file.
//!
//! `engines.node` in `ui/package.json` takes only `>=<major>.<minor>.<patch>`: one minimum Node.
//! This is the one place that grammar is written down for Rust; `engines_node_minimum` applies it
//! here and in `xtask/src/buildtime/web.rs`, and `minNode` in `ui/app/scripts/buildtime-lib.mjs`
//! applies it in the same words (`ENGINES_NODE_RULE`). Anything else fails as a home that cannot
//! be read.
//!
//! Adding a home: a cargo tool in `TOOLS` or a binary in `BINARIES` needs nothing here, as the
//! forms read their names. A version kept in another file is a `FileHome` variant, which touches
//! its arms in `impl FileHome` (the compiler lists them), its reader and field in `homes.rs`, the
//! forms that name its tool in `forms.rs`, and one sample in each exhaustive test in `tests.rs`.

mod cargo;
mod fix;
mod forms;
mod homes;
mod lefthook;
mod workflows;
mod yaml;

pub(crate) use fix::Fix;

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
            "in {CARGO_TOML} and the members' manifests, {LEFTHOOK}, {}/ and {}/",
            workflows::DIR,
            workflows::ACTIONS
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
    inspect(ctx, TOOLS, BINARIES)
}

/// `run` with these `TOOLS` and `BINARIES`, so tests can pin versions of their own.
fn inspect(ctx: &Context, tools: &[Tool], binaries: &[Binary]) -> Inspection {
    let mut found = Inspection::default();
    let homes = homes::read(&ctx.root, &mut found);
    cargo::check(ctx, homes.rust.as_deref(), &mut found);
    lefthook::check(&ctx.root, binaries, &mut found);
    workflows::check(&ctx.root, tools, binaries, &mut found);
    found
}

/// A pinned version and the home it is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Home {
    /// A version kept in a file of the repo.
    File(FileHome),
    /// A cargo tool in `TOOLS`, by crate name.
    Tool(&'static str),
    /// A binary in `BINARIES`, by name.
    Binary(&'static str),
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
    /// The tool, as the end of "which ... to install".
    fn tool(self) -> &'static str {
        match self {
            FileHome::Rust => "Rust toolchain",
            FileHome::Pnpm => "pnpm",
            FileHome::Node => "Node",
        }
    }

    /// The version, as "the one home of ... is".
    fn noun(self) -> &'static str {
        match self {
            FileHome::Rust => "the Rust version",
            FileHome::Pnpm => "the pnpm version",
            FileHome::Node => "the minimum Node",
        }
    }

    /// Where the version is written.
    fn place(self) -> String {
        match self {
            FileHome::Rust => format!("`channel` in {TOOLCHAIN}"),
            FileHome::Pnpm => format!("`packageManager` in {PACKAGE_JSON}"),
            FileHome::Node => format!("`engines.node` in {PACKAGE_JSON}"),
        }
    }

    /// How a workflow installs the version from its home.
    fn install(self) -> String {
        match self {
            FileHome::Rust => format!(
                "a `run: rustup toolchain install` step with no argument installs the toolchain \
                 {TOOLCHAIN} names"
            ),
            FileHome::Pnpm => format!(
                "pnpm/action-setup with `package_json_file: {PACKAGE_JSON}` installs the pnpm \
                 `packageManager` names"
            ),
            FileHome::Node => format!(
                "actions/setup-node with `node-version-file: {PACKAGE_JSON}` installs a Node \
                 that `engines.node` allows"
            ),
        }
    }

    /// The fix of a version input to the action that installs the tool.
    fn input_fix(self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileHome::Rust => write!(
                f,
                "install Rust with a `run: rustup toolchain install` step instead: with no \
                 argument rustup installs the toolchain {TOOLCHAIN} names, the one home of the \
                 Rust version"
            ),
            FileHome::Pnpm => write!(
                f,
                "remove `version` and set `package_json_file: {PACKAGE_JSON}`: pnpm/action-setup \
                 then installs the pnpm `packageManager` names there, its one home"
            ),
            FileHome::Node => write!(
                f,
                "remove `node-version` and set `node-version-file: {PACKAGE_JSON}`: \
                 actions/setup-node then installs a Node that `engines.node` there allows, its \
                 one home"
            ),
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
                 it is the one home of the pnpm version, and pins looks for copies of it"
            ),
            FileHome::Node => write!(
                f,
                "set `\"engines\": {{ \"node\": \">=<major>.<minor>.<patch>\" }}` in \
                 {PACKAGE_JSON}: it is the one home of the minimum Node, and pins looks for \
                 copies of it"
            ),
        }
    }
}

impl Home {
    /// The tool, as the end of "names ...".
    fn tool(self) -> &'static str {
        match self {
            Home::File(home) => home.tool(),
            Home::Tool(name) | Home::Binary(name) => name,
        }
    }

    /// The version, as "the one home of ... is".
    fn noun(self) -> String {
        match self {
            Home::File(home) => home.noun().to_owned(),
            Home::Tool(name) | Home::Binary(name) => format!("the {name} version"),
        }
    }

    /// Where the version is written.
    fn place(self) -> String {
        match self {
            Home::File(home) => home.place(),
            Home::Tool(_) => format!("`TOOLS` in {SETUP}"),
            Home::Binary(_) => format!("`BINARIES` in {SETUP}"),
        }
    }

    /// How a workflow installs the version from its home.
    fn install(self) -> String {
        match self {
            Home::File(home) => home.install(),
            Home::Tool(_) | Home::Binary(_) => {
                format!("`cargo xtask setup --binaries` installs the versions {SETUP} pins")
            }
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
