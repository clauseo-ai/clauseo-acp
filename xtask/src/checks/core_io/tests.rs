//! The planted cases of the core-io check and its other tests. Each case builds a scratch git
//! repo with an `ARCHITECTURE.md` whose `protocol` row holds core I/O, a small root `clippy.toml`
//! and list, manifests that inherit the workspace lints, a library root with the forbid line, and
//! whatever other files the case needs. `tests/clippy.rs` runs clippy itself on the forms the
//! bans must catch.

mod clippy;
#[macro_use]
mod expected;
pub(super) mod planted;
mod repos;

use super::*;
use crate::checks::planted::Repo;
use crate::scratch::Scratch;
use repos::{Layout, expected, scratch};

#[test]
fn every_row_counts_when_no_member_is_held() {
    let dir = scratch("count", &Layout::default());
    let found = run(Repo::new(dir).ctx());
    assert_eq!(found, Inspection::new(3, Vec::new()));
}

#[test]
fn a_missing_table_inspects_nothing() {
    let dir = Scratch::new("core-io-no-table");
    assert_eq!(run(Repo::new(dir).ctx()), Inspection::default());
}

/// The Crates table rejects these names in a Never cell (`crates_table.rs`); `time` is the
/// crates.io crate.
#[test]
fn banned_module_names_are_std_and_its_io_modules() {
    for name in ["std", "fs", "net", "process", "env", "os"] {
        assert!(holds_banned_modules(name), "{name}");
    }
    for name in ["time", "io", "path", "thread", "tauri", "Fs", ""] {
        assert!(!holds_banned_modules(name), "{name}");
    }
}

/// The generated file parses back to the table it was rendered from.
#[test]
fn the_generated_file_parses_to_the_expected_table() {
    let expected = expected("parses");
    let parsed: toml::Table = toml::from_str(&expected.text).expect("the generated file parses");
    assert_eq!(parsed, expected.table);
}

/// The repo's own list and root file load, so the harness and every crate file can use them.
#[test]
fn the_repo_list_loads() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let expected = clippy_toml::load(&root).expect("the repo's clippy.toml and list load");
    let parsed: toml::Table = toml::from_str(&expected.text).expect("the generated file parses");
    assert_eq!(parsed, expected.table);
}
