//! Pins the text of every `pins::Fix` variant, and of each home and file home, with `fix_texts!`:
//! one arm per variant, and `assert_texts` or `assert_shown` fails until each arm has a sample, so
//! a new variant, `Home` or `FileHome` needs both.

use super::{FileHome, Fix, Home};
use crate::fix::tests::{assert_texts, fix_texts};

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

/// The texts of each home: what it is called, where its version is written, and the fix a
/// workflow copy gets. Its `Home::Tool` and `Home::Binary` arms take the name from the sample.
mod homes {
    use super::*;
    use crate::fix::tests::{assert_shown, fix_texts};

    fix_texts! {
        fn home_text for Home {
            Home::File(FileHome::Rust) => concat!(
                "Rust toolchain\n",
                "the Rust version\n",
                "`channel` in rust-toolchain.toml\n",
                "remove the version from this file: a `run: rustup toolchain install` step with no \
                 argument installs the toolchain rust-toolchain.toml names"
            ),
            Home::File(FileHome::Pnpm) => concat!(
                "pnpm\n",
                "the pnpm version\n",
                "`packageManager` in ui/package.json\n",
                "remove the version from this file: pnpm/action-setup with \
                 `package_json_file: ui/package.json` installs the pnpm `packageManager` names"
            ),
            Home::File(FileHome::Node) => concat!(
                "Node\n",
                "the minimum Node\n",
                "`engines.node` in ui/package.json\n",
                "remove the version from this file: actions/setup-node with \
                 `node-version-file: ui/package.json` installs a Node that `engines.node` allows"
            ),
            Home::Tool(name) => format!(
                "{name}\nthe {name} version\n`TOOLS` in xtask/src/setup.rs\n\
                 remove the version from this file: `cargo xtask setup --binaries` installs the \
                 versions xtask/src/setup.rs pins"
            ),
            Home::Binary(name) => format!(
                "{name}\nthe {name} version\n`BINARIES` in xtask/src/setup.rs\n\
                 remove the version from this file: `cargo xtask setup --binaries` installs the \
                 versions xtask/src/setup.rs pins"
            ),
        }
    }

    /// A home's four texts, one line each, as the messages and the fix line print them.
    fn shown(home: &Home) -> String {
        format!(
            "{}\n{}\n{}\n{}",
            home.tool(),
            home.noun(),
            home.place(),
            Fix::Copy(*home)
        )
    }

    #[test]
    fn every_home_has_its_exact_texts() {
        let homes = [
            Home::File(FileHome::Rust),
            Home::File(FileHome::Pnpm),
            Home::File(FileHome::Node),
            Home::Tool("cargo-fake"),
            Home::Binary("lefthook"),
        ];
        assert_shown(&homes, shown, home_text, ARMS);
    }
}

/// The texts of each file home's two fixes, a version input's and an unreadable home's, one line
/// each.
mod file_homes {
    use super::*;
    use crate::fix::tests::{assert_shown, fix_texts};

    fix_texts! {
        fn fixes_text for FileHome {
            FileHome::Rust => concat!(
                "install Rust with a `run: rustup toolchain install` step instead: with no \
                 argument rustup installs the toolchain rust-toolchain.toml names, the one home \
                 of the Rust version\n",
                "set `channel = \"<major>.<minor>.<patch>\"` under `[toolchain]` in \
                 rust-toolchain.toml: it is the one home of the Rust version, and pins compares \
                 its copies with it"
            ),
            FileHome::Pnpm => concat!(
                "remove `version` and set `package_json_file: ui/package.json`: pnpm/action-setup \
                 then installs the pnpm `packageManager` names there, its one home\n",
                "set `\"packageManager\": \"pnpm@<major>.<minor>.<patch>\"` in ui/package.json: \
                 it is the one home of the pnpm version, and pins looks for copies of it"
            ),
            FileHome::Node => concat!(
                "remove `node-version` and set `node-version-file: ui/package.json`: \
                 actions/setup-node then installs a Node that `engines.node` there allows, its \
                 one home\n",
                "set `\"engines\": { \"node\": \">=<major>.<minor>.<patch>\" }` in \
                 ui/package.json: it is the one home of the minimum Node, and pins looks for \
                 copies of it"
            ),
        }
    }

    #[test]
    fn every_file_home_has_its_exact_fixes() {
        // A file home's two fixes, one line each, as the fix lines print them.
        let shown = |home: &FileHome| format!("{}\n{}", Fix::Input(*home), Fix::Unread(*home));
        let homes = [FileHome::Rust, FileHome::Pnpm, FileHome::Node];
        assert_shown(&homes, shown, fixes_text, ARMS);
    }
}
