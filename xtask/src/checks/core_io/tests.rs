//! The planted cases of the core-io check and its other tests. Each case builds a scratch git
//! repo with an `ARCHITECTURE.md` whose `protocol` row holds core I/O, a small root `clippy.toml`
//! and list, manifests that inherit the workspace lints, and whatever other files the case needs.
//! `tests/clippy.rs` runs clippy itself on the forms the bans must catch.

mod cases;
mod clippy;
pub(super) mod planted;

use cases::*;

use super::*;
use crate::process::{self, Cmd, Program};
use crate::scratch::Scratch;
use std::fs;

/// A Crates table with one held row (`protocol`, line 8), one that is not (`xtask`) and a
/// planned held row (`ids`).
const ARCHITECTURE: &str = "# Architecture\n\n## Crates\n\n\
    | Crate | Owns | May depend on | Never depends on | Status |\n\
    |---|---|---|---|---|\n\
    | `xtask` | checks | none of ours | tauri | exists |\n\
    | `protocol` | events | serde | tauri, fs, process | exists |\n\
    | `ids` | ids | serde | any I/O | planned |\n";

/// The scratch repo's root `clippy.toml`.
const ROOT_TOML: &str = "# root\nmsrv = \"1.99\"\ndisallowed-methods = [\n  \
    { path = \"std::time::Instant::now\", reason = \"the core takes time as input\" },\n]\n";

/// The scratch repo's list.
const LIST_TOML: &str = "disallowed-methods = [\n  \
    { path = \"std::fs::read\", reason = \"no files\" },\n]\n\
    disallowed-types = [\n  { path = \"std::fs::File\", reason = \"no files\", \
    allow-invalid = true },\n]\n";

/// The scratch repo's root `Cargo.toml`.
const ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"crates/*\"]\n\n\
    [workspace.lints.clippy]\ndisallowed_methods = \"deny\"\n";

/// The scratch repo's `crates/protocol/Cargo.toml`.
const CRATE_MANIFEST: &str =
    "[package]\nname = \"clauseo-protocol\"\n\n[lints]\nworkspace = true\n";

/// The inputs of one scratch repo.
#[derive(Default)]
struct Repo<'a> {
    /// `crates/protocol/clippy.toml`, if any.
    crate_file: Option<&'a str>,
    /// More files, written last, so one can replace a manifest: (repo-relative path, text).
    files: &'a [(&'a str, &'a str)],
    /// `ARCHITECTURE.md` in place of `ARCHITECTURE`.
    architecture: Option<&'a str>,
    /// The root `clippy.toml`, `ROOT_TOML` by default.
    root: Input<'a>,
    /// The list, `LIST_TOML` by default.
    list: Input<'a>,
}

/// One input file of a scratch repo.
#[derive(Default, Clone, Copy)]
enum Input<'a> {
    /// The scratch repo's usual text.
    #[default]
    Usual,
    /// No file.
    Missing,
    /// This text.
    Text(&'a str),
}

impl<'a> Input<'a> {
    /// The text to write, if any: `usual` for `Usual`.
    fn text(self, usual: &'a str) -> Option<&'a str> {
        match self {
            Input::Usual => Some(usual),
            Input::Missing => None,
            Input::Text(text) => Some(text),
        }
    }
}

/// Writes `repo` into a scratch git repo named after `case`; returns the dir.
fn scratch(case: &str, repo: &Repo<'_>) -> Scratch {
    let dir = Scratch::new(&format!("core-io-{case}"));
    let init = Cmd::new(Program::Git, ["init", "-q"]);
    let quiet = process::run_quiet(&init, &dir).expect("run git init");
    assert!(quiet.success, "git init failed: {}", quiet.stderr);
    let architecture = repo.architecture.unwrap_or(ARCHITECTURE);
    fs::write(dir.join(crates_table::FILE), architecture).expect("write ARCHITECTURE.md");
    fs::create_dir_all(dir.join("crates/protocol")).expect("create the crate dir");
    fs::create_dir_all(dir.join("xtask")).expect("create xtask/");
    if let Some(text) = repo.root.text(ROOT_TOML) {
        fs::write(dir.join(clippy_toml::ROOT), text).expect("write the root clippy.toml");
    }
    if let Some(text) = repo.list.text(LIST_TOML) {
        fs::write(dir.join(clippy_toml::LIST), text).expect("write the list");
    }
    if let Some(text) = repo.crate_file {
        fs::write(dir.join("crates/protocol/clippy.toml"), text).expect("write clippy.toml");
    }
    let defaults = [
        (lints::ROOT_MANIFEST, ROOT_MANIFEST),
        ("crates/protocol/Cargo.toml", CRATE_MANIFEST),
    ];
    for (path, text) in defaults.iter().chain(repo.files) {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().expect("a file has a parent")).expect("create dirs");
        fs::write(path, text).expect("write a file");
    }
    dir
}

