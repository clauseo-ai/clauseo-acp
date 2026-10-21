//! Pins the text of every `pins::Fix` variant, and of each file home, with `fix_texts!`: one arm
//! per variant, and `assert_texts` or `assert_shown` fails until each arm has a sample, so a new
//! variant or `FileHome` needs both.

use super::{FileHome, Fix};
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
        Fix::Unread(FileHome::Rust),
        Fix::NotYaml,
        Fix::NotToml,
    ];
    assert_texts(&samples, pinned, ARMS);
}

/// The text of each file home's fix, an unreadable home's.
mod file_homes {
    use super::*;
    use crate::fix::tests::{assert_shown, fix_texts};

    fix_texts! {
        fn fixes_text for FileHome {
            FileHome::Rust => {
                "set `channel = \"<major>.<minor>.<patch>\"` under `[toolchain]` in \
                 rust-toolchain.toml: it is the one home of the Rust version, and pins compares \
                 its copies with it"
            },
            FileHome::Pnpm => {
                "set `\"packageManager\": \"pnpm@<major>.<minor>.<patch>\"` in ui/package.json: \
                 it is the one home of the pnpm version, and pins reads it there"
            },
            FileHome::Node => {
                "set `\"engines\": { \"node\": \">=<major>.<minor>.<patch>\" }` in \
                 ui/package.json: it is the one home of the minimum Node, and pins reads it there"
            },
        }
    }

    #[test]
    fn every_file_home_has_its_exact_fixes() {
        // A file home's fix, as the fix line prints it.
        let shown = |home: &FileHome| Fix::Unread(*home).to_string();
        let homes = [FileHome::Rust, FileHome::Pnpm, FileHome::Node];
        assert_shown(&homes, shown, fixes_text, ARMS);
    }
}
