//! Pins the text of every `pins::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::{FileHome, Fix, Home};
use crate::fix::tests::{assert_texts, fix_texts};
use std::collections::HashSet;

fix_texts! {
    fn pinned for Fix {
        Fix::RustVersion { .. } => {
            "set rust-version to \"3.4\", the major.minor of `channel` in rust-toolchain.toml, \
             the one home of the Rust version"
        },
        Fix::LefthookComment { .. } => {
            "check that the comment holds for lefthook 9.8.7, then make it name 9.8.7, the \
             version `BINARIES` in xtask/src/setup.rs pins"
        },
        Fix::Input(_) => {
            "remove `version` and set `package_json_file: ui/package.json`: pnpm/action-setup \
             then installs the pnpm `packageManager` names there, its one home"
        },
        Fix::Copy(_) => {
            "remove the version from this file: pnpm/action-setup with \
             `package_json_file: ui/package.json` installs the pnpm `packageManager` names"
        },
        Fix::Unread(_) => {
            "set `channel = \"<major>.<minor>.<patch>\"` under `[toolchain]` in \
             rust-toolchain.toml: it is the one home of the Rust version, and pins compares \
             its copies with it"
        },
        Fix::NotYaml => {
            "make the file valid YAML at this line: pins parses it to find the versions it \
             holds"
        },
        Fix::NotToml => {
            "make the manifest valid TOML at this line: pins reads its rust-version there"
        },
    }
}

/// The text of every fix: `fix_texts!` numbers the arms, and `assert_texts` fails until each
/// arm has a sample.
#[test]
fn every_fix_has_its_exact_text() {
    let samples = [
        Fix::RustVersion {
            channel_major_minor: "3.4".to_owned(),
        },
        Fix::LefthookComment {
            pinned: "9.8.7".to_owned(),
        },
        Fix::Input(FileHome::Pnpm),
        Fix::Copy(Home::File(FileHome::Pnpm)),
        Fix::Unread(FileHome::Rust),
        Fix::NotYaml,
        Fix::NotToml,
    ];
    assert_texts(&samples, pinned, ARMS);
}

/// What each home is called and how a workflow installs from it. The match is exhaustive, so a
/// new home needs an arm here, but the list of homes below is not checked against the enum: a
/// home missing from that list is not tested.
#[test]
fn every_home_has_its_exact_texts() {
    let homes = [
        Home::File(FileHome::Rust),
        Home::File(FileHome::Pnpm),
        Home::File(FileHome::Node),
        Home::Tool("cargo-fake"),
        Home::Binary("lefthook"),
    ];
    let unique: HashSet<String> = homes.iter().map(|home| format!("{home:?}")).collect();
    assert_eq!(unique.len(), homes.len(), "two samples are the same home");
    let setup = "`cargo xtask setup --binaries` installs the versions xtask/src/setup.rs pins";
    for home in homes {
        let (tool, noun, place, install) = match home {
            Home::File(FileHome::Rust) => (
                "Rust toolchain",
                "the Rust version",
                "`channel` in rust-toolchain.toml",
                "a `run: rustup toolchain install` step with no argument installs the toolchain \
                 rust-toolchain.toml names",
            ),
            Home::File(FileHome::Pnpm) => (
                "pnpm",
                "the pnpm version",
                "`packageManager` in ui/package.json",
                "pnpm/action-setup with `package_json_file: ui/package.json` installs the pnpm \
                 `packageManager` names",
            ),
            Home::File(FileHome::Node) => (
                "Node",
                "the minimum Node",
                "`engines.node` in ui/package.json",
                "actions/setup-node with `node-version-file: ui/package.json` installs a Node \
                 that `engines.node` allows",
            ),
            Home::Tool(_) => (
                "cargo-fake",
                "the cargo-fake version",
                "`TOOLS` in xtask/src/setup.rs",
                setup,
            ),
            Home::Binary(_) => (
                "lefthook",
                "the lefthook version",
                "`BINARIES` in xtask/src/setup.rs",
                setup,
            ),
        };
        assert_eq!(
            (home.tool(), home.noun(), home.place()),
            (tool, noun.to_owned(), place.to_owned())
        );
        assert_eq!(
            Fix::Copy(home).to_string(),
            format!("remove the version from this file: {install}")
        );
    }
}

/// The fixes of a version input and of a home that could not be read, for each file home in the
/// list below. As with `every_home_has_its_exact_texts`, a new `FileHome` is not forced into it.
#[test]
fn every_file_home_has_its_exact_fixes() {
    for home in [FileHome::Rust, FileHome::Pnpm, FileHome::Node] {
        let (input, unread) = match home {
            FileHome::Rust => (
                "install Rust with a `run: rustup toolchain install` step instead: with no \
                 argument rustup installs the toolchain rust-toolchain.toml names, the one home \
                 of the Rust version",
                "set `channel = \"<major>.<minor>.<patch>\"` under `[toolchain]` in \
                 rust-toolchain.toml: it is the one home of the Rust version, and pins compares \
                 its copies with it",
            ),
            FileHome::Pnpm => (
                "remove `version` and set `package_json_file: ui/package.json`: pnpm/action-setup \
                 then installs the pnpm `packageManager` names there, its one home",
                "set `\"packageManager\": \"pnpm@<major>.<minor>.<patch>\"` in ui/package.json: \
                 it is the one home of the pnpm version, and pins looks for copies of it",
            ),
            FileHome::Node => (
                "remove `node-version` and set `node-version-file: ui/package.json`: \
                 actions/setup-node then installs a Node that `engines.node` there allows, its \
                 one home",
                "set `\"engines\": { \"node\": \">=<major>.<minor>.<patch>\" }` in \
                 ui/package.json: it is the one home of the minimum Node, and pins looks for \
                 copies of it",
            ),
        };
        assert_eq!(
            (Fix::Input(home).to_string(), Fix::Unread(home).to_string()),
            (input.to_owned(), unread.to_owned())
        );
    }
}