/// The workspace members of every case: `clauseo-protocol` at `crates/protocol`, and
/// `clauseo-extra`, which has no row.
fn members() -> Vec<Member> {
    ["protocol", "extra"]
        .into_iter()
        .map(|name| Member {
            name: format!("clauseo-{name}"),
            dir: format!("crates/{name}"),
            path_deps: Vec::new(),
        })
        .collect()
}

/// Runs the check on a scratch repo holding `repo`.
fn run_repo(case: &str, repo: &Repo<'_>) -> Inspection {
    let dir = scratch(case, repo);
    run(&Context {
        root: dir.to_path_buf(),
        members: members(),
        dependencies: Default::default(),
        load_resolved_graph: false,
    })
}

/// Runs the check with `crate_file` as `crates/protocol/clippy.toml`.
fn with_crate_file(case: &str, crate_file: &str) -> Inspection {
    run_repo(
        case,
        &Repo {
            crate_file: Some(crate_file),
            ..Repo::default()
        },
    )
}

/// What `crates/protocol/clippy.toml` must be in the scratch repo, built in a scratch repo of
/// its own named after `case`.
fn expected(case: &str) -> Expected {
    let dir = scratch(&format!("{case}-expected"), &Repo::default());
    clippy_toml::load(&dir).expect("the scratch inputs load")
}

pub(super) fn missing_crate_file() -> Inspection {
    run_repo("missing-crate-file", &Repo::default())
}

pub(super) fn crate_file_differs() -> Inspection {
    with_crate_file("crate-file-differs", ROOT_TOML)
}

pub(super) fn crate_file_matches() -> Inspection {
    with_crate_file("crate-file-matches", &expected("crate-file-matches").text)
}

/// The same tables as the generated file, written as arrays of tables, in another key order
/// and with comments.
pub(super) fn crate_file_reformatted() -> Inspection {
    with_crate_file(
        "crate-file-reformatted",
        "# written by hand\nmsrv = \"1.99\"\n\
         [[disallowed-types]]\nallow-invalid = true\nreason = \"no files\"\npath = \"std::fs::File\"\n\n\
         [[disallowed-methods]]\npath = \"std::time::Instant::now\"\n\
         reason = \"the core takes time as input\"\n\n\
         [[disallowed-methods]]\npath = \"std::fs::read\" # the list's\nreason = \"no files\"\n",
    )
}

pub(super) fn crate_file_not_toml() -> Inspection {
    with_crate_file(
        "crate-file-not-toml",
        "msrv = \"1.99\"\ndisallowed-methods = [\n",
    )
}

/// A row whose Never list is `clock` alone needs no file: the root bans both clocks.
pub(super) fn clock_only_row() -> Inspection {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "clock");
    run_repo(
        "clock-only-row",
        &Repo {
            architecture: Some(&architecture),
            ..Repo::default()
        },
    )
}

/// `clauseo-extra` has no row and no file; with protocol's row gone, nothing is held.
pub(super) fn crate_without_row() -> Inspection {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "tauri");
    run_repo(
        "crate-without-row",
        &Repo {
            architecture: Some(&architecture),
            ..Repo::default()
        },
    )
}

pub(super) fn list_missing() -> Inspection {
    run_repo(
        "list-missing",
        &Repo {
            list: Input::Missing,
            ..Repo::default()
        },
    )
}

pub(super) fn root_missing() -> Inspection {
    run_repo(
        "root-missing",
        &Repo {
            root: Input::Missing,
            ..Repo::default()
        },
    )
}

pub(super) fn list_unknown_key() -> Inspection {
    run_repo(
        "list-unknown-key",
        &Repo {
            list: Input::Text(
                "disallowed-methods = [\n  { path = \"std::fs::read\", reason = \"no files\" },\n]\n\
                 disallowed-macros = []\n",
            ),
            ..Repo::default()
        },
    )
}

pub(super) fn list_entry_problems() -> Inspection {
    run_repo(
        "list-entry-problems",
        &Repo {
            list: Input::Text(
                "disallowed-methods = [\n  { path = \"std::fs::read\", reason = \"no files\" },\n  \
                 { path = \"std::fs::read\", reason = \" \" },\n]\n",
            ),
            ..Repo::default()
        },
    )
}

pub(super) fn root_array_is_a_string() -> Inspection {
    run_repo(
        "root-array-is-a-string",
        &Repo {
            root: Input::Text("msrv = \"1.99\"\ndisallowed-types = \"std::fs::File\"\n"),
            ..Repo::default()
        },
    )
}

#[test]
fn every_row_counts_when_no_member_is_held() {
    let dir = scratch("count", &Repo::default());
    let found = run(&Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
        dependencies: Default::default(),
        load_resolved_graph: false,
    });
    assert_eq!(found, Inspection::new(3, Vec::new()));
}

#[test]
fn a_missing_table_inspects_nothing() {
    let dir = Scratch::new("core-io-no-table");
    assert_eq!(
        run(&Context {
            root: dir.to_path_buf(),
            members: Vec::new(),
            dependencies: Default::default(),
            load_resolved_graph: false,
        }),
        Inspection::default()
    );
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
